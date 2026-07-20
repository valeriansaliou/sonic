// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use fst::{
    Automaton, Error as FSTError, IntoStreamer, Map as FSTMap, MapBuilder as FSTMapBuilder,
    Streamer,
};
use fst_levenshtein::Levenshtein;
use hashbrown::{HashMap, HashSet};
use radix::RadixNum;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::str;
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::generic::{
    StoreGeneric, StoreGenericActionBuilder, StoreGenericBuilder, StoreGenericPool,
};
use super::identifiers::StoreBucketID;
use super::keyer::StoreKeyerHasher;
use crate::query::QueryMatchScore;

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
    graph: RwLock<Arc<FSTMap>>,
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
    push: Arc<RwLock<HashMap<Vec<u8>, u64>>>,
}

pub struct StoreFSTActionBuilder<'build> {
    pub fst_store_config: &'build crate::config::ConfigStoreFST,
}

pub struct StoreFSTAction {
    store: StoreFSTBox,
}

impl StoreFSTAction {
    fn config(&self) -> &StoreFSTActionConfig {
        &self.store.action_config
    }
}

#[derive(PartialEq, Eq, Hash, Clone, Copy)]
pub struct StoreFSTKey {
    collection_hash: StoreFSTAtom,
    bucket_id: StoreFSTAtom,
}

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
    pub fuzzy_matching_enabled: bool,
}

impl Default for StoreFSTActionConfig {
    fn default() -> Self {
        Self {
            fuzzy_matching_enabled: true,
        }
    }
}

