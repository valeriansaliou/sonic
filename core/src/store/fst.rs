// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use fst::automaton::AlwaysMatch;
use fst::set::Stream as FSTStream;
use fst::{
    Automaton, Error as FSTError, IntoStreamer, Set as FSTSet, SetBuilder as FSTSetBuilder,
    Streamer,
};
use fst_levenshtein::Levenshtein;
use fst_regex::Regex;
use hashbrown::{HashMap, HashSet};
use indexmap::IndexMap;
use radix::RadixNum;
use regex_syntax::escape as regex_escape;
use std::collections::VecDeque;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::iter::FromIterator;
use std::path::{Path, PathBuf};
use std::str;
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::thread;
use std::time::{Duration, SystemTime};

use super::generic::{StoreGeneric, StoreGenericActionBuilder, StoreGenericBuilder};
use super::keyer::StoreKeyerHasher;
use crate::lexer::ranges::LexerRegexRange;
use crate::store::generic::{proceed_acquire_cache, proceed_acquire_open, proceed_janitor};

// NOTE: This type cannot be generic over a lifetime as spawning threads would
//   force it to be `'static`.
#[derive(Clone)]
pub struct StoreFSTPool {
    fst_store_config: Arc<crate::config::ConfigStoreFST>,
    // NOTE: This shouldn’t be here, but until a big rewrite let’s not care.
    pub fst_action_config: StoreFSTActionConfig,
    graph_pool: Arc<RwLock<HashMap<StoreFSTKey, StoreFSTBox>>>,
    graph_acquire_lock: Arc<Mutex<()>>,
    graph_rebuild_lock: Arc<Mutex<()>>,
    graph_access_lock: Arc<RwLock<()>>,
    graph_consolidate: Arc<RwLock<HashSet<StoreFSTKey>>>,
}

pub struct StoreFSTBuilder<'build> {
    fst_store_config: &'build crate::config::ConfigStoreFST,
    // NOTE: This shouldn’t be here, but until a big rewrite let’s not care.
    fst_action_config: StoreFSTActionConfig,
    graph_consolidate: Arc<RwLock<HashSet<StoreFSTKey>>>,
}

pub struct StoreFST {
    graph: FSTSet,
    target: StoreFSTKey,
    pending: StoreFSTPending,
    last_used: Arc<RwLock<SystemTime>>,
    last_consolidated: Arc<RwLock<SystemTime>>,
    graph_consolidate: Arc<RwLock<HashSet<StoreFSTKey>>>,
    // NOTE: This shouldn’t be here, but until a big rewrite let’s not care.
    action_config: StoreFSTActionConfig,
}

#[derive(Default)]
pub struct StoreFSTPending {
    pop: Arc<RwLock<HashSet<Vec<u8>>>>,
    push: Arc<RwLock<HashSet<Vec<u8>>>>,
}

pub struct StoreFSTActionBuilder<'build> {
    pub fst_store_config: &'build crate::config::ConfigStoreFST,
}

#[derive(PartialEq, Eq, Hash, Clone, Copy)]
pub struct StoreFSTKey {
    collection_hash: StoreFSTAtom,
    bucket_hash: StoreFSTAtom,
}

pub struct StoreFSTMisc;

#[derive(Copy, Clone)]
enum StoreFSTPathMode {
    Permanent,
    Temporary,
    Backup,
}

type StoreFSTAtom = u32;
type StoreFSTBox = Arc<StoreFST>;

#[derive(Debug, Clone, Copy)]
pub struct StoreFSTActionConfig {
    pub prefix_matching_enabled: bool,
    pub fuzzy_matching_enabled: bool,
}

impl Default for StoreFSTActionConfig {
    fn default() -> Self {
        Self {
            prefix_matching_enabled: true,
            fuzzy_matching_enabled: true,
        }
    }
}

const WORD_LIMIT_LENGTH: usize = 40;
const ATOM_HASH_RADIX: usize = 16;

impl StoreFSTPathMode {
    fn extension(&self) -> &'static str {
        match self {
            StoreFSTPathMode::Permanent => ".fst",
            StoreFSTPathMode::Temporary => ".fst.tmp",
            StoreFSTPathMode::Backup => ".fst.bck",
        }
    }
}

impl StoreFSTPool {
    pub fn new(
        fst_store_config: Arc<crate::config::ConfigStoreFST>,
        fst_action_config: StoreFSTActionConfig,
    ) -> Self {
        Self {
            fst_store_config,
            fst_action_config,
            graph_pool: Arc::default(),
            graph_acquire_lock: Arc::default(),
            graph_rebuild_lock: Arc::default(),
            graph_access_lock: Arc::default(),
            graph_consolidate: Arc::default(),
        }
    }

    pub fn count(&self) -> (usize, usize) {
        (
            self.graph_pool.read().unwrap().len(),
            self.graph_consolidate.read().unwrap().len(),
        )
    }

