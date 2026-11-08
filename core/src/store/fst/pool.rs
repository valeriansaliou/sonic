// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::path::PathBuf;
use std::str::FromStr as _;
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::Instant;
use std::{fmt, fs};

use hashbrown::{DefaultHashBuilder, HashMap};

use crate::Context as _;
use crate::store::fst::ConsolidateStats;
use crate::store::{AsCollectionHash, Bucket, CollectionHash, Hash, StoreItemPart};
use crate::store::{BucketOwned, generic::*};

use super::{FstRepositoryConfig, FstStore, FstStorePathMode};

// MARK: - Store pool

// NOTE: This type cannot be generic over a lifetime as spawning threads would
//   force it to be `'static`.
#[derive(Clone)]
pub struct FstStorePool {
    pub(super) fst_store_config: Arc<crate::config::FstStoreConfig>,
    // NOTE: This shouldn’t be here, but until a big rewrite let’s not care.
    pub fst_repo_config: FstRepositoryConfig,
    graph_pool: Arc<RwLock<HashMap<FstStoreId, Arc<FstStore>>>>,
    graph_acquire_lock: Arc<Mutex<()>>,
    graph_rebuild_lock: Arc<Mutex<()>>,
    pub(super) graph_access_lock: Arc<RwLock<()>>,
}

impl FstStorePool {
    pub fn new(
        fst_store_config: Arc<crate::config::FstStoreConfig>,
        fst_repo_config: FstRepositoryConfig,
    ) -> Self {
        Self {
            fst_store_config,
            fst_repo_config,
            graph_pool: Arc::default(),
            graph_acquire_lock: Arc::default(),
            graph_rebuild_lock: Arc::default(),
            graph_access_lock: Arc::default(),
        }
    }

    pub fn count(&self) -> (usize, usize) {
        use std::sync::atomic;

        let pool = self.graph_pool.read().unwrap();

        let open_count = pool.len();
        let consolidate_count = (pool.values())
            .filter(|store| store.should_consolidate.load(atomic::Ordering::Acquire))
            .count();

        (open_count, consolidate_count)
    }

    pub fn lock_read_access<'a>(&'a self) -> RwLockReadGuard<'a, ()> {
        self.graph_access_lock.read().unwrap()
    }

    pub fn lock_write_access<'a>(&'a self) -> RwLockWriteGuard<'a, ()> {
        self.graph_access_lock.write().unwrap()
    }
}

impl GenericStorePool for FstStorePool {
    type StoreId = FstStoreId;
    type Store = FstStore;
    type HashBuilder = DefaultHashBuilder;

    fn consider_inactive_after_secs(&self) -> u64 {
        self.fst_store_config.pool.inactive_after
    }

    fn access_lock(&self) -> &RwLock<()> {
        &self.graph_access_lock
    }