const WORD_LIMIT_LENGTH: usize = 40;
const ATOM_HASH_RADIX: usize = 16;
const CONSOLIDATE_BATCH_SIZE: usize = 8;
const CONSOLIDATE_WARN_MILLIS: u128 = 100;

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

    pub fn acquire(
        &self,
        collection: impl AsRef<str>,
        bucket_id: StoreBucketID,
    ) -> Result<StoreFSTBox, ()> {
        let collection_str = collection.as_ref();
        let pool_key =
            StoreFSTKey::from_atom(StoreKeyerHasher::to_compact(collection_str), bucket_id);

        // Freeze acquire lock, and reference it in context
        // Notice: this prevents two graphs on the same collection to be opened at the same time.
        let _acquire = self.graph_acquire_lock.lock().unwrap();

        // Acquire a thread-safe store pool reference in read mode
        let graph_pool_read = self.graph_pool.read().unwrap();

        if let Some(store_fst) = graph_pool_read.get(&pool_key) {
            Self::proceed_acquire_cache("fst", collection_str, pool_key, store_fst)
        } else {
            tracing::info!(
                "fst store not in pool for collection: {} <{:x}> / bucket ID: {}, opening it",
                collection_str,
                pool_key.collection_hash,
                pool_key.bucket_id
            );

            // Important: we need to drop the read reference first, to avoid dead-locking \
            //   when acquiring the RWLock in write mode in this block.
            drop(graph_pool_read);

            let builder = StoreFSTBuilder {
                fst_store_config: &self.fst_store_config,
                graph_consolidate: Arc::clone(&self.graph_consolidate),
                fst_action_config: self.fst_action_config,
            };

            Self::proceed_acquire_open("fst", collection_str, pool_key, &self.graph_pool, &builder)
        }
    }

    pub fn janitor(&self) {
        Self::proceed_janitor(
            "fst",
            &self.graph_pool,
            self.fst_store_config.pool.inactive_after,
            &self.graph_access_lock,
        )
    }

    pub fn backup(&self, path: &Path) -> Result<(), io::Error> {
        tracing::debug!("backing up all fst stores to path: {:?}", path);

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
        tracing::debug!("restoring all fst stores from path: {:?}", path);

        // Proceed dump action (restore)
        self.dump_action(
            "restore",
            StoreFSTPathMode::Backup,
            path,
            &self.fst_store_config.path,
            &Self::restore_item,
        )
    }

    pub fn consolidate(&self, force: bool) {
        tracing::debug!("scanning for fst store pool items to consolidate");
        let _rebuild = self.graph_rebuild_lock.lock().unwrap();
        if self.graph_consolidate.read().unwrap().is_empty() {
            tracing::info!("no fst store pool items to consolidate in register");
            return;
        }
        let mut keys_consolidate: Vec<StoreFSTKey> = Vec::new();
        {
            let (graph_pool_read, graph_consolidate_read) = (
                self.graph_pool.read().unwrap(),
                self.graph_consolidate.read().unwrap(),
            );
            for key in &*graph_consolidate_read {
                if let Some(store) = graph_pool_read.get(key) {
                    let not_consolidated_for = store
                        .last_consolidated
                        .read()
                        .unwrap()
                        .elapsed()
                        .unwrap_or_else(|err| {
                            tracing::error!(
                                "fst key: {} last consolidated duration clock issue, zeroing: {}",
                                key,
                                err
                            );

                            // Assuming a zero seconds fallback duration
                            Duration::from_secs(0)
                        })
                        .as_secs();

                    if force
                        || not_consolidated_for >= self.fst_store_config.graph.consolidate_after
                    {
                        tracing::info!(
                            "fst key: {} not consolidated for: {} seconds, may consolidate",
                            key,
                            not_consolidated_for
                        );
                        keys_consolidate.push(*key);
                    } else {
                        tracing::debug!(
                            "fst key: {} not consolidated for: {} seconds, no consolidate",
                            key,
                            not_consolidated_for
                        );
                    }
                }
            }
        }
        if keys_consolidate.is_empty() {
            tracing::info!("no fst store pool items need to consolidate at the moment");
            return;
        }
        keys_consolidate.sort_unstable_by_key(|key| (key.collection_hash, key.bucket_id));
        if !force {
            keys_consolidate.truncate(CONSOLIDATE_BATCH_SIZE);
        }
        {
            let mut graph_consolidate_write = self.graph_consolidate.write().unwrap();
            for key in &keys_consolidate {
                graph_consolidate_write.remove(key);
                tracing::debug!("fst key: {} cleared from consolidate register", key);
            }
        }
        let (mut count_moved, mut count_pushed, mut count_popped) = (0, 0, 0);
        for key in &keys_consolidate {
            let _access = self.graph_access_lock.read().unwrap();
            let store = self.graph_pool.read().unwrap().get(key).cloned();
            let Some(store) = store else {
                continue;
            };
            let started = Instant::now();
            let consolidate_counts = self.consolidate_item(&store);
            count_moved += consolidate_counts.1;
            count_pushed += consolidate_counts.2;
            count_popped += consolidate_counts.3;
            let elapsed_millis = started.elapsed().as_millis();
            if elapsed_millis >= CONSOLIDATE_WARN_MILLIS {
                tracing::warn!(
                    fst_key = %key,
                    elapsed_ms = elapsed_millis,
                    success = consolidate_counts.0,
                    "slow fst consolidation finished"
                );
            } else {
                tracing::info!(
                    fst_key = %key,
                    elapsed_ms = elapsed_millis,
                    success = consolidate_counts.0,
                    "fst consolidation finished"
                );
            }
            thread::yield_now();
        }
        tracing::info!(
            remaining = self.graph_consolidate.read().unwrap().len(),
            processed = keys_consolidate.len(),
            "done scanning for fst store pool items to consolidate (move: {}, push: {}, pop: {})",
            count_moved,
            count_pushed,
            count_popped
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
        let fst_extension_len = fst_extension.len();

        // Iterate on FST collections
        for collection in fs::read_dir(read_path)? {
            let collection = collection?;

            // Actual collection found?
            if let (Ok(collection_file_type), Some(collection_name)) =
                (collection.file_type(), collection.file_name().to_str())
            {
                if collection_file_type.is_dir() {
                    tracing::debug!("fst collection ongoing {}: {}", action, collection_name);

                    // Create write folder for collection
                    fs::create_dir_all(write_path.join(collection_name))?;

                    // Iterate on FST collection buckets
                    for bucket in fs::read_dir(read_path.join(collection_name))? {
                        let bucket = bucket?;

                        // Actual bucket found?
                        if let (Ok(bucket_file_type), Some(bucket_file_name)) =
                            (bucket.file_type(), bucket.file_name().to_str())
                        {
                            let bucket_file_name_len = bucket_file_name.len();

                            if bucket_file_type.is_file()
                                && bucket_file_name_len > fst_extension_len
                                && bucket_file_name.ends_with(fst_extension)
                            {
                                // Acquire bucket name (from full file name)
                                let bucket_name =
                                    &bucket_file_name[..(bucket_file_name_len - fst_extension_len)];

                                tracing::debug!(
                                    "fst bucket ongoing {}: {}/{}",
                                    action,
                                    collection_name,
                                    bucket_name
                                );

                                fn_item(
                                    self,
                                    write_path,
                                    &bucket.path(),
                                    collection_name,
                                    bucket_name,
                                )?;
                            }
                        }
                    }
                }
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
        // Notice: this prevents store to be acquired from any context
        let _access = self.graph_access_lock.write().unwrap();

        // Generate path to FST backup
        let fst_backup_path = backup_path.join(collection_name).join(format!(
            "{}{}",
            bucket_name,
            StoreFSTPathMode::Backup.extension()
        ));

        tracing::debug!(
            "fst bucket: {}/{} backing up to path: {:?}",
            collection_name,
            bucket_name,
            fst_backup_path
        );

        // Erase any previously-existing FST backup
        fs::remove_file(&fst_backup_path).ok();

        // Stream actual FST data to FST backup
        let backup_fst_file = File::create(&fst_backup_path)?;
        let mut backup_fst_writer = BufWriter::new(backup_fst_file);

        let mut count_words = 0;

        // Convert names to hashes (as names are hashes encoded as base-16 strings, but we need \
        //   them as proper integers)
        if let (Ok(collection_radix), Ok(bucket_radix)) = (
            RadixNum::from_str(collection_name, ATOM_HASH_RADIX),
            RadixNum::from_str(bucket_name, ATOM_HASH_RADIX),
        ) {
            if let (Ok(collection_hash), Ok(bucket_id)) =
                (collection_radix.as_decimal(), bucket_radix.as_decimal())
            {
                let origin_fst = StoreFSTBuilder::open(
                    collection_hash as StoreFSTAtom,
                    bucket_id as StoreFSTAtom,
                    &self.fst_store_config,
                )
                .map_err(|_| io::Error::other("graph open failure"))?;

                let mut origin_fst_stream = origin_fst.stream();

                while let Some((word, frequency)) = origin_fst_stream.next() {
                    count_words += 1;

                    writeln!(
                        backup_fst_writer,
                        "{frequency}\t{}",
                        String::from_utf8_lossy(word)
                    )?;
                }

                tracing::info!(
                    "fst bucket: {}/{} backed up to path: {:?} ({} words)",
                    collection_name,
                    bucket_name,
                    fst_backup_path,
                    count_words
                );
            }
        }

        Ok(())
    }

    fn restore_item(
        &self,
        _backup_path: &Path,
        origin_path: &Path,
        collection_name: &str,
        bucket_name: &str,
    ) -> Result<(), io::Error> {
        // Acquire access lock (in blocking write mode), and reference it in context
        // Notice: this prevents store to be acquired from any context
        let _access = self.graph_access_lock.write().unwrap();

        tracing::debug!(
            "fst bucket: {}/{} restoring from path: {:?}",
            collection_name,
            bucket_name,
            origin_path
        );

        // Convert names to hashes (as names are hashes encoded as base-16 strings, but we need \
        //   them as proper integers)
        if let (Ok(collection_radix), Ok(bucket_radix)) = (
            RadixNum::from_str(collection_name, ATOM_HASH_RADIX),
            RadixNum::from_str(bucket_name, ATOM_HASH_RADIX),
        ) {
            if let (Ok(collection_hash), Ok(bucket_id)) =
                (collection_radix.as_decimal(), bucket_radix.as_decimal())
            {
                // Force a FST store close
                self.close(collection_hash as StoreFSTAtom, bucket_id as StoreFSTAtom);

                // Generate path to FST
                let fst_path = self.fst_store_config.path(
                    StoreFSTPathMode::Permanent,
                    collection_hash as StoreFSTAtom,
                    Some(bucket_id as StoreFSTAtom),
                );

                // Remove existing FST data?
                if fst_path.exists() {
                    fs::remove_file(&fst_path)?;
                }

                // Stream backup words to restored FST
                let fst_writer = BufWriter::new(File::create(&fst_path)?);
                let fst_backup_reader = BufReader::new(File::open(&origin_path)?);

                let mut fst_builder = FSTMapBuilder::new(fst_writer)
                    .map_err(|_| io::Error::other("graph restore builder failure"))?;

                for entry in fst_backup_reader.lines() {
                    let entry = entry?;
                    let (frequency, word) = entry
                        .split_once('\t')
                        .ok_or_else(|| io::Error::other("invalid graph backup entry"))?;
                    let frequency = frequency
                        .parse::<u64>()
                        .map_err(|_| io::Error::other("invalid graph backup frequency"))?;

                    fst_builder
                        .insert(word, frequency)
                        .map_err(|_| io::Error::other("graph restore word insert failure"))?;
                }

                fst_builder
                    .finish()
                    .map_err(|_| io::Error::other("graph restore finish failure"))?;

                tracing::info!(
                    "fst bucket: {}/{} restored to path: {:?} from backup: {:?}",
                    collection_name,
                    bucket_name,
                    fst_path,
                    origin_path
                );
            }
        }

        Ok(())
    }

    fn consolidate_item(&self, store: &StoreFSTBox) -> (bool, usize, usize, usize) {
        let (pending_push, pending_pop) = {
            let push = store.pending.push.read().unwrap();
            let pop = store.pending.pop.read().unwrap();
            (push.clone(), pop.clone())
        };
        if pending_push.is_empty() && pending_pop.is_empty() {
            return (false, 0, 0, 0);
        }
        let old_fst = Arc::clone(&store.graph.read().unwrap());
        let mut candidates = HashMap::<Vec<u8>, u64>::with_capacity(
            old_fst.len().saturating_add(pending_push.len()),
        );
        let mut old_stream = old_fst.stream();
        while let Some((word, frequency)) = old_stream.next() {
            candidates.insert(word.to_vec(), frequency);
        }
        let old_count = candidates.len();

        let mut count_popped = 0;
        for word in pending_pop.iter() {
            if candidates.remove(word).is_some() {
                count_popped += 1;
            }
        }

        let mut count_pushed = 0;
        for (word, frequency) in pending_push.iter() {
            match candidates.entry(word.clone()) {
                hashbrown::hash_map::Entry::Occupied(mut entry) => {
                    *entry.get_mut() = *frequency;
                }
                hashbrown::hash_map::Entry::Vacant(entry) => {
                    entry.insert(*frequency);
                    count_pushed += 1;
                }
            }
        }

        let mut hottest = candidates.into_iter().collect::<Vec<_>>();
        hottest.sort_unstable_by(
            |(left_word, left_frequency), (right_word, right_frequency)| {
                right_frequency
                    .cmp(left_frequency)
                    .then_with(|| left_word.cmp(right_word))
            },
        );
        hottest.truncate(self.fst_store_config.graph.max_words);

        // Keep the hottest terms within a conservative serialized-size budget.
        let max_bytes = self.fst_store_config.graph.max_size.saturating_mul(1024);
        let mut estimated_bytes = 0usize;
        hottest.retain(|(word, _)| {
            let item_size = word.len().saturating_add(size_of::<u64>());
            if estimated_bytes.saturating_add(item_size) > max_bytes {
                false
            } else {
                estimated_bytes += item_size;
                true
            }
        });
        hottest.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));

        let bucket_tmp_path = self.fst_store_config.path(
            StoreFSTPathMode::Temporary,
            store.target.collection_hash,
            Some(store.target.bucket_id),
        );
        let Some(bucket_tmp_parent) = bucket_tmp_path.parent() else {
            store.should_consolidate();
            return (false, 0, 0, 0);
        };

        if fs::create_dir_all(bucket_tmp_parent).is_err() {
            store.should_consolidate();
            return (false, 0, 0, 0);
        }
        fs::remove_file(&bucket_tmp_path).ok();
        let mut builder = FSTMapBuilder::memory();

        for (word, frequency) in &hottest {
            if let Err(err) = builder.insert(word, *frequency) {
                tracing::error!(?err, "failed inserting adaptive typo term into fst");
                store.should_consolidate();
                return (false, 0, 0, 0);
            }
        }
        let Ok(encoded) = builder.into_inner() else {
            store.should_consolidate();
            return (false, 0, 0, 0);
        };
        let Ok(new_fst) = FSTMap::from_bytes(encoded.clone()) else {
            store.should_consolidate();
            return (false, 0, 0, 0);
        };
        if fs::write(&bucket_tmp_path, encoded).is_err() {
            store.should_consolidate();
            return (false, 0, 0, 0);
        }

        let bucket_final_path = self.fst_store_config.path(
            StoreFSTPathMode::Permanent,
            store.target.collection_hash,
            Some(store.target.bucket_id),
        );
        if fs::rename(&bucket_tmp_path, &bucket_final_path).is_err() {
            store.should_consolidate();
            return (false, 0, 0, 0);
        }
        *store.graph.write().unwrap() = Arc::new(new_fst);
        let pending_remaining = {
            let mut current_push = store.pending.push.write().unwrap();
            let mut current_pop = store.pending.pop.write().unwrap();
            current_push.retain(|word, frequency| pending_push.get(word) != Some(frequency));
            current_pop.retain(|word| !pending_pop.contains(word));
            !current_push.is_empty() || !current_pop.is_empty()
        };
        if pending_remaining {
            store.should_consolidate();
        }
        tracing::info!(
            terms = hottest.len(),
            path = ?bucket_final_path,
            "consolidated adaptive typo lexicon"
        );

        let count_moved = hottest.len().saturating_sub(count_pushed);
        (true, count_moved.min(old_count), count_pushed, count_popped)
    }

    fn close(&self, collection_hash: StoreFSTAtom, bucket_id: StoreFSTAtom) {
        tracing::debug!(
            "closing finite-state transducer graph for collection: <{:x}> and bucket: <{:x}>",
            collection_hash,
            bucket_id
        );

        let bucket_target = StoreFSTKey::from_atom(collection_hash, bucket_id);

        self.graph_pool.write().unwrap().remove(&bucket_target);
        self.graph_consolidate
            .write()
            .unwrap()
            .remove(&bucket_target);
    }
}