    pub fn lock_read_access<'a>(&'a self) -> RwLockReadGuard<'a, ()> {
        self.graph_access_lock.read().unwrap()
    }

    pub fn lock_write_access<'a>(&'a self) -> RwLockWriteGuard<'a, ()> {
        self.graph_access_lock.write().unwrap()
    }

    pub fn acquire<T: AsRef<str>>(&self, collection: T, bucket: T) -> Result<StoreFSTBox, ()> {
        let (collection, bucket) = (collection.as_ref(), bucket.as_ref());

        let pool_key = StoreFSTKey::from_str(collection, bucket);

        // Freeze acquire lock, and reference it in context
        // Notice: this prevents two graphs on the same collection to be opened at the same time.
        let _acquire = self.graph_acquire_lock.lock().unwrap();

        // Acquire a thread-safe store pool reference in read mode
        let graph_pool_read = self.graph_pool.read().unwrap();

        if let Some(store_fst) = graph_pool_read.get(&pool_key) {
            proceed_acquire_cache("fst", collection, pool_key, store_fst)
        } else {
            tracing::info!(
                ?pool_key,
                ?collection,
                ?bucket,
                "fst store not in pool, opening it"
            );

            // Important: we need to drop the read reference first, to avoid dead-locking \
            //   when acquiring the RWLock in write mode in this block.
            drop(graph_pool_read);

            let builder = StoreFSTBuilder {
                fst_store_config: &self.fst_store_config,
                graph_consolidate: Arc::clone(&self.graph_consolidate),
                fst_action_config: self.fst_action_config,
            };

            proceed_acquire_open(
                "fst",
                collection,
                pool_key,
                &self.graph_pool,
                &builder,
                None,
                |_| {},
            )
        }
    }

    pub fn janitor(&self, filter: impl Fn(&StoreFSTKey) -> bool) {
        proceed_janitor(
            "fst",
            &self.graph_pool,
            self.fst_store_config.pool.inactive_after,
            &self.graph_access_lock,
            filter,
        )
    }

    pub fn backup(&self, path: &Path) -> Result<(), io::Error> {
        tracing::debug!("backing up all fst stores to path: {path:?}");

        // Create backup directory (full path)
        fs::create_dir_all(path)?;

        // Proceed dump action (backup)
        self.dump_action(
            "backup",
            StoreFSTPathMode::Permanent,
            &self.fst_store_config.path,
            path,
            &Self::backup_item,
        )
    }

    pub fn restore(&self, path: &Path) -> Result<(), io::Error> {
        tracing::debug!("restoring all fst stores from path: {path:?}");

        // Proceed dump action (restore)
        self.dump_action(
            "restore",
            StoreFSTPathMode::Backup,
            path,
            &self.fst_store_config.path,
            &Self::restore_item,
        )
    }

    pub fn consolidate(&self, force: bool, filter: impl Fn(&StoreFSTKey) -> bool) {
        tracing::debug!("scanning for fst store pool items to consolidate");

        // Notice: we do not consolidate all items at each tick, we try to even out multiple \
        //   consolidation tasks over time. This lowers the overall HZ of the tasker system for \
        //   certain heavy tasks, which is better to spread out consolidation steps over time over \
        //   a large number of very active buckets.

        // Acquire rebuild lock, and reference it in context
        // Notice: this prevents two consolidate operations to be executed at the same time.
        let _rebuild = self.graph_rebuild_lock.lock().unwrap();

        // Exit trap: Register is empty? Abort there.
        if self.graph_consolidate.read().unwrap().is_empty() {
            tracing::info!("no fst store pool items to consolidate in register");

            return;
        }

        // Step 1: List keys to be consolidated
        let mut keys_consolidate: Vec<StoreFSTKey> = Vec::new();

        {
            // Acquire access lock (in blocking write mode), and reference it in context
            // Notice: this prevents store to be acquired from any context
            let _access = self.graph_access_lock.write().unwrap();

            let (graph_pool_read, graph_consolidate_read) = (
                self.graph_pool.read().unwrap(),
                self.graph_consolidate.read().unwrap(),
            );

            for key in graph_consolidate_read.iter().filter(|k| filter(k)) {
                if let Some(store) = graph_pool_read.get(key) {
                    // Important: be lenient with system clock going back to a past duration, \
                    //   since we may be running in a virtualized environment where clock is not \
                    //   guaranteed to be monotonic. This is done to avoid poisoning associated \
                    //   mutexes by crashing on unwrap().
                    let not_consolidated_for = store
                        .last_consolidated
                        .read()
                        .unwrap()
                        .elapsed()
                        .unwrap_or_else(|err| {
                            tracing::error!("fst key {key:?} last consolidated duration clock issue, zeroing: {err:?}");

                            // Assuming a zero seconds fallback duration
                            Duration::from_secs(0)
                        });

                    if force
                        || not_consolidated_for.as_secs()
                            >= self.fst_store_config.graph.consolidate_after
                    {
                        tracing::info!(
                            "fst key {key:?} not consolidated for {not_consolidated_for:.1?}, may consolidate"
                        );

                        keys_consolidate.push(*key);
                    } else {
                        tracing::debug!(
                            "fst key: {key:?} not consolidated for {not_consolidated_for:.1?}, no consolidate"
                        );
                    }
                }
            }
        }

        // Exit trap: Nothing to consolidate yet? Abort there.
        if keys_consolidate.is_empty() {
            tracing::info!("no fst store pool items need to consolidate at the moment");

            return;
        }

        // Step 2: Clear keys to be consolidated from register
        {
            // Acquire access lock (in blocking write mode), and reference it in context
            // Notice: this prevents store to be acquired from any context
            let _access = self.graph_access_lock.write().unwrap();

            let mut graph_consolidate_write = self.graph_consolidate.write().unwrap();

            for key in &keys_consolidate {
                graph_consolidate_write.remove(key);

                tracing::debug!("fst key {key:?} cleared from consolidate register");
            }
        }

        // Step 3: Consolidate FSTs, one-by-one (sequential locking; this avoids global locks)
        let mut stats = ConsolidateStats::default();

        for key in &keys_consolidate {
            // As we may be renaming the FST file, ensure no consumer out of this is
            // trying to access the FST file as it gets processed. This also waits for
            // current consumers to finish reading the FST, and prevents any new
            // consumer from opening it while we are not done there.
            let access_guard = self.graph_access_lock.write().unwrap();

            let do_close = if let Some(store) = self.graph_pool.read().unwrap().get(key) {
                tracing::debug!("fst key: {key:?} consolidate started");

                #[allow(
                    clippy::unnecessary_lazy_evaluations,
                    reason = "Ensures errors are handled"
                )]
                let should_close = self
                    .consolidate_item(store, &mut stats)
                    .unwrap_or_else(|()| false);

                tracing::debug!("fst key: {key:?} consolidate complete");

                // Should close this FST?
                should_close
            } else {
                false
            };

            // Nuke old opened FST?
            // NOTE: Last consolidated date will be bumped to a new date in the future
            //   when a push or pop operation will be done, thus effectively scheduling
            //   a consolidation in the future properly.
            // NOTE: We remove this one early as to release write lock early
            if do_close {
                self.graph_pool.write().unwrap().remove(key);
            }

            // Release lock before yielding.
            drop(access_guard);

            // Give a bit of time to other threads before continuing (a consolidate operation
            // must not block all other threads until it completes); this method tells the
            // thread scheduler to give a bit of priority to other threads, and get back
            // to this thread's work when other threads are done. On large setups, this
            // loop can starve other threads due to the locks used (unfortunately they
            // are all necessary).
            thread::yield_now();
        }

        tracing::info!(
            ?stats,
            "Done scanning for fst store pool items to consolidate"
        );
    }

    #[allow(clippy::type_complexity)]
    fn dump_action(
        &self,
        action: &str,
        path_mode: StoreFSTPathMode,
        read_path: &Path,
        write_path: &Path,
        fn_item: &dyn Fn(&Self, &Path, &Path, &str, &str) -> Result<(), io::Error>,
    ) -> Result<(), io::Error> {
        let fst_extension = path_mode.extension();

        // Iterate on FST collections.
        'collections: for collection_entry in fs::read_dir(read_path)? {
            let collection_entry = collection_entry?;

            // Actual collection found?
            let file_type = collection_entry.file_type()?;
            if !file_type.is_dir() {
                tracing::trace!(
                    ?file_type,
                    "Found non-directory entry in {read_path:?}, ignoring"
                );
                continue 'collections;
            }

            let file_name = collection_entry.file_name();
            let Some(collection_name) = file_name.to_str() else {
                tracing::warn!(
                    file_name_bytes = ?file_name.as_encoded_bytes(),
                    "Found invalid entry in {read_path:?}, ignoring"
                );
                continue 'collections;
            };

            tracing::debug!("fst collection ongoing {action}: {collection_name}");

            // Create write folder for collection.
            fs::create_dir_all(write_path.join(&collection_name))?;

            // Iterate on FST collection buckets.
            let buckets_path = read_path.join(&collection_name);
            'buckets: for bucket_entry in fs::read_dir(&buckets_path)? {
                let bucket_entry = bucket_entry?;

                // Actual bucket found?
                let file_type = collection_entry.file_type()?;
                if !file_type.is_file() {
                    tracing::trace!(
                        ?file_type,
                        "Found non-file entry in {buckets_path:?}, ignoring"
                    );
                    continue 'buckets;
                }

                let file_name = collection_entry.file_name();
                let Some(bucket_file_name) = file_name.to_str() else {
                    tracing::warn!(
                        file_name_bytes = ?file_name.as_encoded_bytes(),
                        "Found invalid entry in {buckets_path:?}, ignoring"
                    );
                    continue 'buckets;
                };

                let Some(bucket_name) = bucket_file_name.strip_suffix(fst_extension) else {
                    tracing::trace!(
                        file_name_bytes = ?file_name.as_encoded_bytes(),
                        "Ignoring {bucket_file_name:?}: wrong extension (expected: {fst_extension:?})"
                    );
                    continue 'buckets;
                };

                tracing::debug!("fst bucket ongoing {action}: {collection_name}/{bucket_name}");

                fn_item(
                    self,
                    write_path,
                    &bucket_entry.path(),
                    &collection_name,
                    bucket_name,
                )?;
            }
        }

        Ok(())
    }

    fn backup_item(
        &self,
        backup_path: &Path,
        _origin_path: &Path,
        collection_name: &str,
        bucket_name: &str,
    ) -> Result<(), io::Error> {
        // Acquire access lock (in blocking write mode), and reference it in context
        // Notice: this prevents store to be acquired from any context.
        let _access = self.graph_access_lock.write().unwrap();

        // Generate path to FST backup.
        let fst_backup_path = {
            let ext = StoreFSTPathMode::Backup.extension();
            assert!(ext.starts_with("."));
            backup_path
                .join(collection_name)
                .join(format!("{bucket_name}{ext}"))
        };

        tracing::debug!(
            "fst bucket {collection_name}/{bucket_name} backing up to path: {fst_backup_path:?}"
        );

        // Erase any previously-existing FST backup.
        fs::remove_file(&fst_backup_path).ok();

        // Stream actual FST data to FST backup.
        let backup_fst_file = File::create(&fst_backup_path)?;
        let mut backup_fst_writer = BufWriter::new(backup_fst_file);

        // Convert names to hashes (as names are hashes encoded as base-16
        // strings, but we need them as proper integers).
        let (Ok(collection_radix), Ok(bucket_radix)) = (
            RadixNum::from_str(collection_name, ATOM_HASH_RADIX),
            RadixNum::from_str(bucket_name, ATOM_HASH_RADIX),
        ) else {
            // TODO(errors): Return an error.
            return Ok(());
        };

        let (Ok(collection_hash), Ok(bucket_hash)) =
            (collection_radix.as_decimal(), bucket_radix.as_decimal())
        else {
            // TODO(errors): Return an error.
            return Ok(());
        };

        let origin_fst = StoreFSTBuilder::open(
            collection_hash as StoreFSTAtom,
            bucket_hash as StoreFSTAtom,
            &self.fst_store_config,
        )
        .map_err(|error| io::Error::other(format!("Graph open failure: {error:?}")))?;

        let mut origin_fst_stream = origin_fst.stream();

        let mut count_words = 0;
        while let Some(word) = origin_fst_stream.next() {
            count_words += 1;

            // Write word, and append a new line.
            backup_fst_writer.write_all(word)?;
            backup_fst_writer.write_all(b"\n")?;
        }

        tracing::info!(
            "fst bucket {collection_name}/{bucket_name} backed up to path: {fst_backup_path:?} ({count_words} words)"
        );

        Ok(())
    }

    fn restore_item(
        &self,
        _backup_path: &Path,
        origin_path: &Path,
        collection_name: &str,
        bucket_name: &str,
    ) -> Result<(), io::Error> {
        // Acquire access lock (in blocking write mode) to prevent store from
        // being acquired from any context.
        let _access = self.graph_access_lock.write().unwrap();

        tracing::debug!(
            "fst bucket {collection_name}/{bucket_name} restoring from path: {origin_path:?}"
        );

        // Convert names to hashes (as names are hashes encoded as base-16
        // strings, but we need them as proper integers).
        let (Ok(collection_radix), Ok(bucket_radix)) = (
            RadixNum::from_str(collection_name, ATOM_HASH_RADIX),
            RadixNum::from_str(bucket_name, ATOM_HASH_RADIX),
        ) else {
            // TODO(errors): Return an error.
            return Ok(());
        };

        let (Ok(collection_hash), Ok(bucket_hash)) =
            (collection_radix.as_decimal(), bucket_radix.as_decimal())
        else {
            // TODO(errors): Return an error.
            return Ok(());
        };

        // Force a FST store close.
        self.close(collection_hash as StoreFSTAtom, bucket_hash as StoreFSTAtom);

        // Generate path to FST.
        let fst_path = self.fst_store_config.path(
            StoreFSTPathMode::Permanent,
            collection_hash as StoreFSTAtom,
            Some(bucket_hash as StoreFSTAtom),
        );

        // Remove existing FST data?
        if fst_path.exists() {
            fs::remove_file(&fst_path)?;
        }

        // Stream backup words to restored FST.
        let fst_writer = BufWriter::new(File::create(&fst_path)?);
        let fst_backup_reader = BufReader::new(File::open(&origin_path)?);

        let mut fst_builder = FSTSetBuilder::new(fst_writer).map_err(|error| {
            io::Error::other(format!("Graph restore builder failure: {error:?}"))
        })?;

        for word in fst_backup_reader.lines() {
            let word = word?;

            fst_builder.insert(word).map_err(|error| {
                io::Error::other(format!("Graph restore word insert failure: {error:?}"))
            })?;
        }

        fst_builder.finish().map_err(|error| {
            io::Error::other(format!("Graph restore finish failure: {error:?}"))
        })?;

        tracing::info!(
            "fst bucket: {collection_name}/{bucket_name} restored to path: {fst_path:?} from backup: {origin_path:?}"
        );

        Ok(())
    }
}

