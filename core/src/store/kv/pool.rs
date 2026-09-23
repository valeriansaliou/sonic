// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{Duration, SystemTime};
use std::{fmt, fs};

use hashbrown::{DefaultHashBuilder, HashMap};
use rocksdb::DB;

use crate::store::generic::*;
use crate::store::types::*;

use super::KvStore;

// MARK: - Store pool

pub type KvStoreId = CollectionHash;

// NOTE: This type cannot be generic over a lifetime as spawning threads would
//   force it to be `'static`.
#[derive(Clone)]
pub struct KvStorePool {
    pool: Arc<RwLock<HashMap<KvStoreId, Arc<KvStore>>>>,
    pub(super) kv_store_config: Arc<crate::config::KvStoreConfig>,
    pub(super) store_access_lock: Arc<RwLock<()>>,
    store_acquire_lock: Arc<Mutex<()>>,
    store_flush_lock: Arc<Mutex<()>>,
}

impl KvStorePool {
    pub fn new(kv_store_config: Arc<crate::config::KvStoreConfig>) -> Self {
        Self {
            pool: Arc::default(),
            kv_store_config,
            store_access_lock: Arc::default(),
            store_acquire_lock: Arc::default(),
            store_flush_lock: Arc::default(),
        }
    }

    pub fn count(&self) -> usize {
        self.pool.read().unwrap().len()
    }

    pub fn lock_read_access<'a>(&'a self) -> RwLockReadGuard<'a, ()> {
        self.store_access_lock.read().unwrap()
    }

    pub fn lock_write_access<'a>(&'a self) -> RwLockWriteGuard<'a, ()> {
        self.store_access_lock.write().unwrap()
    }
}

impl StoreGenericPool for KvStorePool {
    type StoreId = KvStoreId;
    type Store = KvStore;
    type HashBuilder = DefaultHashBuilder;

    fn kind() -> &'static str {
        "kv"
    }

    fn consider_inactive_after_secs(&self) -> u64 {
        self.kv_store_config.pool.inactive_after
    }

    fn access_lock(&self) -> &RwLock<()> {
        &self.store_access_lock
    }

    fn proceed_erase_collection(&self, collection: StoreItemPart) -> Result<u32, ()> {
        let store_id = KvStoreId::from_part(collection);
        let collection_path = self.kv_store_config.store_path(&store_id);

        // Force a KV store close
        self.close(store_id, None);

        if !collection_path.exists() {
            tracing::debug!(
                "kv collection store does not exist, consider already erased: {collection}/* at path: {collection_path:?}"
            );

            return Ok(0);
        }

        tracing::debug!(
            "kv collection store exists, erasing: {collection}/* at path: {collection_path:?}"
        );

        // Remove KV store storage from filesystem
        match fs::remove_dir_all(&collection_path) {
            Ok(()) => {
                tracing::debug!("done with kv collection erasure");

                Ok(1)
            }
            Err(_err) => Err(()),
        }
    }

    fn proceed_erase_bucket(&self, collection: StoreItemPart, bucket: Bucket) -> Result<u32, ()> {
        let kv_store = self
            .acquire(false, collection, None, |_| {})
            .map_err(|()| tracing::error!("failed erasing KV buckets"))?;

        let Some(kv_store) = kv_store else {
            tracing::debug!(
                "collection store does not exist, consider {bucket:?} from {collection:?} already erased"
            );
            return Ok(0);
        };

        // Important: acquire bucket store write lock
        let _write_guard = kv_store.lock.write().unwrap();

        // Store exists, proceed erasure.
        tracing::debug!("collection store exists, erasing: {bucket} from {collection}");

        let kv_repo = kv_store.to_repository_read_write(bucket);

        // Notice: we cannot use the provided KV bucket erasure helper there, as \
        //   erasing a bucket requires a database lock, which would incur a dead-lock, \
        //   thus we need to perform the erasure from there.
        kv_repo
            .batch_erase_bucket()
            .inspect(|_n| tracing::debug!("done with bucket erasure"))
    }
}