impl<'build> StoreGenericPool<StoreFSTKey, StoreFST, StoreFSTBuilder<'build>> for StoreFSTPool {}

impl<'build> StoreFSTBuilder<'build> {
    fn open(
        collection_hash: StoreFSTAtom,
        bucket_id: StoreFSTAtom,
        fst_store_config: &crate::config::ConfigStoreFST,
    ) -> Result<FSTMap, FSTError> {
        tracing::debug!(
            "opening finite-state transducer graph for collection: <{:x}> and bucket: <{:x}>",
            collection_hash,
            bucket_id
        );

        let collection_bucket_path = fst_store_config.path(
            StoreFSTPathMode::Permanent,
            collection_hash,
            Some(bucket_id),
        );

        if collection_bucket_path.exists() {
            // Open graph at path for collection
            // Notice: this is unsafe, as loaded memory is a memory-mapped file, that cannot be \
            //   guaranteed not to be muted while we own a read handle to it. Though, we use \
            //   higher-level locking mechanisms on all callers of this method, so we are safe.
            unsafe { FSTMap::from_path(collection_bucket_path) }
        } else {
            // FST does not exist on disk, generate an empty FST for now; until a consolidation \
            //   task occurs and populates the on-disk-FST.
            let empty_iter: Vec<(&str, u64)> = Vec::new();

            FSTMap::from_iter(empty_iter)
        }
    }
}