#[derive(Debug, Default)]
struct ConsolidateStats {
    count_moved: usize,
    count_pushed: usize,
    count_popped: usize,
}

impl StoreFSTPool {
    fn consolidate_item(
        &self,
        store: &StoreFSTBox,
        stats: &mut ConsolidateStats,
    ) -> Result<bool, ()> {
        // Acquire write references to pending sets.
        let mut pending_push_write = store.pending.push.write().unwrap();
        let mut pending_pop_write = store.pending.pop.write().unwrap();

        // Do consolidate? (any change committed)
        // NOTE: If both pending sets are empty do not consolidate as there may have
        //   been a push then a pop of this push, nulling out any committed change.
        if pending_push_write.is_empty() && pending_pop_write.is_empty() {
            return Ok(false);
        }

        // Read old FST (or default to empty FST).
        let old_fst = StoreFSTBuilder::open(
            store.target.collection_hash,
            store.target.bucket_hash,
            &self.fst_store_config,
        )
        .map_err(|error| tracing::error!("Error opening old fst: {error:?}"))?;

        // Initialize the new FST (temporary).
        let bucket_tmp_path = self.fst_store_config.path(
            StoreFSTPathMode::Temporary,
            store.target.collection_hash,
            Some(store.target.bucket_hash),
        );

        let bucket_tmp_path_parent = bucket_tmp_path.parent().unwrap();

        fs::create_dir_all(&bucket_tmp_path_parent).map_err(|error| tracing::error!(
            "Error initializing temporary fst directory at path {bucket_tmp_path_parent:?}: {error:?}"
        ))?;

        // Erase any previously-existing temporary FST (e.g. process stopped while
        // writing the temporary FST); there is no guarantee this succeeds.
        fs::remove_file(&bucket_tmp_path).ok();

        let tmp_fst_file = File::create(&bucket_tmp_path).map_err(|error| {
            tracing::error!(
                "Error initializing temporary fst at path {bucket_tmp_path:?}: {error:?}"
            )
        })?;

        let tmp_fst_writer = BufWriter::new(tmp_fst_file);

        // Create a builder that can be used to insert new key-value pairs.
        let mut tmp_fst_builder = FSTSetBuilder::new(tmp_fst_writer).map_err(|error| {
            tracing::error!(
                "Error starting building temporary fst at path {bucket_tmp_path:?}: {error:?}"
            )
        })?;

        // Convert push keys to an ordered vector.
        // NOTE: We must go from a `Vec` to a `VecDeque` to sort values,
        //   which is a requirement for FST insertions.
        let mut ordered_push_vec: Vec<&[u8]> =
            Vec::from_iter(pending_push_write.iter().map(|item| item.as_ref()));

        ordered_push_vec.sort();

        let mut ordered_push: VecDeque<&[u8]> = VecDeque::from_iter(ordered_push_vec);

        // Append words not in pop list to new FST (i.e. old words minus pop words).
        let mut old_fst_stream = old_fst.stream();

        'old: while let Some(old_fst_word) = old_fst_stream.next() {
            // Append new words from front? (i.e. push words)
            // NOTE: As an FST is ordered, inserts would fail if they are
            //   committed out-of-order. Thus, the only way to check for
            //   order is there.
            // NOTE: A quick check is done before engaging in the loop, to
            //   prevent any de-optimized jump instruction, as we may call
            //   this code block a lot on large FSTs, and the loop should not
            //   be engaged that often on stabilized FSTs (i.e. mature FSTs).
            if let Some(push_first_ref) = ordered_push.front() {
                // Engage the loop?
                if *push_first_ref <= old_fst_word {
                    while let Some(push_front_ref) = ordered_push.front() {
                        if *push_front_ref > old_fst_word {
                            // Important: stop loop on next front item (always the same).
                            break;
                        }

                        // Pop front item and consume it.
                        // SAFETY: As we validated previously that there
                        //   is a front value, this unwrap is safe.
                        let push_front = ordered_push.pop_front().unwrap();

                        if StoreFSTMisc::check_over_limits(
                            tmp_fst_builder.bytes_written() as usize,
                            stats.count_pushed + stats.count_moved,
                            &self.fst_store_config.graph,
                        ) {
                            // FST cannot accept more items (limits reached).
                            tracing::warn!("Limit reached on new from old in fst");

                            // Important: stop the main loop (limit reached).
                            break 'old;
                        }

                        match tmp_fst_builder.insert(push_front) {
                            // Word inserted in FST.
                            Ok(()) => stats.count_pushed += 1,
                            // Could not insert word in FST.
                            Err(error) => {
                                tracing::error!("Failed inserting new from old in fst: {error:?}")
                            }
                        }

                        // Continue scanning next word (may also come
                        // before this FST word in order).
                        continue;
                    }
                }
            }

            // Restore old word (if not popped).
            if pending_pop_write.contains(old_fst_word) {
                stats.count_popped += 1;
            } else {
                if StoreFSTMisc::check_over_limits(
                    tmp_fst_builder.bytes_written() as usize,
                    stats.count_pushed + stats.count_moved,
                    &self.fst_store_config.graph,
                ) {
                    // FST cannot accept more items (limits reached).
                    tracing::warn!("Limit reached on old word in fst");

                    // Important: stop the main loop (limit reached).
                    break 'old;
                }

                match tmp_fst_builder.insert(old_fst_word) {
                    // Word moved to FST.
                    Ok(()) => stats.count_moved += 1,
                    // Could not move word to FST.
                    Err(error) => tracing::error!("Failed inserting old word in fst: {error:?}"),
                }
            }
        }