    #[tracing::instrument(
        level = "debug",
        skip_all, fields(collection = ?collection_name, collection_path = tracing::field::Empty),
    )]
    fn proceed_erase_collection(
        &self,
        collection_name: StoreItemPart,
    ) -> Result<u32, crate::Error> {
        let collection_hash = CollectionHash::from_part(collection_name);
        let collection_path = self.fst_store_config.collection_path(&collection_hash);

        tracing::Span::current().record("collection_path", tracing::field::debug(&collection_path));

        // Force a FST graph close (on all contained buckets)
        // NOTE: we first need to scan for opened buckets in-memory, as not all FSTs may be
        //   committed to disk; thus some FST stores that exist in-memory may not exist on-disk.
        // TODO(perf): Instead of collecting into a `Vec` just to check `is_empty` and
        //   lock only if necessary, use a `LazyCell` to do the same in a single step.
        let mut buckets: Vec<BucketOwned> = Vec::new();

        {
            let graph_pool_read = self.graph_pool.read().unwrap();

            for store_id in graph_pool_read.keys() {
                if store_id.collection_hash == collection_hash {
                    buckets.push(store_id.bucket.clone());
                }
            }
        }

        if !buckets.is_empty() {
            tracing::trace!(
                "Will force-close {nbuckets} FST buckets",
                nbuckets = buckets.len()
            );

            let mut graph_pool_write = self.graph_pool.write().unwrap();

            for bucket in buckets {
                tracing::debug!(?bucket, "Force-closing FST graph…");

                let store_id = FstStoreId::new(collection_hash, bucket);

                graph_pool_write.remove(&store_id);
            }
        }

        // Remove all on-disk FSTs.
        if collection_path.exists() {
            tracing::trace!("FST collection store exists, erasing it…");

            // Remove FST graph storage from filesystem.
            fs::remove_dir_all(&collection_path).context("Failed erasing FST collection")?;

            tracing::info!(?collection_name, "FST collection erased successfully.");

            Ok(1)
        } else {
            tracing::debug!("FST collection store does not exist, consider already erased.");

            Ok(0)
        }
    }

    #[tracing::instrument(
        level = "debug",
        skip_all, fields(collection = ?collection_name, bucket = ?bucket_name, bucket_path = tracing::field::Empty),
    )]
    fn proceed_erase_bucket(
        &self,
        collection_name: StoreItemPart,
        bucket_name: Bucket,
    ) -> Result<u32, crate::Error> {
        tracing::debug!("Erasing FST bucket graph…");

        let store_id = FstStoreId::new(collection_name, bucket_name);

        let bucket_path = self
            .fst_store_config
            .store_path(&store_id, FstStorePathMode::Permanent);

        tracing::Span::current().record("bucket_path", tracing::field::debug(&bucket_path));

        // Force a FST graph close.
        self.close(&store_id);

        // Remove on-disk FST.
        if bucket_path.exists() {
            tracing::trace!("FST bucket graph exists, erasing it…");

            // Remove FST graph storage from filesystem.
            fs::remove_file(&bucket_path).context("Failed erasing FST bucket")?;

            tracing::info!(%store_id, "FST bucket graph erased successfully.");

            Ok(1)
        } else {
            tracing::debug!("FST bucket graph does not exist, consider already erased.");

            Ok(0)
        }
    }
}