impl crate::config::ConfigStoreFST {
    fn path(
        &self,
        mode: StoreFSTPathMode,
        collection_hash: StoreFSTAtom,
        bucket_id: Option<StoreFSTAtom>,
    ) -> PathBuf {
        let mut final_path = self.path.join(format!("{:x}", collection_hash));

        if let Some(bucket_id) = bucket_id {
            final_path = final_path.join(format!("{:x}{}", bucket_id, mode.extension()));
        }

        final_path
    }
}

impl<'build> StoreGenericBuilder<StoreFSTKey, StoreFST> for StoreFSTBuilder<'build> {
    fn build(&self, pool_key: StoreFSTKey) -> Result<StoreFST, ()> {
        Self::open(
            pool_key.collection_hash,
            pool_key.bucket_id,
            self.fst_store_config,
        )
        .map(|graph| {
            let now = SystemTime::now();

            StoreFST {
                graph: RwLock::new(Arc::new(graph)),
                target: pool_key,
                pending: StoreFSTPending::default(),
                last_used: Arc::new(RwLock::new(now)),
                last_consolidated: Arc::new(RwLock::new(now)),
                graph_consolidate: Arc::clone(&self.graph_consolidate),
                action_config: self.fst_action_config,
            }
        })
        .map_err(|err| {
            tracing::error!("failed opening fst: {}", err);
        })
    }
}