        // Complete FST with last pushed items.
        // NOTE: This is necessary if the FST was empty, or if we have push
        //   items that come after the last ordered word of the FST.
        while let Some(push_front) = ordered_push.pop_front() {
            if StoreFSTMisc::check_over_limits(
                tmp_fst_builder.bytes_written() as usize,
                stats.count_pushed + stats.count_moved,
                &self.fst_store_config.graph,
            ) {
                // FST cannot accept more items (limits reached).
                tracing::warn!("Limit reached on new word from complete in fst");

                // Important: stop the main loop (limit reached).
                break;
            }

            match tmp_fst_builder.insert(push_front) {
                // Word inserted in FST.
                Ok(()) => stats.count_pushed += 1,
                // Could not insert word in FST.
                Err(error) => {
                    tracing::error!("Failed inserting new word from complete in fst: {error:?}")
                }
            }
        }

        // Finish building new FST.
        let should_close = match tmp_fst_builder.finish() {
            Ok(()) => {
                // Replace old FST with new FST (this nukes the old FST).
                // NOTE: There is no need to re-open the new FST, as it will be
                //   automatically opened on its next access.
                let bucket_final_path = self.fst_store_config.path(
                    StoreFSTPathMode::Permanent,
                    store.target.collection_hash,
                    Some(store.target.bucket_hash),
                );

                // Proceed temporary FST to final FST path rename?
                match fs::rename(&bucket_tmp_path, &bucket_final_path) {
                    Ok(()) => tracing::info!("Done consolidate fst at path {bucket_final_path:?}"),
                    Err(error) => tracing::error!(
                        "Error consolidating fst at path {bucket_final_path:?}: {error:?}"
                    ),
                }

                // Should close open store reference to old FST.
                true
            }
            Err(error) => {
                tracing::error!(
                    "Error finishing building temporary fst at path {bucket_tmp_path:?}: {error:?}"
                );

                false
            }
        };