impl FstStorePool {
    #[tracing::instrument(
        level = "debug",
        skip_all, fields(store = %Self::kind(), store_id = tracing::field::Empty),
    )]
    pub fn acquire(
        &self,
        collection: StoreItemPart,
        bucket: Bucket,
    ) -> Result<Arc<FstStore>, crate::Error> {
        let store_id = FstStoreId::new(collection, bucket);

        tracing::Span::current().record("store_id", tracing::field::display(&store_id));

        // Freeze acquire lock, and reference it in context
        // Notice: this prevents two graphs on the same collection to be opened at the same time.
        let _acquire = self.graph_acquire_lock.lock().unwrap();

        // Acquire a thread-safe store pool reference in read mode
        let graph_pool_read = self.graph_pool.read().unwrap();

        if let Some(store_fst) = graph_pool_read.get(&store_id) {
            Self::proceed_acquire_cache(store_fst)
        } else {
            tracing::debug!("FST store not in pool, opening it…");

            // Important: we need to drop the read reference first, to avoid dead-locking \
            //   when acquiring the RWLock in write mode in this block.
            drop(graph_pool_read);

            self.proceed_acquire_open(store_id, Self::build, None)
        }
    }

    fn build(&self, store_id: &FstStoreId) -> Result<FstStore, crate::Error> {
        let graph =
            FstStore::open(store_id, &self.fst_store_config).context("Failed opening FST")?;

        let now = Instant::now();

        Ok(FstStore {
            graph,
            config: Arc::clone(&self.fst_store_config),
            target: store_id.clone(),
            pending: Default::default(),
            last_used: Arc::new(RwLock::new(now)),
            last_consolidated: Arc::new(RwLock::new(now)),
            should_consolidate: Default::default(),
            action_config: self.fst_repo_config,
        })
    }

    pub(super) fn close(&self, store_id: &FstStoreId) {
        tracing::debug!(%store_id, "Closing FST store");

        self.graph_pool.write().unwrap().remove(store_id);
    }

    pub fn janitor(&self, filter: impl Fn(&FstStoreId) -> bool) {
        self.proceed_janitor(filter)
    }

    #[tracing::instrument(
        level = "info",
        skip_all, fields(store = %Self::kind()),
    )]
    pub fn consolidate(&self, force: bool, filter: impl Fn(&FstStoreId) -> bool) {
        use std::sync::atomic;

        tracing::debug!("Scanning for FST stores to consolidate");

        // Notice: we do not consolidate all items at each tick, we try to even out multiple \
        //   consolidation tasks over time. This lowers the overall HZ of the tasker system for \
        //   certain heavy tasks, which is better to spread out consolidation steps over time over \
        //   a large number of very active buckets.

        // Acquire rebuild lock, and reference it in context
        // Notice: this prevents two consolidate operations to be executed at the same time.
        let _rebuild = self.graph_rebuild_lock.lock().unwrap();

        // Step 1: List keys to be consolidated
        let mut keys_consolidate: Vec<FstStoreId> = Vec::new();

        {
            // Acquire access lock (in blocking write mode), and reference it in context
            // Notice: this prevents store to be acquired from any context
            let _access = self.graph_access_lock.write().unwrap();

            let graph_pool_read = self.graph_pool.read().unwrap();

            for store in (graph_pool_read.values())
                // WARN: Filter before doing the read-and-update on `should_consolidate`.
                .filter(|store| filter(&store.target))
                .filter(|store| store.should_consolidate.load(atomic::Ordering::Acquire))
            {
                let store_id = &store.target;

                let not_consolidated_for = store.last_consolidated.read().unwrap().elapsed();

                if force
                    || not_consolidated_for.as_secs()
                        >= self.fst_store_config.graph.consolidate_after
                {
                    tracing::trace!(
                        "FST store {store_id} not consolidated for {not_consolidated_for:.1?}, may consolidate"
                    );

                    keys_consolidate.push(store_id.clone());
                } else {
                    tracing::trace!(
                        "FST store {store_id} not consolidated for {not_consolidated_for:.1?}, no consolidate"
                    );
                }
            }
        }

        // Exit trap: Nothing to consolidate yet? Abort there.
        if keys_consolidate.is_empty() {
            tracing::debug!("No FST store needs to be consolidated at the moment");
            return;
        }

        // Step 2: Consolidate FSTs, one-by-one (sequential locking; this avoids global locks)
        let mut stats = ConsolidateStats::default();

        for key in &keys_consolidate {
            // As we may be renaming the FST file, ensure no consumer out of this is
            // trying to access the FST file as it gets processed. This also waits for
            // current consumers to finish reading the FST, and prevents any new
            // consumer from opening it while we are not done there.
            let access_guard = self.graph_access_lock.write().unwrap();

            let do_close = if let Some(store) = self.graph_pool.read().unwrap().get(key) {
                #[allow(
                    clippy::unnecessary_lazy_evaluations,
                    reason = "Ensures errors are handled"
                )]
                let should_close = store.consolidate(&mut stats).unwrap_or_else(|error| {
                    tracing::error!(%key, "Failed consolidating: {error:#}");
                    false
                });

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

        tracing::info!(?stats, "Consolidated {} store(s)", keys_consolidate.len());
    }
}

impl FstStorePool {
    pub fn erase(
        &self,
        collection: StoreItemPart,
        bucket: Option<Bucket>,
    ) -> Result<u32, crate::Error> {
        self.dispatch_erase(collection, bucket)
            .context("FST erasure failed")
    }