impl StoreFST {
    pub fn cardinality(&self) -> usize {
        self.graph.read().unwrap().len()
    }

    pub fn list_words(&self, limit: usize, offset: usize) -> Vec<String> {
        let graph = self.graph.read().unwrap();
        FSTStreamIterator(graph.into_stream())
            .skip(offset)
            .take(limit)
            .collect()
    }

    pub fn lookup_begins(&self, word: &str) -> Result<Vec<(String, u64)>, ()> {
        let upper_bound = Self::prefix_upper_bound(word.as_bytes()).ok_or(())?;
        let graph = self.graph.read().unwrap();
        let mut stream = graph.range().ge(word).lt(upper_bound).into_stream();
        let mut candidates = Vec::new();
        while let Some((word, frequency)) = stream.next() {
            if let Ok(word) = str::from_utf8(word) {
                candidates.push((word.to_owned(), frequency));
            }
        }
        Ok(candidates)
    }

    pub fn lookup_typos(&self, word: &str, typo_factor: u32) -> Result<Vec<(String, u64)>, ()> {
        tracing::debug!(
            "looking-up word in fst via 'typos': {} with typo factor: {}",
            word,
            typo_factor
        );

        if let Ok(fuzzy) = Levenshtein::new(word, typo_factor) {
            let graph = self.graph.read().unwrap();
            let mut stream = graph.search(fuzzy).into_stream();
            let mut candidates = Vec::new();
            while let Some((word, frequency)) = stream.next() {
                if let Ok(word) = str::from_utf8(word) {
                    candidates.push((word.to_owned(), frequency));
                }
            }
            Ok(candidates)
        } else {
            Err(())
        }
    }