        // Clear all pending sets.
        pending_push_write.clear();
        pending_pop_write.clear();

        Ok(should_close)
    }

    fn close(&self, collection_hash: StoreFSTAtom, bucket_hash: StoreFSTAtom) {
        tracing::debug!("Closing fst graph <{collection_hash:x}>/<{bucket_hash:x}>");

        let bucket_target = StoreFSTKey::from_atom(collection_hash, bucket_hash);

        (self.graph_pool.write().unwrap()).remove(&bucket_target);
        (self.graph_consolidate.write().unwrap()).remove(&bucket_target);
    }
}

impl<'build> StoreFSTBuilder<'build> {
    fn open(
        collection_hash: StoreFSTAtom,
        bucket_hash: StoreFSTAtom,
        fst_store_config: &crate::config::ConfigStoreFST,
    ) -> Result<FSTSet, FSTError> {
        tracing::debug!("Opening fst graph for <{collection_hash:x}>/<{bucket_hash:x}>");

        let collection_bucket_path = fst_store_config.path(
            StoreFSTPathMode::Permanent,
            collection_hash,
            Some(bucket_hash),
        );

        if collection_bucket_path.exists() {
            // Open graph at path for collection
            // SAFETY: This is unsafe, as loaded memory is a memory-mapped file, that cannot be
            //   guaranteed not to be muted while we own a read handle to it. Though, we use
            //   higher-level locking mechanisms on all callers of this method, so we are safe.
            unsafe { FSTSet::from_path(collection_bucket_path) }
        } else {
            // FST does not exist on disk; generate an empty FST for now
            // (until a consolidation task occurs and populates the on-disk FST).
            FSTSet::from_iter(std::iter::empty::<&str>())
        }
    }
}

impl crate::config::ConfigStoreFST {
    fn path(
        &self,
        mode: StoreFSTPathMode,
        collection_hash: StoreFSTAtom,
        bucket_hash: Option<StoreFSTAtom>,
    ) -> PathBuf {
        let mut final_path = self.path.join(format!("{collection_hash:x}"));

        if let Some(bucket_hash) = bucket_hash {
            final_path = final_path.join(format!("{bucket_hash:x}{ext}", ext = mode.extension()));
        }

        final_path
    }
}

impl<'build> StoreGenericBuilder<StoreFSTKey, StoreFST> for StoreFSTBuilder<'build> {
    type Options = ();

    fn build(
        &self,
        pool_key: StoreFSTKey,
        _override_options: impl FnOnce(&mut Self::Options),
    ) -> Result<StoreFST, ()> {
        let graph = Self::open(
            pool_key.collection_hash,
            pool_key.bucket_hash,
            self.fst_store_config,
        )
        .map_err(|error| tracing::error!("Failed opening fst: {error:?}"))?;

        let now = SystemTime::now();

        Ok(StoreFST {
            graph,
            target: pool_key,
            pending: StoreFSTPending::default(),
            last_used: Arc::new(RwLock::new(now)),
            last_consolidated: Arc::new(RwLock::new(now)),
            graph_consolidate: Arc::clone(&self.graph_consolidate),
            action_config: self.fst_action_config,
        })
    }
}

impl StoreFST {
    pub fn cardinality(&self) -> usize {
        self.graph.len()
    }