    /// Counts buckets by reading the filesystem.
    #[tracing::instrument(
        level = "debug",
        skip_all, fields(?collection, collection_path = tracing::field::Empty),
    )]
    pub fn count_collection_buckets(
        &self,
        collection: StoreItemPart,
    ) -> Result<usize, crate::Error> {
        let path_mode = FstStorePathMode::Permanent;

        let collection_hash = CollectionHash::from_part(collection);
        let collection_path = self.fst_store_config.collection_path(&collection_hash);

        tracing::Span::current().record("collection_path", tracing::field::debug(&collection_path));

        if !collection_path.exists() {
            return Ok(0);
        }

        let entries =
            fs::read_dir(&collection_path).context("Failed reading collection directory")?;

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

#[derive(PartialEq, Eq, Hash, Clone)]
pub struct FstStoreId {
    collection_hash: CollectionHash,
    pub(super) bucket: BucketOwned,
}

impl FstStoreId {
    pub fn new(
        collection_hash: impl Into<CollectionHash>,
        bucket: impl Into<BucketOwned>,
    ) -> FstStoreId {
        FstStoreId {
            collection_hash: collection_hash.into(),
            bucket: bucket.into(),
        }
    }

    /// Filesystem path components are hex-encoded (via `format!("{:x}")`), we
    /// must convert it back into proper `u32` otherwise roundtrips will fail.
    #[inline]
    pub fn try_from_hex(
        collection_hash: &str,
        bucket_name: &str,
    ) -> Result<FstStoreId, crate::Error> {
        let collection_hash = CollectionHash::try_from_hex(collection_hash)?;
        let bucket_name = BucketOwned::from_str(bucket_name)?;

        Ok(Self::new(collection_hash, bucket_name))
    }
}

impl AsCollectionHash for FstStoreId {
    fn as_collection_hash(&self) -> &Hash {
        self.collection_hash.as_collection_hash()
    }
}

impl fmt::Display for FstStoreId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let Self {
            collection_hash,
            bucket,
        } = self;

        write!(f, "{collection_hash}/{bucket}")
    }
}

// MARK: - Helpers

impl crate::config::FstStoreConfig {
    #[inline]
    pub(super) fn collection_path(&self, collection_hash: &CollectionHash) -> PathBuf {
        self.path.join(collection_hash.to_hex())
    }

    #[inline]
    pub(super) fn store_path(&self, id: &FstStoreId, mode: FstStorePathMode) -> PathBuf {
        let FstStoreId {
            collection_hash,
            bucket,
        } = id;

        let bucket = bucket.as_str();
        // FIXME: Introduce proper parsing/validation, this is just a last resort debug-only check!
        debug_assert!(!bucket.contains("../"));

        let extension = mode.extension();
        assert!(extension.starts_with("."));

        self.collection_path(collection_hash)
            .join(format!("{bucket}{extension}"))
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
}

// MARK: - Boilerplate

impl std::ops::Deref for FstStorePool {
    type Target = RwLock<HashMap<FstStoreId, Arc<FstStore>>>;

    fn deref(&self) -> &Self::Target {
        &self.graph_pool
    }
}

impl fmt::Debug for FstStorePool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use crate::util::fmt::{AsPrettyMutex, AsPrettyRwLock};

        // NOTE: Deconstructing to future-proof this function.
        let Self {
            fst_repo_config: fst_action_config,
            graph_pool,
            graph_acquire_lock,
            graph_rebuild_lock,
            graph_access_lock,
            // NOTE: We don’t care about the configuration,
            //   we can see it elsewhere if needed.
            fst_store_config: _fst_store_config,
        } = self;

        f.debug_struct("FstStorePool")
            .field("fst_action_config", fst_action_config)
            .field("graph_pool", &AsPrettyRwLock(graph_pool))
            .field("graph_acquire_lock", &AsPrettyMutex(graph_acquire_lock))
            .field("graph_rebuild_lock", &AsPrettyMutex(graph_rebuild_lock))
            .field("graph_access_lock", &AsPrettyRwLock(graph_access_lock))
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for FstStoreId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self, f)
    }
}