    pub fn should_consolidate(&self) {
        // Check if not already scheduled
        if !self
            .graph_consolidate
            .read()
            .unwrap()
            .contains(&self.target)
        {
            // Schedule target for next consolidation tick (ie. collection + bucket tuple)
            self.graph_consolidate.write().unwrap().insert(self.target);

            // Bump 'last consolidated' time, effectively de-bouncing consolidation to a fixed \
            //   and predictable tick time in the future.
            let mut last_consolidated_value = self.last_consolidated.write().unwrap();

            *last_consolidated_value = SystemTime::now();

            // Perform an early drop of the lock (frees up write lock early)
            drop(last_consolidated_value);

            tracing::info!("graph consolidation scheduled on pool key: {}", self.target);
        } else {
            tracing::debug!(
                "graph consolidation already scheduled on pool key: {}",
                self.target
            );
        }
    }

    fn prefix_upper_bound(prefix: &[u8]) -> Option<Vec<u8>> {
        let mut upper_bound = prefix.to_vec();

        for index in (0..upper_bound.len()).rev() {
            if upper_bound[index] != u8::MAX {
                upper_bound[index] += 1;
                upper_bound.truncate(index + 1);
                return Some(upper_bound);
            }
        }

        None
    }
}

impl StoreGeneric for StoreFST {
    fn ref_last_used(&self) -> &RwLock<SystemTime> {
        &self.last_used
    }
}

impl<'build> StoreFSTActionBuilder<'build> {
    pub fn access(store: StoreFSTBox) -> StoreFSTAction {
        Self::build(store)
    }

    fn build(store: StoreFSTBox) -> StoreFSTAction {
        StoreFSTAction { store }
    }
}

impl StoreFSTPool {
    pub fn erase<T: AsRef<str>>(&self, collection: T, bucket: Option<T>) -> Result<u32, ()> {
        self.dispatch_erase("fst", collection, bucket)
    }

    pub fn erase_bucket_id(
        &self,
        collection: impl AsRef<str>,
        bucket_id: StoreBucketID,
    ) -> Result<u32, ()> {
        let collection_hash = StoreKeyerHasher::to_compact(collection.as_ref());
        let bucket_path = self.fst_store_config.path(
            StoreFSTPathMode::Permanent,
            collection_hash,
            Some(bucket_id),
        );
        self.close(collection_hash, bucket_id);

        if bucket_path.exists() {
            fs::remove_file(bucket_path).map(|_| 1).or(Err(()))
        } else {
            Ok(0)
        }
    }
}

impl StoreGenericActionBuilder for StoreFSTPool {
    fn proceed_erase_collection(&self, collection_str: &str) -> Result<u32, ()> {
        let path_mode = StoreFSTPathMode::Permanent;

        let collection_atom = StoreKeyerHasher::to_compact(collection_str);
        let collection_path = self.fst_store_config.path(path_mode, collection_atom, None);

        // Force a FST graph close (on all contained buckets)
        // Notice: we first need to scan for opened buckets in-memory, as not all FSTs may be \
        //   committed to disk; thus some FST stores that exist in-memory may not exist on-disk.
        let mut bucket_atoms: Vec<StoreFSTAtom> = Vec::new();

        {
            let graph_pool_read = self.graph_pool.read().unwrap();

            for target_key in graph_pool_read.keys() {
                if target_key.collection_hash == collection_atom {
                    bucket_atoms.push(target_key.bucket_id);
                }
            }
        }

        if !bucket_atoms.is_empty() {
            tracing::debug!(
                "will force-close {} fst buckets for collection: {}",
                bucket_atoms.len(),
                collection_str
            );

            let (mut graph_pool_write, mut graph_consolidate_write) = (
                self.graph_pool.write().unwrap(),
                self.graph_consolidate.write().unwrap(),
            );

            for bucket_atom in bucket_atoms {
                tracing::debug!(
                    "fst bucket graph force close for bucket: {}/<{:x}>",
                    collection_str,
                    bucket_atom
                );

                let bucket_target = StoreFSTKey::from_atom(collection_atom, bucket_atom);

                graph_pool_write.remove(&bucket_target);
                graph_consolidate_write.remove(&bucket_target);
            }
        }

        // Remove all FSTs on-disk
        if collection_path.exists() {
            tracing::debug!(
                "fst collection store exists, erasing: {}/* at path: {:?}",
                collection_str,
                &collection_path
            );

            // Remove FST graph storage from filesystem
            let erase_result = fs::remove_dir_all(&collection_path);

            if erase_result.is_ok() {
                tracing::debug!("done with fst collection erasure");

                Ok(1)
            } else {
                Err(())
            }
        } else {
            tracing::debug!(
                "fst collection store does not exist, consider already erased: {}/* at path: {:?}",
                collection_str,
                &collection_path
            );

            Ok(0)
        }
    }