    pub fn as_stream(&self) -> FSTStream<'_, AlwaysMatch> {
        self.graph.into_stream()
    }

    pub fn lookup_begins_(&self, word: &str) -> Result<FSTStream<'_, Regex>, ()> {
        // NOTE: This regex maps over an unicode range, for speed reasons at scale.
        //   We found out that the 'match any' syntax ('.*') was super-slow. Using the restrictive
        //   syntax below divided the cost of e.g. a search query by 2. The regex below has been
        //   found out to be nearly zero-cost to compile and execute, for whatever reason.
        // Regex format: '{escaped_word}([{unicode_range}]*)'
        let mut regex_str = regex_escape(word);

        regex_str.push('(');

        LexerRegexRange::from(word)
            .unwrap_or_default()
            .write_to(&mut regex_str)
            // Regex write failed? (this should not happen)
            .map_err(|error| tracing::error!(
                "Could not lookup word in fst via 'begins': {word:?} because regex write failed: {error:?}"
            ))?;

        regex_str.push_str("*)");

        // Proceed word lookup.
        tracing::debug!("Looking-up word in fst via 'begins': {word:?} with regex: {regex_str:?}");

        let regex = Regex::new(&regex_str).map_err(|_error| ())?;

        Ok(self.graph.search(regex).into_stream())
    }

    pub fn lookup_typos_(
        &self,
        word: &str,
        typo_factor: u32,
    ) -> Result<FSTStream<'_, Levenshtein>, ()> {
        tracing::debug!(
            "Looking-up word in fst via 'typos': {word:?} with typo factor: {typo_factor:?}"
        );

        let fuzzy = Levenshtein::new(word, typo_factor).map_err(|_error| ())?;

        Ok(self.graph.search(fuzzy).into_stream())
    }

    pub fn should_consolidate(&self) {
        let target = self.target;

        // Check if not already scheduled.
        if self.graph_consolidate.read().unwrap().contains(&target) {
            tracing::debug!("Graph consolidation already scheduled on pool key: {target}");
            return;
        };

        // Schedule target for next consolidation tick (i.e. collection + bucket tuple).
        self.graph_consolidate.write().unwrap().insert(target);

        // Bump “last consolidated” time, effectively de-bouncing consolidation
        // to a fixed and predictable tick time in the future.
        let mut last_consolidated_value = self.last_consolidated.write().unwrap();

        *last_consolidated_value = SystemTime::now();

        // Perform an early drop of the lock (frees up write lock early).
        drop(last_consolidated_value);

        tracing::info!("Graph consolidation scheduled on pool key: {target}");
    }
}

impl StoreGeneric for StoreFST {
    fn ref_last_used(&self) -> &RwLock<SystemTime> {
        &self.last_used
    }
}

impl StoreFSTPool {
    pub fn erase<T: AsRef<str>>(&self, collection: T, bucket: Option<T>) -> Result<u32, ()> {
        self.dispatch_erase("fst", collection, bucket)
    }
}

impl StoreGenericActionBuilder for StoreFSTPool {
    fn proceed_erase_collection(&self, collection_name: &str) -> Result<u32, ()> {
        let path_mode = StoreFSTPathMode::Permanent;

        let collection_atom = StoreKeyerHasher::to_compact(collection_name);
        let collection_path = self.fst_store_config.path(path_mode, collection_atom, None);

        // Force a FST graph close (on all contained buckets)
        // NOTE: we first need to scan for opened buckets in-memory, as not all FSTs may be
        //   committed to disk; thus some FST stores that exist in-memory may not exist on-disk.
        // TODO(perf): Instead of collection into a `Vec` just to check `is_empty` and
        //   lock only if necessary, use a `LazyCell` to do the same in a single step.
        let mut bucket_atoms: Vec<StoreFSTAtom> = Vec::new();

        {
            let graph_pool_read = self.graph_pool.read().unwrap();

            for target_key in graph_pool_read.keys() {
                if target_key.collection_hash == collection_atom {
                    bucket_atoms.push(target_key.bucket_hash);
                }
            }
        }

        if !bucket_atoms.is_empty() {
            tracing::trace!(
                "Will force-close {nbuckets} fst buckets for collection {collection_name:?}",
                nbuckets = bucket_atoms.len()
            );

            let mut graph_pool_write = self.graph_pool.write().unwrap();
            let mut graph_consolidate_write = self.graph_consolidate.write().unwrap();

            for bucket_atom in bucket_atoms {
                tracing::debug!(
                    "fst bucket graph force close for bucket: {collection_name}/<{bucket_atom:x}>"
                );

                let bucket_target = StoreFSTKey::from_atom(collection_atom, bucket_atom);

                graph_pool_write.remove(&bucket_target);
                graph_consolidate_write.remove(&bucket_target);
            }
        }

        // Remove all on-disk FSTs.
        if collection_path.exists() {
            tracing::trace!(
                "fst collection store exists, erasing: {collection_name}/* at path: {collection_path:?}"
            );

            // Remove FST graph storage from filesystem.
            match fs::remove_dir_all(&collection_path) {
                Ok(()) => {
                    tracing::info!(?collection_name, "Done with fst collection erasure");

                    Ok(1)
                }
                Err(error) => {
                    tracing::error!(
                        "Error erasing fst collection at path {collection_path:?}: {error:?}"
                    );

                    Err(())
                }
            }
        } else {
            tracing::debug!(
                "fst collection store does not exist, consider already erased: {collection_name}/* at path: {collection_path:?}"
            );

            Ok(0)
        }
    }

    fn proceed_erase_bucket(&self, collection_name: &str, bucket_name: &str) -> Result<u32, ()> {
        tracing::debug!(
            "Sub-erase on fst bucket {bucket_name:?} for collection {collection_name:?}"
        );

        let (collection_atom, bucket_atom) = (
            StoreKeyerHasher::to_compact(collection_name),
            StoreKeyerHasher::to_compact(bucket_name),
        );

        let bucket_path = self.fst_store_config.path(
            StoreFSTPathMode::Permanent,
            collection_atom,
            Some(bucket_atom),
        );

        // Force a FST graph close.
        self.close(collection_atom, bucket_atom);

        // Remove on-disk FST.
        if bucket_path.exists() {
            tracing::trace!(
                "fst bucket graph exists, erasing: {collection_name}/{bucket_name} at path: {bucket_path:?}"
            );

            // Remove FST graph storage from filesystem.
            match fs::remove_file(&bucket_path) {
                Ok(()) => {
                    tracing::info!(
                        ?collection_name,
                        ?bucket_name,
                        "Done with fst bucket erasure"
                    );

                    Ok(1)
                }
                Err(error) => {
                    tracing::error!("Error erasing fst bucket at path {bucket_path:?}: {error:?}");

                    Err(())
                }
            }
        } else {
            tracing::debug!(
                "fst bucket graph does not exist, consider already erased: {collection_name}/{bucket_name} at path: {bucket_path:?}"
            );

            Ok(0)
        }
    }
}

impl StoreFST {
    pub fn push_word(&self, word: &str, fst_store_config: &crate::config::ConfigStoreFST) -> bool {
        // Word over limit? (abort, the FST does not perform well over large words)
        if Self::word_over_limit(word) {
            return false;
        }

        let word_bytes = word.as_bytes();

        // Nuke word from 'pop' set? (void a previous un-consolidated commit)
        if self.pending.pop.read().unwrap().contains(word_bytes) {
            self.pending.pop.write().unwrap().remove(word_bytes);
        }

        // Add word in 'push' set? (only if word is not in FST)
        // NOTE: also check whether FST is over limits or not from there, to avoid
        //   stacking words that could never be consolidated to final FST anyway.
        let graph_fst = self.graph.as_fst();

        if self.graph.contains(&word) {
            return false;
        }

        if StoreFSTMisc::check_over_limits(
            graph_fst.size(),
            graph_fst.len(),
            &fst_store_config.graph,
        ) {
            return false;
        }

        {
            let pending_push_guard = self.pending.push.read().unwrap();

            if pending_push_guard.contains(word_bytes)
                || pending_push_guard.len() >= fst_store_config.graph.max_words
            {
                return false;
            }
        }

        (self.pending.push.write().unwrap()).insert(word_bytes.to_vec());

        self.should_consolidate();

        true
    }