impl KvStorePool {
    // TODO(refactor): Replace `create_if_missing` and `override_options` by a
    //   struct. I(@RemiBardon) had suggested adding `bypass_cache: bool` before,
    //   but I don’t remember why.
    pub fn acquire<'a>(
        &'a self,
        create_if_missing: bool,
        collection: StoreItemPart,
        write_guard: Option<&mut RwLockWriteGuard<'a, HashMap<KvStoreId, Arc<KvStore>>>>,
        override_options: impl FnOnce(&mut rocksdb::Options),
    ) -> Result<Option<Arc<KvStore>>, ()> {
        let store_id = KvStoreId::from_part(collection);

        // Freeze acquire lock, and reference it in context
        // Notice: this prevents two databases on the same collection to be opened at the same time.
        let _acquire = self.store_acquire_lock.lock().unwrap();

        // Return cached value if store is already open.
        match write_guard {
            Some(ref store_pool_write) => {
                if let Some(store_kv) = store_pool_write.get(&store_id) {
                    return Self::proceed_acquire_cache(store_id, store_kv).map(Some);
                }
            }
            None => {
                let store_pool_read = self.pool.read().unwrap();

                if let Some(store_kv) = store_pool_read.get(&store_id) {
                    return Self::proceed_acquire_cache(store_id, store_kv).map(Some);
                }
            }
        };

        tracing::debug!("kv store {store_id} not in pool, opening it");

        // Check if can open database?
        let can_open_db = create_if_missing || self.kv_store_config.store_path(&store_id).exists();

        // Do not create a new KV database file tree if the database does not
        // exist yet on disk and we are just looking to read data from it.
        if !can_open_db {
            return Ok(None);
        }

        // Open KV database.
        self.proceed_acquire_open(
            store_id,
            |pool, store_id| pool.build(store_id, override_options),
            write_guard,
        )
        .map(Some)
    }

    fn build(
        &self,
        store_id: &KvStoreId,
        override_options: impl FnOnce(&mut rocksdb::Options),
    ) -> Result<KvStore, ()> {
        match self.open(store_id, override_options) {
            Ok(db) => {
                let now = SystemTime::now();

                Ok(KvStore {
                    database: db,
                    last_used: RwLock::new(now),
                    last_flushed: RwLock::new(now),
                    lock: RwLock::new(()),
                    kv_store_config: Arc::clone(&self.kv_store_config),
                    iid_incr_per_bucket: RwLock::new(HashMap::new()),
                })
            }
            Err(err) => {
                tracing::error!("failed opening kv: {err}");

                Err(())
            }
        }
    }

    pub(super) fn open(
        &self,
        store_id: &KvStoreId,
        override_options: impl FnOnce(&mut rocksdb::Options),
    ) -> Result<DB, rocksdb::Error> {
        tracing::debug!("opening key-value database for collection: {store_id}");

        // Configure database options
        tracing::debug!("configuring key-value database");
        let mut db_options = rocksdb::Options::from(&self.kv_store_config.database);

        db_options.set_merge_operator_associative("kv_merge", super::merge::kv_merge_operator);

        override_options(&mut db_options);

        // Open database at path for collection
        DB::open(&db_options, self.kv_store_config.store_path(store_id))
    }

    pub fn close<'a>(
        &'a self,
        store_id: KvStoreId,
        write_guard: Option<&mut RwLockWriteGuard<'a, HashMap<KvStoreId, Arc<KvStore>>>>,
    ) {
        tracing::debug!("closing key-value database for collection: {store_id}");

        let store_pool_write = match write_guard {
            Some(x) => x,
            None => &mut self.pool.write().unwrap(),
        };

        store_pool_write.remove(&store_id);
    }

    // NOTE: This wrapper makes `janitor` public, while `proceed_janitor` comes
    //   from a private trait.
    pub fn janitor(&self, filter: impl Fn(&KvStoreId) -> bool) {
        self.proceed_janitor(filter)
    }

    pub fn flush(&self, force: bool, filter: impl Fn(&KvStoreId) -> bool) {
        tracing::debug!("scanning for kv store pool items to flush to disk");

        // Acquire flush lock, and reference it in context
        // Notice: this prevents two flush operations to be executed at the same time.
        let _flush = self.store_flush_lock.lock().unwrap();

        // Step 1: List keys to be flushed
        let mut keys_flush: Vec<KvStoreId> = Vec::new();

        let store_pool_read = self.pool.read().unwrap();

        for (key, store) in store_pool_read.iter().filter(|(k, _)| filter(k)) {
            let last_flushed_guard = store.last_flushed.read().unwrap();

            let not_flushed_for = (last_flushed_guard.elapsed())
                // WARN: Be lenient with system clock going back to a past
                //   duration, since we may be running in a virtualized
                //   environment where clock is not guaranteed to be
                //   monotonic. This is done to avoid poisoning associated
                //   locks by crashing on `.unwrap()`.
                .unwrap_or_else(|err| {
                    tracing::error!(
                        "kv key: {key} last flush duration clock issue, zeroing: {err}"
                    );

                    // Assuming a zero seconds fallback duration
                    Duration::ZERO
                });

            drop(last_flushed_guard);

            if force || not_flushed_for.as_secs() >= self.kv_store_config.database.flush_after {
                tracing::info!("kv key: {key} not flushed for: {not_flushed_for:.0?}, may flush");

                keys_flush.push(*key);
            } else {
                tracing::debug!("kv key: {key} not flushed for: {not_flushed_for:.0?}, no flush");
            }
        }

        // Early release lock.
        drop(store_pool_read);

        // Exit trap: Nothing to flush yet? Abort there.
        if keys_flush.is_empty() {
            tracing::info!("no kv store pool items need to be flushed at the moment");

            return;
        }

        // Step 2: Flush KVs, one-by-one (sequential locking; this avoids global locks)
        let mut count_flushed = 0;

        for key in keys_flush.iter() {
            let pool_guard = self.pool.read().unwrap();

            if let Some(store) = pool_guard.get(key) {
                tracing::debug!("kv key: {key} flush started");

                if let Err(err) = store.flush() {
                    tracing::error!("kv key: {key} flush failed: {err}");
                } else {
                    count_flushed += 1;

                    tracing::debug!("kv key: {key} flush complete");
                }

                // Bump 'last flushed' time
                *store.last_flushed.write().unwrap() = SystemTime::now();
            }

            // Early release the lock.
            drop(pool_guard);

            // Give a bit of time to other threads before continuing
            std::thread::yield_now();
        }

        tracing::info!(
            "done scanning for kv store pool items to flush to disk (flushed: {count_flushed})"
        );
    }

    pub fn compact(&self, collections_opt: Option<&[StoreItemPart]>) {
        match collections_opt {
            Some(collections) => tracing::debug!("compacting {collections:?}…"),
            None => tracing::debug!("compacting all collections…"),
        }

        let store_ids: Vec<KvStoreId> = match collections_opt {
            Some(collections) => collections
                .iter()
                .map(|&c| KvStoreId::from_part(c))
                .collect(),
            None => {
                let pool_guard = self.pool.read().unwrap();

                let store_ids = pool_guard.keys().map(KvStoreId::to_owned).collect();

                drop(pool_guard);

                store_ids
            }
        };

        for store_id in store_ids.iter() {
            let pool_guard = self.pool.write().unwrap();

            let Some(store) = pool_guard.get(store_id).map(Arc::clone) else {
                tracing::warn!("Cannot compact {store_id:?}: no open connection");
                continue;
            };

            // Early release the lock.
            drop(pool_guard);

            // Compact whole range of keys (we can hardly predict the range here).
            store.database.compact_range::<&[u8], &[u8]>(None, None);

            // Give a bit of time to other threads before continuing
            // PERF: Compactions can take a very long time, and collections are
            //   likely to be very few, so it’s better to yield between runs.
            std::thread::yield_now();
        }

        tracing::info!("done compacting {store_ids:?}");
    }

    pub fn erase(&self, collection: StoreItemPart, bucket: Option<Bucket>) -> Result<u32, ()> {
        self.dispatch_erase(collection, bucket)
    }
}

