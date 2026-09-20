// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::collections::VecDeque;
use std::fs::File;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{Duration, SystemTime};
use std::{fmt, fs, io};

use fst::Streamer as _;
use hashbrown::{DefaultHashBuilder, HashMap, HashSet};

use crate::store::StoreItemPart;
use crate::store::generic::*;

use super::util::*;
use super::{StoreFST, StoreFSTActionConfig, StoreFSTAtom, StoreFSTPathMode};

// MARK: - Store pool

// NOTE: This type cannot be generic over a lifetime as spawning threads would
//   force it to be `'static`.
#[derive(Clone)]
pub struct StoreFSTPool {
    pub(super) fst_store_config: Arc<crate::config::StoreFSTConfig>,
    // NOTE: This shouldn’t be here, but until a big rewrite let’s not care.
    pub fst_action_config: StoreFSTActionConfig,
    graph_pool: Arc<RwLock<HashMap<StoreFSTId, Arc<StoreFST>>>>,
    graph_acquire_lock: Arc<Mutex<()>>,
    graph_rebuild_lock: Arc<Mutex<()>>,
    pub(super) graph_access_lock: Arc<RwLock<()>>,
    graph_consolidate: Arc<RwLock<HashSet<StoreFSTId>>>,
}

impl StoreFSTPool {
    pub fn new(
        fst_store_config: Arc<crate::config::StoreFSTConfig>,
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
}

impl StoreGenericPool for StoreFSTPool {
    type StoreId = StoreFSTId;
    type Store = StoreFST;
    type HashBuilder = DefaultHashBuilder;

    fn kind() -> &'static str {
        "fst"
    }

    fn consider_inactive_after_secs(&self) -> u64 {
        self.fst_store_config.pool.inactive_after
    }

    fn access_lock(&self) -> &RwLock<()> {
        &self.graph_access_lock
    }