    pub fn pop_word(&self, word: &str) -> bool {
        // Word over limit? (abort, the FST does not perform well over large words)
        if Self::word_over_limit(word) {
            return false;
        }

        let word_bytes = word.as_bytes();

        // Nuke word from 'push' set? (void a previous un-consolidated commit)
        if self.pending.push.read().unwrap().contains(word_bytes) {
            self.pending.push.write().unwrap().remove(word_bytes);
        }

        if !self.graph.contains(word_bytes) {
            return false;
        }

        // Add word in 'pop' set? (only if word is in FST)
        if self.pending.pop.read().unwrap().contains(word_bytes) {
            return false;
        }

        (self.pending.pop.write().unwrap()).insert(word_bytes.to_vec());

        self.should_consolidate();

        true
    }

    pub fn suggest_words(
        &self,
        from_word: &str,
        // Length before stemming. Useful to apply fuzzy matching rules based
        // on user input.
        original_word_len: usize,
        limit: usize,
        max_typo_factor: Option<u32>,
    ) -> Option<impl ExactSizeIterator<Item = (String, u16)> + DoubleEndedIterator + use<>> {
        // Word over limit? (abort, the FST does not perform well over large words)
        if Self::word_over_limit(from_word) {
            return None;
        }

        let mut found_words: IndexMap<String, u16> = IndexMap::with_capacity(limit);

        if self.action_config.prefix_matching_enabled {
            // Try to complete provided word
            if let Some(stream) = self.lookup_begins(from_word, original_word_len) {
                for (word, score) in stream {
                    if found_words.contains_key(&word) {
                        continue;
                    }

                    found_words.insert(word, score);

                    // Requested limit reached? Stop there.
                    if found_words.len() >= limit {
                        break;
                    }
                }
            }
        }

        // Try to fuzzy-suggest other words? (e.g. correct typos)
        if self.action_config.fuzzy_matching_enabled && found_words.len() < limit {
            // Allow more typos in word as the word gets longer, up to a maximum limit
            let max_typo_factor = max_typo_factor.unwrap_or(typo_factor(original_word_len));
            let mut typo_factor = 1u32;

            // TODO: Rework the Levenshtein query feature to avoid repeating
            //   the same query over and over again. Maybe try to see if
            //   `fst_levenshtein` can return distances in its response.
            while found_words.len() < limit && typo_factor <= max_typo_factor {
                let Some(stream) = self.lookup_typos(from_word, typo_factor) else {
                    break;
                };

                for (word, score) in stream {
                    if found_words.contains_key(&word) {
                        continue;
                    }

                    found_words.insert(word, score);

                    // Requested limit reached? Stop there.
                    if found_words.len() >= limit {
                        break;
                    }
                }

                typo_factor += 1;
            }
        }

        if !found_words.is_empty() {
            Some(found_words.into_iter())
        } else {
            None
        }
    }

    pub fn lookup_begins(
        &self,
        word: &str,
        // Length before stemming. Useful to calculate correct score.
        original_word_len: usize,
    ) -> Option<impl Iterator<Item = (String, u16)>> {
        // Word over limit? (abort, the FST does not perform well over large words)
        if Self::word_over_limit(word) {
            return None;
        }

        if !self.action_config.prefix_matching_enabled {
            return None;
        }

        let Ok(stream) = self.lookup_begins_(word) else {
            return None;
        };

        tracing::debug!(?word, "looking up for word in 'begins' fst stream");

        Some(FSTStreamIterator(stream).map(move |word| {
            // WARN: Calculating distance to original word length might
            //   yield weird results when combines with stemming.
            let distance: usize = original_word_len.abs_diff(word.len());
            let score = u16::try_from(distance).unwrap_or(u16::MAX);
            (word, score)
        }))
    }

    pub fn lookup_typos(
        &self,
        word: &str,
        typo_factor: u32,
    ) -> Option<impl Iterator<Item = (String, u16)>> {
        if !self.action_config.fuzzy_matching_enabled {
            return None;
        }

        let Ok(stream) = self.lookup_typos_(word, typo_factor) else {
            return None;
        };

        tracing::debug!(
            ?word,
            typo_factor,
            "looking up for word in 'typos' fst stream"
        );

        // NOTE: Returning the same score for every word works only
        //   because we re-run `lookup_typos` for increasingly
        //   larger typo factors and do not re-insert existing
        //   values. As explained in previous TODO, we should try
        //   to get the real distance back from `fst_levenshtein`.
        let score = u16::try_from(typo_factor).unwrap_or(u16::MAX);

        Some(FSTStreamIterator(stream).map(move |word| (word, score)))
    }

    pub fn list_words(&self, limit: usize, offset: usize) -> Result<Vec<String>, ()> {
        let stream = self.as_stream();

        // Enumerate words from FST stream.
        match stream
            .into_strs()
            .map(|words| words.into_iter().skip(offset).take(limit).collect())
        {
            Err(err) => {
                tracing::debug!("conversion of stream failed: {err:?}");
                Err(())
            }
            Ok(words) => Ok(words),
        }
    }

    pub fn count_words(&self) -> usize {
        self.cardinality()
    }

    fn word_over_limit(word: &str) -> bool {
        if word.len() > WORD_LIMIT_LENGTH {
            tracing::debug!("got over-limit fst word: {word:?}");

            true
        } else {
            false
        }
    }
}

/// Allow more typos in word as the word gets longer, up to a maximum limit.
pub(crate) fn typo_factor(word_len: usize) -> u32 {
    match word_len {
        1..=3 => 0,
        4..=6 => 1,
        7..=9 => 2,
        _ => 3,
    }
}