// MARK: - Helpers

impl crate::config::KvStoreConfig {
    #[inline]
    pub(super) fn store_path(&self, id: &KvStoreId) -> PathBuf {
        let collection_hash = id.into_inner();

        self.path.join(format!("{collection_hash:x}"))
    }
}

// MARK: - Tests

#[cfg(test)]
mod tests {
    use crate::store::kv::tests::test_kv_store_config;

    use super::*;

    #[test]
    fn it_acquires_database() {
        let kv_store_config = test_kv_store_config();
        let kv_pool = KvStorePool::new(kv_store_config);

        assert!(
            kv_pool
                .acquire(true, "c:test:1".into(), None, |_| {})
                .is_ok()
        );
    }

    #[test]
    fn it_janitors_database() {
        let kv_store_config = test_kv_store_config();
        let kv_pool = KvStorePool::new(kv_store_config);

        kv_pool.janitor(|_| true);
    }
}

// MARK: - Boilerplate

impl std::ops::Deref for KvStorePool {
    type Target = RwLock<HashMap<KvStoreId, Arc<KvStore>>>;

    fn deref(&self) -> &Self::Target {
        &self.pool
    }
}

impl fmt::Debug for KvStorePool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use crate::util::fmt::{AsPrettyMutex, AsPrettyRwLock};

        // NOTE: Deconstructing to future-proof this function.
        let Self {
            pool,
            store_access_lock,
            store_acquire_lock,
            store_flush_lock,
            // NOTE: We don’t care about the configuration,
            //   we can see it elsewhere if needed.
            kv_store_config: _kv_store_config,
        } = self;

        f.debug_struct("KvStorePool")
            .field("pool", &AsPrettyRwLock(pool))
            .field("store_access_lock", &AsPrettyRwLock(store_access_lock))
            .field("store_acquire_lock", &AsPrettyMutex(store_acquire_lock))
            .field("store_flush_lock", &AsPrettyMutex(store_flush_lock))
            .finish_non_exhaustive()
    }
}