    fn proceed_erase_collection(&self, collection_name: StoreItemPart) -> Result<u32, ()> {
        let collection_atom = collection_name.into_compact();
        let collection_path = self.fst_store_config.collection_path(collection_atom);

        // Force a FST graph close (on all contained buckets)
        // NOTE: we first need to scan for opened buckets in-memory, as not all FSTs may be
        //   committed to disk; thus some FST stores that exist in-memory may not exist on-disk.
        // TODO(perf): Instead of collection into a `Vec` just to check `is_empty` and
        //   lock only if necessary, use a `LazyCell` to do the same in a single step.
        let mut bucket_atoms: Vec<StoreFSTAtom> = Vec::new();

        {
            let graph_pool_read = self.graph_pool.read().unwrap();

            for store_id in graph_pool_read.keys() {
                if store_id.collection_hash == collection_atom {
                    bucket_atoms.push(store_id.bucket_hash);
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

                let bucket_target = StoreFSTId::from_atoms(collection_atom, bucket_atom);

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

    fn proceed_erase_bucket(
        &self,
        collection_name: StoreItemPart,
        bucket_name: StoreItemPart,
    ) -> Result<u32, ()> {
        tracing::debug!(
            "Sub-erase on fst bucket {bucket_name:?} for collection {collection_name:?}"
        );

        let store_id = StoreFSTId::from_parts(collection_name, bucket_name);

        let bucket_path = self
            .fst_store_config
            .store_path(store_id, StoreFSTPathMode::Permanent);

        // Force a FST graph close.
        self.close(store_id);

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

impl StoreFSTPool {
    pub fn acquire(
        &self,
        collection: StoreItemPart,
        bucket: StoreItemPart,
    ) -> Result<Arc<StoreFST>, ()> {
        let store_id = StoreFSTId::from_parts(collection, bucket);

        // Freeze acquire lock, and reference it in context
        // Notice: this prevents two graphs on the same collection to be opened at the same time.
        let _acquire = self.graph_acquire_lock.lock().unwrap();

        // Acquire a thread-safe store pool reference in read mode
        let graph_pool_read = self.graph_pool.read().unwrap();

        if let Some(store_fst) = graph_pool_read.get(&store_id) {
            Self::proceed_acquire_cache(store_id, store_fst)
        } else {
            tracing::debug!("fst store {store_id} not in pool, opening it");

            // Important: we need to drop the read reference first, to avoid dead-locking \
            //   when acquiring the RWLock in write mode in this block.
            drop(graph_pool_read);

            self.proceed_acquire_open(store_id, Self::build, None)
        }
    }

    fn build(&self, store_id: StoreFSTId) -> Result<StoreFST, ()> {
        let graph = (self.open(store_id))
            .map_err(|error| tracing::error!("Failed opening fst: {error:?}"))?;

        let now = SystemTime::now();

        Ok(StoreFST {
            graph,
            target: store_id,
            pending: Default::default(),
            last_used: Arc::new(RwLock::new(now)),
            last_consolidated: Arc::new(RwLock::new(now)),
            graph_consolidate: Arc::clone(&self.graph_consolidate),
            action_config: self.fst_action_config,
        })
    }

    pub(super) fn open(&self, id: StoreFSTId) -> Result<fst::Set, fst::Error> {
        tracing::debug!("Opening fst graph for {id}");

        let collection_bucket_path = self
            .fst_store_config
            .store_path(id, StoreFSTPathMode::Permanent);

        if collection_bucket_path.exists() {
            // Open graph at path for collection
            // SAFETY: This is unsafe, as loaded memory is a memory-mapped file, that cannot be
            //   guaranteed not to be muted while we own a read handle to it. Though, we use
            //   higher-level locking mechanisms on all callers of this method, so we are safe.
            unsafe { fst::Set::from_path(collection_bucket_path) }
        } else {
            // FST does not exist on disk; generate an empty FST for now
            // (until a consolidation task occurs and populates the on-disk FST).
            fst::Set::from_iter(std::iter::empty::<&str>())
        }
    }

    pub(super) fn close(&self, id: StoreFSTId) {
        tracing::debug!("Closing fst graph {id}");

        self.graph_pool.write().unwrap().remove(&id);
        self.graph_consolidate.write().unwrap().remove(&id);
    }

    pub fn janitor(&self, filter: impl Fn(&StoreFSTId) -> bool) {
        self.proceed_janitor(filter)
    }

    pub fn consolidate(&self, force: bool, filter: impl Fn(&StoreFSTId) -> bool) {
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
        let mut keys_consolidate: Vec<StoreFSTId> = Vec::new();

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
                            Duration::ZERO
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
            std::thread::yield_now();
        }

        tracing::info!(
            ?stats,
            "Done scanning for fst store pool items to consolidate"
        );
    }
}

#[derive(Debug, Default)]
struct ConsolidateStats {
    count_moved: usize,
    count_pushed: usize,
    count_popped: usize,
}

impl StoreFSTPool {
    fn consolidate_item(&self, store: &StoreFST, stats: &mut ConsolidateStats) -> Result<bool, ()> {
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
        let old_fst = (self.open(store.target))
            .map_err(|error| tracing::error!("Error opening old fst: {error:?}"))?;

        // Initialize the new FST (temporary).
        let bucket_tmp_path = self
            .fst_store_config
            .store_path(store.target, StoreFSTPathMode::Temporary);

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

        let tmp_fst_writer = io::BufWriter::new(tmp_fst_file);

        // Create a builder that can be used to insert new key-value pairs.
        let mut tmp_fst_builder = fst::SetBuilder::new(tmp_fst_writer).map_err(|error| {
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

                        if check_over_limits(
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
                if check_over_limits(
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
            if check_over_limits(
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
                let bucket_final_path = self
                    .fst_store_config
                    .store_path(store.target, StoreFSTPathMode::Permanent);

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

    pub fn erase(
        &self,
        collection: StoreItemPart,
        bucket: Option<StoreItemPart>,
    ) -> Result<u32, ()> {
        self.dispatch_erase(collection, bucket)
    }

    /// Counts buckets by reading the filesystem.
    pub fn count_collection_buckets(&self, collection: StoreItemPart) -> Result<usize, ()> {
        let path_mode = StoreFSTPathMode::Permanent;

        let collection_atom = collection.into_compact();
        let collection_path = self.fst_store_config.collection_path(collection_atom);

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
}

// MARK: - Store ID

#[derive(PartialEq, Eq, Hash, Clone, Copy)]
pub struct StoreFSTId {
    collection_hash: StoreFSTAtom,
    bucket_hash: StoreFSTAtom,
}

impl StoreFSTId {
    pub fn from_atoms(collection_hash: StoreFSTAtom, bucket_hash: StoreFSTAtom) -> StoreFSTId {
        StoreFSTId {
            collection_hash,
            bucket_hash,
        }
    }

    pub fn from_parts(collection: StoreItemPart, bucket: StoreItemPart) -> StoreFSTId {
        StoreFSTId {
            collection_hash: collection.into_compact(),
            bucket_hash: bucket.into_compact(),
        }
    }

    /// Filesystem path components are hex-encoded (via `format!("{:x}")`), we
    /// must convert it back into proper `u32` otherwise roundtrips will fail.
    #[inline]
    pub fn try_from_hex(collection_hash: &str, bucket_hash: &str) -> Result<StoreFSTId, io::Error> {
        let collection_hash = u32_from_hex(collection_hash)?;
        let bucket_hash = u32_from_hex(bucket_hash)?;

        Ok(Self::from_atoms(collection_hash, bucket_hash))
    }

    pub fn as_collection_hash(&self) -> &StoreFSTAtom {
        &self.collection_hash
    }
}

impl fmt::Display for StoreFSTId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let Self {
            collection_hash,
            bucket_hash,
        } = self;

        write!(f, "<{collection_hash:x}>/<{bucket_hash:x}>")
    }
}

// MARK: - Helpers

impl crate::config::StoreFSTConfig {
    #[inline]
    pub(super) fn collection_path(&self, collection_hash: StoreFSTAtom) -> PathBuf {
        self.path.join(format!("{collection_hash:x}"))
    }

    #[inline]
    pub(super) fn store_path(&self, id: StoreFSTId, mode: StoreFSTPathMode) -> PathBuf {
        let StoreFSTId {
            collection_hash,
            bucket_hash,
        } = id;

        let extension = mode.extension();
        assert!(extension.starts_with("."));

        self.collection_path(collection_hash)
            .join(format!("{bucket_hash:x}{extension}"))
    }
}

// MARK: - Tests

#[cfg(test)]
mod tests {
    use crate::store::fst::tests::test_fst_pool;

    #[test]
    fn it_acquires_graph() {
        let fst_pool = test_fst_pool();

        assert!(
            fst_pool
                .acquire("c:test:1".into(), "b:test:1".into())
                .is_ok()
        );
    }

    #[test]
    fn it_janitors_graph() {
        let fst_pool = test_fst_pool();

        fst_pool.janitor(|_| true);
    }

    #[test]
    fn it_proceeds_primitives() {
        let fst_pool = test_fst_pool();

        let store = fst_pool
            .acquire("c:test:2".into(), "b:test:2".into())
            .unwrap();

        assert!(store.lookup_typos_("valerien", 1).is_ok());
    }
}

// MARK: - Boilerplate

impl std::ops::Deref for StoreFSTPool {
    type Target = RwLock<HashMap<StoreFSTId, Arc<StoreFST>>>;

    fn deref(&self) -> &Self::Target {
        &self.graph_pool
    }
}

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

impl fmt::Debug for StoreFSTId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self, f)
    }
}