impl StoreFSTMisc {
    pub fn count_collection_buckets(
        collection: impl AsRef<str>,
        fst_store_config: &crate::config::ConfigStoreFST,
    ) -> Result<usize, ()> {
        let path_mode = StoreFSTPathMode::Permanent;

        let collection_atom = StoreKeyerHasher::to_compact(collection.as_ref());
        let collection_path = fst_store_config.path(path_mode, collection_atom, None);

        if !collection_path.exists() {
            return Ok(0);
        }

        let entries = fs::read_dir(&collection_path).map_err(|error| {
            tracing::error!(
                ?collection_path,
                "Failed reading collection directory for count: {error:?}"
            )
        })?;

        let mut count = 0;

        let fst_extension = path_mode.extension();
        let fst_extension_len = fst_extension.len();

        // Scan collection directory for contained buckets (count them).
        for entry in entries.flatten() {
            if let Some(entry_name) = entry.file_name().to_str() {
                let entry_name_len = entry_name.len();

                // FST file found? This is a bucket.
                if entry_name_len > fst_extension_len && entry_name.ends_with(fst_extension) {
                    count += 1;
                }
            }
        }

        Ok(count)
    }

    fn check_over_limits(
        bytes_count: usize,
        words_count: usize,
        fst_graph_config: &crate::config::ConfigStoreFSTGraph,
    ) -> bool {
        // Over bytes limit?
        let max_size = fst_graph_config.max_size * 1024;
        if bytes_count >= max_size {
            tracing::info!(
                "fst has exceeded maximum allowed bytes: {bytes_count} over limit: {max_size}"
            );

            return true;
        }

        // Over words limit?
        let max_words = fst_graph_config.max_words;
        if words_count >= max_words {
            tracing::info!(
                "fst has exceeded maximum allowed words: {words_count} over limit: {max_words}"
            );

            return true;
        }

        // Not over limit
        false
    }
}

impl StoreFSTKey {
    pub fn from_atom(collection_hash: StoreFSTAtom, bucket_hash: StoreFSTAtom) -> StoreFSTKey {
        StoreFSTKey {
            collection_hash,
            bucket_hash,
        }
    }

    pub fn from_str(collection_str: &str, bucket_str: &str) -> StoreFSTKey {
        StoreFSTKey {
            collection_hash: StoreKeyerHasher::to_compact(collection_str),
            bucket_hash: StoreKeyerHasher::to_compact(bucket_str),
        }
    }

    pub fn as_collection_hash(&self) -> &StoreFSTAtom {
        &self.collection_hash
    }
}

// MARK: - Helpers

#[repr(transparent)]
struct FSTStreamIterator<'a, A: Automaton>(fst::set::Stream<'a, A>);

impl<'a, A: Automaton> Iterator for FSTStreamIterator<'a, A> {
    type Item = String;

    fn next(&mut self) -> Option<Self::Item> {
        match self.0.next() {
            Some(bytes) => match str::from_utf8(bytes) {
                Ok(str) => Some(str.to_owned()),
                Err(_) => None,
            },
            None => None,
        }
    }
}

// MARK: - Tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_acquires_graph() {
        let fst_pool = test_fst_pool();

        assert!(fst_pool.acquire("c:test:1", "b:test:1").is_ok());
    }

    #[test]
    fn it_janitors_graph() {
        let fst_pool = test_fst_pool();

        fst_pool.janitor(|_| true);
    }

    #[test]
    fn it_proceeds_primitives() {
        let fst_pool = test_fst_pool();

        let store = fst_pool.acquire("c:test:2", "b:test:2").unwrap();

        assert!(store.lookup_typos_("valerien", 1).is_ok());
    }

    fn test_fst_pool() -> StoreFSTPool {
        let fst_store_config = test_fst_store_config();

        StoreFSTPool::new(fst_store_config, Default::default())
    }

    fn test_fst_store_config() -> Arc<crate::config::ConfigStoreFST> {
        Arc::new(
            config::Config::builder()
                .add_source(config::File::from_str(
                    crate::config::tests::defaults_toml(),
                    config::FileFormat::Toml,
                ))
                .build()
                .unwrap()
                .get::<crate::config::ConfigStoreFST>("store.fst")
                .unwrap(),
        )
    }
}

// MARK: - Boilerplate

impl fmt::Debug for StoreFSTPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use crate::util::fmt::{AsPrettyMutex, AsPrettyRwLock};

        // NOTE: Deconstructing to future-proof this function.
        let Self {
            fst_action_config,
            graph_pool,
            graph_acquire_lock,
            graph_rebuild_lock,
            graph_access_lock,
            graph_consolidate,
            // NOTE: We don’t care about the configuration,
            //   we can see it elsewhere if needed.
            fst_store_config: _fst_store_config,
        } = self;

        f.debug_struct("StoreFSTPool")
            .field("fst_action_config", fst_action_config)
            .field("graph_pool", &AsPrettyRwLock(graph_pool))
            .field("graph_acquire_lock", &AsPrettyMutex(graph_acquire_lock))
            .field("graph_rebuild_lock", &AsPrettyMutex(graph_rebuild_lock))
            .field("graph_access_lock", &AsPrettyRwLock(graph_access_lock))
            .field("graph_consolidate", &AsPrettyRwLock(graph_consolidate))
            .finish_non_exhaustive()
    }
}

impl fmt::Display for StoreFSTKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<{:x}>/<{:x}>", self.collection_hash, self.bucket_hash)
    }
}

impl fmt::Debug for StoreFSTKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self, f)
    }
}

impl fmt::Debug for StoreFST {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use crate::util::fmt::AsPrettyRwLock;

        // NOTE: Deconstructing to future-proof this function.
        let Self {
            graph,
            target,
            pending,
            last_used,
            last_consolidated,
            graph_consolidate,
            action_config,
        } = self;

        f.debug_struct("StoreFST")
            .field("graph", graph)
            .field("target", target)
            .field("pending", pending)
            .field("last_used", &AsPrettyRwLock(last_used))
            .field("last_consolidated", &AsPrettyRwLock(last_consolidated))
            .field("graph_consolidate", &AsPrettyRwLock(graph_consolidate))
            .field("action_config", action_config)
            .finish()
    }
}

impl fmt::Debug for StoreFSTPending {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use crate::util::fmt::AsPrettyRwLock;

        // NOTE: Deconstructing to future-proof this function.
        let Self { pop, push } = self;

        f.debug_struct("StoreFSTPending")
            .field("pop", &AsPrettyRwLock(pop))
            .field("push", &AsPrettyRwLock(push))
            .finish()
    }
}