    fn proceed_erase_bucket(&self, collection_str: &str, bucket_str: &str) -> Result<u32, ()> {
        tracing::debug!(
            "sub-erase on fst bucket: {} for collection: {}",
            bucket_str,
            collection_str
        );

        let (collection_atom, bucket_atom) = (
            StoreKeyerHasher::to_compact(collection_str),
            StoreKeyerHasher::to_compact(bucket_str),
        );

        let bucket_path = self.fst_store_config.path(
            StoreFSTPathMode::Permanent,
            collection_atom,
            Some(bucket_atom),
        );

        // Force a FST graph close
        self.close(collection_atom, bucket_atom);

        // Remove FST on-disk
        if bucket_path.exists() {
            tracing::debug!(
                "fst bucket graph exists, erasing: {}/{} at path: {:?}",
                collection_str,
                bucket_str,
                &bucket_path
            );

            // Remove FST graph storage from filesystem
            let erase_result = fs::remove_file(&bucket_path);

            if erase_result.is_ok() {
                tracing::debug!("done with fst bucket erasure");

                Ok(1)
            } else {
                Err(())
            }
        } else {
            tracing::debug!(
                "fst bucket graph does not exist, consider already erased: {}/{} at path: {:?}",
                collection_str,
                bucket_str,
                &bucket_path
            );

            Ok(0)
        }
    }
}

impl StoreFSTAction {
    pub fn push_word(
        &self,
        word: &str,
        frequency: u32,
        fst_store_config: &crate::config::ConfigStoreFST,
    ) -> bool {
        // Word over limit? (abort, the FST does not perform well over large words)
        if Self::word_over_limit(word) {
            return false;
        }
        if frequency < fst_store_config.graph.min_frequency {
            return self.pop_word(word);
        }

        let word_bytes = word.as_bytes();

        // Nuke word from 'pop' set? (void a previous un-consolidated commit)
        if self.store.pending.pop.read().unwrap().contains(word_bytes) {
            self.store.pending.pop.write().unwrap().remove(word_bytes);
        }

        let score = Self::hot_score(frequency);
        let stored_score = self.store.graph.read().unwrap().get(word).unwrap_or(0);
        let effective_score = self
            .store
            .pending
            .push
            .read()
            .unwrap()
            .get(word_bytes)
            .copied()
            .unwrap_or(stored_score);

        if score != effective_score {
            self.store
                .pending
                .push
                .write()
                .unwrap()
                .insert(word_bytes.to_vec(), score);
            self.store.should_consolidate();
            true
        } else {
            false
        }
    }

    pub fn pop_word(&self, word: &str) -> bool {
        // Word over limit? (abort, the FST does not perform well over large words)
        if Self::word_over_limit(word) {
            return false;
        }

        let word_bytes = word.as_bytes();

        // Nuke word from 'push' set? (void a previous un-consolidated commit)
        let removed_pending_push = if self
            .store
            .pending
            .push
            .read()
            .unwrap()
            .contains_key(word_bytes)
        {
            self.store
                .pending
                .push
                .write()
                .unwrap()
                .remove(word_bytes)
                .is_some()
        } else {
            false
        };

        // Keep an explicit pop when canceling a pending push, as it may be in an in-flight rebuild.
        if (removed_pending_push || self.store.graph.read().unwrap().contains_key(word_bytes))
            && !self.store.pending.pop.read().unwrap().contains(word_bytes)
        {
            self.store
                .pending
                .pop
                .write()
                .unwrap()
                .insert(word_bytes.to_vec());

            self.store.should_consolidate();

            // Popped
            true
        } else {
            // Not popped
            false
        }
    }

    pub fn lookup_begins(
        &self,
        from_word: &str,
        original_word_len: usize,
    ) -> Option<impl Iterator<Item = (String, QueryMatchScore)>> {
        if Self::word_over_limit(from_word) {
            return None;
        }

        let Ok(candidates) = self.store.lookup_begins(from_word) else {
            return None;
        };

        Some(candidates.into_iter().map(move |(word, _)| {
            let distance = original_word_len.abs_diff(word.len());
            let score = u16::try_from(distance).unwrap_or(u16::MAX);
            (word, score)
        }))
    }

    pub fn lookup_typos(
        &self,
        from_word: &str,
        typo_factor: u32,
    ) -> Option<impl Iterator<Item = (String, QueryMatchScore)>> {
        if !self.config().fuzzy_matching_enabled {
            return None;
        }

        let Ok(mut candidates) = self.store.lookup_typos(from_word, typo_factor) else {
            return None;
        };

        tracing::debug!(
            word = ?from_word, typo_factor,
            "looking up for word in 'typos' fst stream"
        );

        // NOTE: Returning the same score for every word works only
        //   because we re-run `lookup_typos` for increasingly
        //   larger typo factors and do not re-insert existing
        //   values. As explained in previous TODO, we should try
        //   to get the real distance back from `fst_levenshtein`.
        let score = u16::try_from(typo_factor).unwrap_or(u16::MAX);
        candidates.sort_unstable_by(
            |(left_word, left_frequency), (right_word, right_frequency)| {
                right_frequency
                    .cmp(left_frequency)
                    .then_with(|| left_word.cmp(right_word))
            },
        );

        Some(candidates.into_iter().map(move |(word, _)| (word, score)))
    }

    pub fn list_words(&self, limit: usize, offset: usize) -> Result<Vec<String>, ()> {
        Ok(self.store.list_words(limit, offset))
    }

    pub fn count_words(&self) -> usize {
        self.store.cardinality()
    }

    fn word_over_limit(word: &str) -> bool {
        if word.len() > WORD_LIMIT_LENGTH {
            tracing::debug!("got over-limit fst word: {}", word);

            true
        } else {
            false
        }
    }

    fn hot_score(frequency: u32) -> u64 {
        let recency = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs() as u32)
            .unwrap_or(0);
        (u64::from(frequency) << 32) | u64::from(recency)
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

impl StoreFSTKey {
    pub fn from_atom(collection_hash: StoreFSTAtom, bucket_id: StoreFSTAtom) -> StoreFSTKey {
        StoreFSTKey {
            collection_hash,
            bucket_id,
        }
    }
}

impl fmt::Display for StoreFSTKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<{:x}>/<{:x}>", self.collection_hash, self.bucket_id)
    }
}

// MARK: - Helpers

#[repr(transparent)]
struct FSTStreamIterator<'a, A: Automaton>(fst::map::Stream<'a, A>);

impl<'a, A: Automaton> Iterator for FSTStreamIterator<'a, A> {
    type Item = String;

    fn next(&mut self) -> Option<Self::Item> {
        match self.0.next() {
            Some((bytes, _frequency)) => match str::from_utf8(bytes) {
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_STORE_ID: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn it_acquires_graph() {
        let fst_pool = test_fst_pool();

        assert!(fst_pool.acquire("c:test:1", 1).is_ok());
    }

    #[test]
    fn it_janitors_graph() {
        let fst_pool = test_fst_pool();

        fst_pool.janitor();
    }

    #[test]
    fn it_proceeds_primitives() {
        let fst_pool = test_fst_pool();

        let store = fst_pool.acquire("c:test:2", 2).unwrap();

        assert!(store.lookup_typos("valerien", 1).is_ok());
    }

    #[test]
    fn it_retains_frequent_typo_terms() {
        let mut config = test_fst_store_config_value();
        config.graph.max_words = 2;
        config.graph.min_frequency = 2;
        let config = Arc::new(config);
        let fst_pool = StoreFSTPool::new(Arc::clone(&config), Default::default());
        let store = fst_pool.acquire("c:test:hot", 1).unwrap();
        let action = StoreFSTActionBuilder::access(store);

        assert!(!action.push_word("rare", 1, &config));
        assert!(action.push_word("transient", 2, &config));
        assert!(action.pop_word("transient"));
        assert!(action.push_word("mesenter", 2, &config));
        assert!(action.push_word("other", 5, &config));
        assert!(action.push_word("messenger", 10, &config));
        let access = fst_pool.lock_read_access();
        fst_pool.consolidate(true);
        drop(access);

        let store = fst_pool.acquire("c:test:hot", 1).unwrap();
        let action = StoreFSTActionBuilder::access(store);
        let words = action.list_words(10, 0).unwrap();
        assert_eq!(words, ["messenger", "other"]);
        assert!(
            action
                .lookup_typos("mesengr", 2)
                .unwrap()
                .any(|(word, _)| word == "messenger")
        );
    }

    fn test_fst_pool() -> StoreFSTPool {
        let fst_store_config = test_fst_store_config();

        StoreFSTPool::new(fst_store_config, Default::default())
    }

    fn test_fst_store_config() -> Arc<crate::config::ConfigStoreFST> {
        Arc::new(test_fst_store_config_value())
    }

    fn test_fst_store_config_value() -> crate::config::ConfigStoreFST {
        let mut config = config::Config::builder()
            .add_source(config::File::from_str(
                crate::config::tests::defaults_toml(),
                config::FileFormat::Toml,
            ))
            .build()
            .unwrap()
            .get::<crate::config::ConfigStoreFST>("store.fst")
            .unwrap();
        config.path = std::env::temp_dir().join(format!(
            "sonic-fst-unit-{}-{}",
            std::process::id(),
            TEST_STORE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        config
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
            .field("graph", &AsPrettyRwLock(graph))
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
