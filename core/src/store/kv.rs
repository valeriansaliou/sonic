// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use byteorder::{ByteOrder, LittleEndian, ReadBytesExt};
use hashbrown::HashMap;
use radix::RadixNum;
use rocksdb::backup::{
    BackupEngine as DBBackupEngine, BackupEngineOptions as DBBackupEngineOptions,
    RestoreOptions as DBRestoreOptions,
};
use rocksdb::{
    DB, DBCompactionStyle, DBCompressionType, Env as DBEnv, Error as DBError, FlushOptions,
    MergeOperands, WriteBatch, WriteOptions,
};
use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::io::{self, Cursor};
use std::path::{Path, PathBuf};
use std::str;
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::thread;
use std::time::{Duration, SystemTime};

use crate::config::ConfigStoreKVDatabase;

use super::generic::{
    StoreGeneric, StoreGenericActionBuilder, StoreGenericBuilder, StoreGenericPool,
};
use super::identifiers::*;
use super::item::StoreItemPart;
use super::keyer::{StoreKeyerBuilder, StoreKeyerHasher, StoreKeyerKey, StoreKeyerPrefix};

// NOTE: This type cannot be generic over a lifetime as spawning threads would
//   force it to be `'static`.
#[derive(Clone)]
pub struct StoreKVPool {
    pool: Arc<RwLock<HashMap<StoreKVKey, Arc<StoreKV>>>>,
    kv_store_config: Arc<crate::config::ConfigStoreKV>,
    store_access_lock: Arc<RwLock<()>>,
    store_acquire_lock: Arc<Mutex<()>>,
    store_flush_lock: Arc<Mutex<()>>,
}

pub struct StoreKVBuilder {
    kv_store_config: Arc<crate::config::ConfigStoreKV>,
}

pub struct StoreKV {
    database: DB,
    last_used: RwLock<SystemTime>,
    last_flushed: RwLock<SystemTime>,
    pub lock: RwLock<()>,
    kv_store_config: Arc<crate::config::ConfigStoreKV>,
}

pub struct StoreKVActionBuilder<'build> {
    pub kv_pool: &'build StoreKVPool,
}

pub struct StoreKVActionReadOnly<'a> {
    bucket: StoreItemPart<'a>,
    store: Arc<StoreKV>,
}

pub struct StoreKVActionReadWrite<'a> {
    bucket: StoreItemPart<'a>,
    store: Arc<StoreKV>,
}

#[derive(PartialEq, Eq, Hash, Clone, Copy)]
pub struct StoreKVKey {
    collection_hash: StoreKVAtom,
}

#[derive(PartialEq)]
pub enum StoreKVAcquireMode {
    Any,
    OpenOnly,
}

type StoreKVAtom = u32;

const ATOM_HASH_RADIX: usize = 16;

impl StoreKVPool {
    pub fn new(kv_store_config: Arc<crate::config::ConfigStoreKV>) -> Self {
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

    pub fn acquire(
        &self,
        mode: StoreKVAcquireMode,
        collection: impl AsRef<str>,
    ) -> Result<Option<Arc<StoreKV>>, ()> {
        let collection = collection.as_ref();
        let pool_key = StoreKVKey::from_str(collection);

        // Freeze acquire lock, and reference it in context
        // Notice: this prevents two databases on the same collection to be opened at the same time.
        let _acquire = self.store_acquire_lock.lock().unwrap();

        // Acquire a thread-safe store pool reference in read mode
        let store_pool_read = self.pool.read().unwrap();

        if let Some(store_kv) = store_pool_read.get(&pool_key) {
            return Self::proceed_acquire_cache("kv", collection, pool_key, store_kv).map(Some);
        }

        tracing::info!("kv store not in pool for collection: {collection} {pool_key}, opening it");

        // Important: we need to drop the read reference first, to avoid \
        //   dead-locking when acquiring the RWLock in write mode in this block.
        drop(store_pool_read);

        // Check if can open database?
        let can_open_db = if mode == StoreKVAcquireMode::OpenOnly {
            self.kv_store_config.path(pool_key.collection_hash).exists()
        } else {
            true
        };

        // Do not create a new KV database file tree if the database does not
        // exist yet on disk and we are just looking to read data from it.
        if !can_open_db {
            return Ok(None);
        }

        let builder = StoreKVBuilder {
            kv_store_config: Arc::clone(&self.kv_store_config),
        };

        // Open KV database.
        Self::proceed_acquire_open("kv", collection, pool_key, &self.pool, &builder).map(Some)
    }

    fn close(&self, collection_hash: StoreKVAtom) {
        tracing::debug!("closing key-value database for collection: <{collection_hash:x}>");

        let mut store_pool_write = self.pool.write().unwrap();

        let collection_target = StoreKVKey::from_atom(collection_hash);

        store_pool_write.remove(&collection_target);
    }

    pub fn janitor(&self) {
        Self::proceed_janitor(
            "kv",
            &self.pool,
            self.kv_store_config.pool.inactive_after,
            &self.store_access_lock,
        )
    }

    pub fn backup(&self, path: &Path) -> Result<(), io::Error> {
        tracing::debug!("backing up all kv stores to path: {path:?}");

        // Create backup directory (full path)
        fs::create_dir_all(path)?;

        // Proceed dump action (backup)
        self.dump_action(
            "backup",
            &self.kv_store_config.path,
            path,
            &Self::backup_item,
        )
    }

    pub fn restore(&self, path: &Path) -> Result<(), io::Error> {
        tracing::debug!("restoring all kv stores from path: {path:?}");

        // Proceed dump action (restore)
        self.dump_action(
            "restore",
            path,
            &self.kv_store_config.path,
            &Self::restore_item,
        )
    }

    pub fn flush(&self, force: bool) {
        tracing::debug!("scanning for kv store pool items to flush to disk");

        // Acquire flush lock, and reference it in context
        // Notice: this prevents two flush operations to be executed at the same time.
        let _flush = self.store_flush_lock.lock().unwrap();

        // Step 1: List keys to be flushed
        let mut keys_flush: Vec<StoreKVKey> = Vec::new();

        let store_pool_read = self.pool.read().unwrap();

        for (key, store) in store_pool_read.iter() {
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
            thread::yield_now();
        }

        tracing::info!(
            "done scanning for kv store pool items to flush to disk (flushed: {count_flushed})"
        );
    }

    #[allow(clippy::type_complexity)]
    fn dump_action(
        &self,
        action: &str,
        read_path: &Path,
        write_path: &Path,
        fn_item: &dyn Fn(&Self, &Path, &Path, &str) -> Result<(), io::Error>,
    ) -> Result<(), io::Error> {
        // Iterate on KV collections.
        for entry in fs::read_dir(read_path)? {
            let Ok(collection) = entry else {
                continue;
            };

            // Actual collection found?
            if !collection.file_type().is_ok_and(|f| f.is_dir()) {
                continue;
            }

            if let Some(collection_name) = collection.file_name().to_str() {
                tracing::debug!("kv collection ongoing {action}: {collection_name}");

                fn_item(self, write_path, &collection.path(), collection_name)?;
            }
        }

        Ok(())
    }

    fn backup_item(
        &self,
        backup_path: &Path,
        _origin_path: &Path,
        collection_name: &str,
    ) -> Result<(), io::Error> {
        // Acquire access lock (in blocking write mode), and reference it in context
        // Notice: this prevents store to be acquired from any context
        let _access = self.store_access_lock.write().unwrap();

        // Generate path to KV backup
        let kv_backup_path = backup_path.join(collection_name);

        tracing::debug!("kv collection: {collection_name} backing up to path: {kv_backup_path:?}");

        // Erase any previously-existing KV backup
        if kv_backup_path.exists() {
            fs::remove_dir_all(&kv_backup_path)?;
        }

        // Create backup folder for collection
        fs::create_dir_all(backup_path.join(collection_name))?;

        // Convert names to hashes (as names are hashes encoded as base-16
        // strings, but we need them as proper integers)
        let Ok(collection_hash) =
            RadixNum::from_str(collection_name, ATOM_HASH_RADIX).and_then(|num| num.as_decimal())
        else {
            return Ok(());
        };

        let origin_kv = StoreKVBuilder {
            kv_store_config: Arc::clone(&self.kv_store_config),
        }
        .open(collection_hash as StoreKVAtom)
        .map_err(|_| io::Error::other("database open failure"))?;

        // Initialize KV database backup engine
        let kv_backup_options = DBBackupEngineOptions::new(&kv_backup_path)
            .map_err(|_| io::Error::other("backup engine options acquire failure"))?;
        let kv_backup_environment = DBEnv::new()
            .map_err(|_| io::Error::other("backup engine environment acquire failure"))?;

        let mut kv_backup_engine = DBBackupEngine::open(&kv_backup_options, &kv_backup_environment)
            .map_err(|_| io::Error::other("backup engine failure"))?;

        // Proceed actual KV database backup
        kv_backup_engine
            .create_new_backup(&origin_kv)
            .map_err(|_| io::Error::other("database backup failure"))?;

        tracing::info!("kv collection: {collection_name} backed up to path: {kv_backup_path:?}");

        Ok(())
    }

    fn restore_item(
        &self,
        _backup_path: &Path,
        origin_path: &Path,
        collection_name: &str,
    ) -> Result<(), io::Error> {
        // Acquire access lock (in blocking write mode), and reference it in context
        // Notice: this prevents store to be acquired from any context
        let _access = self.store_access_lock.write().unwrap();

        tracing::debug!("kv collection: {collection_name} restoring from path: {origin_path:?}");

        // Convert names to hashes (as names are hashes encoded as base-16
        // strings, but we need them as proper integers)
        let Ok(collection_hash) =
            RadixNum::from_str(collection_name, ATOM_HASH_RADIX).and_then(|num| num.as_decimal())
        else {
            return Ok(());
        };

        // Force a KV store close
        self.close(collection_hash as StoreKVAtom);

        // Generate path to KV
        let kv_path = self.kv_store_config.path(collection_hash as StoreKVAtom);

        // Remove existing KV database data?
        if kv_path.exists() {
            fs::remove_dir_all(&kv_path)?;
        }

        // Create KV folder for collection
        fs::create_dir_all(&kv_path)?;

        // Initialize KV database backup engine
        let kv_backup_options = DBBackupEngineOptions::new(&origin_path)
            .map_err(|_| io::Error::other("backup engine options acquire failure"))?;
        let kv_backup_environment = DBEnv::new()
            .map_err(|_| io::Error::other("backup engine environment acquire failure"))?;

        let mut kv_backup_engine = DBBackupEngine::open(&kv_backup_options, &kv_backup_environment)
            .map_err(|_| io::Error::other("backup engine failure"))?;

        kv_backup_engine
            .restore_from_latest_backup(&kv_path, &kv_path, &DBRestoreOptions::default())
            .map_err(|_| io::Error::other("database restore failure"))?;

        tracing::info!(
            "kv collection: {collection_name} restored to path: {kv_path:?} from backup: {origin_path:?}"
        );

        Ok(())
    }
}

impl StoreGenericPool<StoreKVKey, StoreKV, StoreKVBuilder> for StoreKVPool {}

impl StoreKVBuilder {
    fn open(&self, collection_hash: StoreKVAtom) -> Result<DB, DBError> {
        tracing::debug!("opening key-value database for collection: <{collection_hash:x}>");

        // Configure database options
        let db_options = self.configure();

        // Open database at path for collection
        DB::open(&db_options, self.kv_store_config.path(collection_hash))
    }

    #[rustfmt::skip]
    fn configure(&self) -> rocksdb::Options {
        tracing::debug!("configuring key-value database");

        // NOTE: Deconstruct to avoid forgetting configuration keys.
        let ConfigStoreKVDatabase {
            flush_after: _,
            compress,
            parallelism,
            max_open_files,
            max_flushes,
            write_ahead_log: _,
            write_buffer_size,
            max_write_buffer_number,
            min_write_buffer_number,
            min_write_buffer_number_to_merge,
            block_cache_size,
            cache_index_and_filter_blocks,
            compression_type,
            wal_compression_type,
            wal_ttl_seconds,
            wal_size_limit_mb,
            wal_bytes_per_sync,
            wal_recovery_mode,
            compression_level,
            min_level_to_compress,
            level_zero_file_num_compaction_trigger,
            level_zero_slowdown_writes_trigger,
            level_zero_stop_writes_trigger,
            max_bytes_for_level_base,
            max_bytes_for_level_multiplier,
            target_file_size_base,
            max_background_jobs,
            max_subcompactions,
            stats_dump_period_sec,
        } = &self.kv_store_config.database;

        // Make database options
        let mut db_options = rocksdb::Options::default();

        macro_rules! if_some {
            ($opts:ident.$set_fn:ident($value:expr)) => {
                if let Some(value) = $value {
                    $opts.$set_fn(*value);
                }
            };
        }

        // Set static options
        db_options.create_if_missing(true);
        db_options.set_use_fsync(false);
        db_options.set_compaction_style(DBCompactionStyle::Level);
        db_options.set_merge_operator_associative("default_merge", default_merge_operator);

        // Set dynamic options
        if_some!(db_options.set_write_buffer_size(write_buffer_size.map(|n| n * 1024).as_ref()));
        if_some!(db_options.set_min_write_buffer_number(min_write_buffer_number));
        if_some!(db_options.set_min_write_buffer_number_to_merge(min_write_buffer_number_to_merge));
        if_some!(db_options.set_max_write_buffer_number(max_write_buffer_number));

        if_some!(db_options.set_max_open_files(max_open_files));

        // db_options.set_block_cache_size();
        // db_options.set_cache_index_and_filter_blocks();

        if let Some(block_cache_size) = block_cache_size {
            let cache = rocksdb::Cache::new_lru_cache((*block_cache_size as usize) * 1024 * 1024);
            let mut block_opts = rocksdb::BlockBasedOptions::default();
            block_opts.set_block_cache(&cache);
            if_some!(block_opts.set_cache_index_and_filter_blocks(cache_index_and_filter_blocks));
            db_options.set_block_based_table_factory(&block_opts);
        }

        // NOTE: `compress` is a legacy shorthand for `compression_type`, it
        //   will get overriden if `compression_type` is also specified.
        if let Some(compress) = compress {
            db_options.set_compression_type(if *compress {
                DBCompressionType::Zstd
            } else {
                DBCompressionType::None
            });
        }
        if_some!(db_options.set_compression_type(compression_type));
        if let Some(compression_level) = compression_level {
            db_options.set_compression_options(
                -14,
                *compression_level,
                0,
                0,
            );
        }

        if_some!(db_options.set_wal_compression_type(wal_compression_type));
        if_some!(db_options.set_wal_ttl_seconds(wal_ttl_seconds));
        if_some!(db_options.set_wal_size_limit_mb(wal_size_limit_mb));
        if_some!(db_options.set_wal_bytes_per_sync(wal_bytes_per_sync));
        if_some!(db_options.set_wal_recovery_mode(wal_recovery_mode));

        if_some!(db_options.set_min_level_to_compress(min_level_to_compress));

        if_some!(db_options.set_level_zero_file_num_compaction_trigger(level_zero_file_num_compaction_trigger));
        if_some!(db_options.set_level_zero_slowdown_writes_trigger(level_zero_slowdown_writes_trigger));
        if_some!(db_options.set_level_zero_stop_writes_trigger(level_zero_stop_writes_trigger));

        if_some!(db_options.set_max_bytes_for_level_base(max_bytes_for_level_base));
        if_some!(db_options.set_max_bytes_for_level_multiplier(max_bytes_for_level_multiplier));
        if_some!(db_options.set_target_file_size_base(target_file_size_base));

        let mut max_background_jobs = *max_background_jobs;
        if max_background_jobs.is_none() {
            if let Some(max_flushes) = max_flushes {
                max_background_jobs = Some((max_subcompactions.unwrap_or(1) + max_flushes) as i32);
            }
        }
        if_some!(db_options.set_max_background_jobs(max_background_jobs.as_ref()));
        if_some!(db_options.set_max_subcompactions(max_subcompactions));

        if_some!(db_options.set_stats_dump_period_sec(stats_dump_period_sec));

        if_some!(db_options.increase_parallelism(parallelism));

        db_options
    }
}

impl crate::config::ConfigStoreKV {
    fn path(&self, collection_hash: StoreKVAtom) -> PathBuf {
        self.path.join(format!("{collection_hash:x}"))
    }
}

impl StoreGenericBuilder<StoreKVKey, StoreKV> for StoreKVBuilder {
    fn build(&self, pool_key: StoreKVKey) -> Result<StoreKV, ()> {
        match self.open(pool_key.collection_hash) {
            Ok(db) => {
                let now = SystemTime::now();

                Ok(StoreKV {
                    database: db,
                    last_used: RwLock::new(now),
                    last_flushed: RwLock::new(now),
                    lock: RwLock::new(()),
                    kv_store_config: Arc::clone(&self.kv_store_config),
                })
            }
            Err(err) => {
                tracing::error!("failed opening kv: {err}");

                Err(())
            }
        }
    }
}

impl StoreKV {
    fn flush(&self) -> Result<(), DBError> {
        // Generate flush options
        let mut flush_options = FlushOptions::default();

        flush_options.set_wait(true);

        // Perform flush (in blocking mode)
        self.database.flush_opt(&flush_options)
    }

    fn do_write(&self, batch: WriteBatch) -> Result<(), DBError> {
        // Configure this write
        let mut write_options = WriteOptions::default();

        // WAL disabled?
        if !self.kv_store_config.database.write_ahead_log {
            tracing::debug!("ignoring wal for kv write");

            write_options.disable_wal(true);
        } else {
            tracing::debug!("using wal for kv write");

            write_options.disable_wal(false);
        }

        // Commit this write
        self.database.write_opt(batch, &write_options)
    }
}

impl<'a> StoreKVActionReadWrite<'a> {
    pub fn write(&self, batch: WriteBatch) -> Result<(), DBError> {
        self.store.do_write(batch)
    }
}

impl StoreGeneric for StoreKV {
    fn ref_last_used(&self) -> &RwLock<SystemTime> {
        &self.last_used
    }
}

impl<'build> StoreKVActionBuilder<'build> {
    pub fn access_read_only<'a>(
        bucket: StoreItemPart<'a>,
        store: Arc<StoreKV>,
    ) -> StoreKVActionReadOnly<'a> {
        StoreKVActionReadOnly { bucket, store }
    }

    pub fn access_read_write<'a>(
        bucket: StoreItemPart<'a>,
        store: Arc<StoreKV>,
    ) -> StoreKVActionReadWrite<'a> {
        StoreKVActionReadWrite { bucket, store }
    }

    pub fn erase<T: AsRef<str>>(&self, collection: T, bucket: Option<T>) -> Result<u32, ()> {
        self.dispatch_erase("kv", collection, bucket)
    }
}

impl<'build> StoreGenericActionBuilder for StoreKVActionBuilder<'build> {
    fn proceed_erase_collection(&self, collection_str: &str) -> Result<u32, ()> {
        let collection_atom = StoreKeyerHasher::to_compact(collection_str);
        let collection_path = self.kv_pool.kv_store_config.path(collection_atom);

        // Force a KV store close
        self.kv_pool.close(collection_atom);

        if !collection_path.exists() {
            tracing::debug!(
                "kv collection store does not exist, consider already erased: {collection_str}/* at path: {collection_path:?}"
            );

            return Ok(0);
        }

        tracing::debug!(
            "kv collection store exists, erasing: {collection_str}/* at path: {collection_path:?}"
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

    fn proceed_erase_bucket(&self, _collection: &str, _bucket: &str) -> Result<u32, ()> {
        // This one is not implemented, as we need to acquire the collection; which would cause \
        //   a party-killer dead-lock.
        Err(())
    }
}

impl<'a> StoreKVActionReadOnly<'a> {
    /// Meta-to-Value mapper
    ///
    /// [IDX=0] ((meta)) ~> ((value))
    pub fn get_meta_to_value(&self, meta: StoreMetaKey) -> Result<Option<StoreMetaValue>, ()> {
        let store_key = StoreKeyerBuilder::meta_to_value(&self.bucket, &meta);

        tracing::debug!("store get meta-to-value: {store_key}");

        match self.store.database.get(&store_key.as_bytes()) {
            Ok(Some(value)) => {
                tracing::debug!("got meta-to-value: {store_key}");

                Ok(str::from_utf8(&value).map_or(None, |value| match meta {
                    StoreMetaKey::IIDIncr => value
                        .parse::<StoreObjectIID>()
                        .ok()
                        .map(StoreMetaValue::IIDIncr),
                }))
            }
            Ok(None) => {
                tracing::debug!("no meta-to-value found: {store_key}");

                Ok(None)
            }
            Err(err) => {
                tracing::error!("error getting meta-to-value: {store_key} with trace: {err}");

                Err(())
            }
        }
    }

    /// Term-to-IIDs mapper
    ///
    /// [IDX=1] ((term)) ~> [((iid))]
    pub fn get_term_to_iids(
        &self,
        term_hashed: StoreTermHashed,
    ) -> Result<Option<Vec<StoreObjectIID>>, ()> {
        let store_key = StoreKeyerBuilder::term_to_iids(&self.bucket, term_hashed);

        tracing::debug!("store get term-to-iids: {store_key}");

        match self.store.database.get(&store_key.as_bytes()) {
            Ok(Some(value)) => {
                tracing::debug!("got term-to-iids: {store_key} with encoded value: {value:?}");

                decode_u32_list(&value).map(|value_decoded| {
                    tracing::debug!(
                        "got term-to-iids: {store_key} with decoded value: {value_decoded:?}"
                    );

                    Some(value_decoded)
                })
            }
            Ok(None) => {
                tracing::debug!("no term-to-iids found: {store_key}");

                Ok(None)
            }
            Err(err) => {
                tracing::error!("error getting term-to-iids: {store_key} with trace: {err}");

                Err(())
            }
        }
    }

    /// OID-to-IID mapper
    ///
    /// [IDX=2] ((oid)) ~> ((iid))
    pub fn get_oid_to_iid(&self, oid: StoreObjectOID) -> Result<Option<StoreObjectIID>, ()> {
        let store_key = StoreKeyerBuilder::oid_to_iid(&self.bucket, oid);

        tracing::debug!("store get oid-to-iid: {store_key}");

        match self.store.database.get(&store_key.as_bytes()) {
            Ok(Some(value)) => {
                tracing::debug!("got oid-to-iid: {store_key} with encoded value: {value:?}");

                decode_u32(&value).map(|value_decoded| {
                    tracing::debug!(
                        "got oid-to-iid: {store_key} with decoded value: {value_decoded:?}"
                    );

                    Some(value_decoded)
                })
            }
            Ok(None) => {
                tracing::debug!("no oid-to-iid found: {store_key}");

                Ok(None)
            }
            Err(err) => {
                tracing::error!("error getting oid-to-iid: {store_key} with trace: {err}");

                Err(())
            }
        }
    }

    /// IID-to-OID mapper
    ///
    /// [IDX=3] ((iid)) ~> ((oid))
    pub fn get_iid_to_oid(&self, iid: StoreObjectIID) -> Result<Option<String>, ()> {
        let store_key = StoreKeyerBuilder::iid_to_oid(&self.bucket, iid);

        tracing::debug!("store get iid-to-oid: {store_key}");

        match self.store.database.get(&store_key.as_bytes()) {
            Ok(Some(value)) => {
                tracing::debug!("got iid-to-oid: {store_key}");

                Ok(str::from_utf8(&value).ok().map(str::to_string))
            }
            Ok(None) => {
                tracing::debug!("no iid-to-oid found: {store_key}");

                Ok(None)
            }
            Err(err) => {
                tracing::error!("error getting iid-to-oid: {store_key} with trace: {err}");

                Err(())
            }
        }
    }

    /// IID-to-Terms mapper
    ///
    /// [IDX=4] ((iid)) ~> [((term))]
    pub fn get_iid_to_terms(
        &self,
        iid: StoreObjectIID,
    ) -> Result<Option<Vec<StoreTermHashed>>, ()> {
        let store_key = StoreKeyerBuilder::iid_to_terms(&self.bucket, iid);

        tracing::debug!("store get iid-to-terms: {store_key}");

        match self.store.database.get(&store_key.as_bytes()) {
            Ok(Some(value)) => {
                tracing::debug!("got iid-to-terms: {store_key} with encoded value: {value:?}");

                decode_u32_list(&value).map(|value_decoded| {
                    tracing::debug!(
                        "got iid-to-terms: {store_key} with decoded value: {value_decoded:?}"
                    );

                    // TODO: Do not map empty to `None`, as it has a different
                    //   meaning. Let handlers do what they want. Also this
                    //   creates a discrepancy with `get_term_to_iids`.
                    if !value_decoded.is_empty() {
                        Some(value_decoded)
                    } else {
                        None
                    }
                })
            }
            Ok(None) => {
                tracing::debug!("no iid-to-terms found: {store_key}");

                Ok(None)
            }
            Err(err) => {
                tracing::error!("error getting iid-to-terms: {store_key} with trace: {err}");

                Err(())
            }
        }
    }
}

impl<'a> StoreKVActionReadWrite<'a> {
    /// This is `O(1)`, nothing meaningful happens.
    fn to_read_only<'b>(&'b self) -> StoreKVActionReadOnly<'b> {
        StoreKVActionReadOnly {
            bucket: self.bucket,
            store: Arc::clone(&self.store),
        }
    }

    /// Meta-to-Value mapper
    ///
    /// [IDX=0] ((meta)) ~> ((value))
    pub fn get_meta_to_value(&self, meta: StoreMetaKey) -> Result<Option<StoreMetaValue>, ()> {
        self.to_read_only().get_meta_to_value(meta)
    }

    pub fn set_meta_to_value(
        &self,
        batch: &mut WriteBatch,
        meta: StoreMetaKey,
        value: StoreMetaValue,
    ) {
        let store_key = StoreKeyerBuilder::meta_to_value(&self.bucket, &meta);

        tracing::debug!("store set meta-to-value: {store_key}");

        let value_string = match value {
            StoreMetaValue::IIDIncr(iid_incr) => iid_incr.to_string(),
        };

        batch.put(&store_key.as_bytes(), value_string.as_bytes())
    }

    // TODO: Make this really atomic by using a `merge` command.
    /// Atomically(ish) increments the `IIDIncr` counter and returns the new
    /// value.
    pub fn auto_increment_iid(
        &self,
        guard: Option<RwLockWriteGuard<()>>,
    ) -> Result<StoreObjectIID, Box<dyn std::error::Error>> {
        // SAFETY: Lock the database in exclusive access, to ensure IID
        //   increments are atomic. See <https://github.com/valeriansaliou/sonic/issues/389>
        //   for more information about why this is important.
        let _guard = guard.unwrap_or_else(|| self.store.lock.write().unwrap());

        let Ok(iid_incr_opt) = self.get_meta_to_value(StoreMetaKey::IIDIncr) else {
            return Err("failed getting push executor meta-to-value iid increment".into());
        };

        let iid_incr = iid_incr_opt.map_or(0, |meta_val| match meta_val {
            StoreMetaValue::IIDIncr(iid_incr) => iid_incr + 1,
        });

        let mut batch = WriteBatch::default();

        // Bump last stored increment
        self.set_meta_to_value(
            &mut batch,
            StoreMetaKey::IIDIncr,
            StoreMetaValue::IIDIncr(iid_incr),
        );

        match self.write(batch) {
            Ok(()) => Ok(iid_incr),
            Err(err) => Err(Box::from(format!(
                "failed updating push executor meta-to-value iid increment: {err}"
            ))),
        }
    }

    /// Term-to-IIDs mapper
    ///
    /// [IDX=1] ((term)) ~> [((iid))]
    #[inline]
    pub fn get_term_to_iids(
        &self,
        term_hashed: StoreTermHashed,
    ) -> Result<Option<Vec<StoreObjectIID>>, ()> {
        self.to_read_only().get_term_to_iids(term_hashed)
    }

    // TODO(pref): Update merge operator to support deletion and get rid of this.
    pub fn set_term_to_iids(
        &self,
        batch: &mut WriteBatch,
        term_hashed: StoreTermHashed,
        iids: impl ExactSizeIterator<Item = StoreObjectIID>,
    ) {
        let store_key = StoreKeyerBuilder::term_to_iids(&self.bucket, term_hashed);

        tracing::debug!("store set term-to-iids: {store_key}");

        // Encode IID list into storage serialized format
        let iids_encoded = encode_u32_list(iids);

        tracing::debug!("store set term-to-iids: {store_key} with encoded value: {iids_encoded:?}");

        batch.put(&store_key.as_bytes(), &iids_encoded)
    }

    pub fn add_term_to_iids(
        &self,
        batch: &mut WriteBatch,
        term_hashed: StoreTermHashed,
        iids: impl Iterator<Item = StoreObjectIID>,
    ) {
        let store_key = StoreKeyerBuilder::term_to_iids(&self.bucket, term_hashed);

        tracing::debug!("store add term-to-iids: {store_key}");

        for iid in iids {
            batch.merge(&store_key.as_bytes(), encode_u32(iid));
        }
    }

    pub fn delete_term_to_iids(&self, batch: &mut WriteBatch, term_hashed: StoreTermHashed) {
        let store_key = StoreKeyerBuilder::term_to_iids(&self.bucket, term_hashed);

        tracing::debug!("store delete term-to-iids: {store_key}");

        batch.delete(&store_key.as_bytes())
    }

    /// OID-to-IID mapper
    ///
    /// [IDX=2] ((oid)) ~> ((iid))
    pub fn get_oid_to_iid(&self, oid: StoreObjectOID) -> Result<Option<StoreObjectIID>, ()> {
        self.to_read_only().get_oid_to_iid(oid)
    }

    pub fn set_oid_to_iid(&self, batch: &mut WriteBatch, oid: StoreObjectOID, iid: StoreObjectIID) {
        let store_key = StoreKeyerBuilder::oid_to_iid(&self.bucket, oid);

        tracing::debug!("store set oid-to-iid: {store_key}");

        // Encode IID
        let iid_encoded = encode_u32(iid);

        tracing::debug!("store set oid-to-iid: {store_key} with encoded value: {iid_encoded:?}");

        batch.put(&store_key.as_bytes(), &iid_encoded)
    }

    pub fn delete_oid_to_iid(&self, batch: &mut WriteBatch, oid: StoreObjectOID) {
        let store_key = StoreKeyerBuilder::oid_to_iid(&self.bucket, oid);

        tracing::debug!("store delete oid-to-iid: {store_key}");

        batch.delete(&store_key.as_bytes())
    }

    /// IID-to-OID mapper
    ///
    /// [IDX=3] ((iid)) ~> ((oid))
    pub fn get_iid_to_oid(&self, iid: StoreObjectIID) -> Result<Option<String>, ()> {
        self.to_read_only().get_iid_to_oid(iid)
    }

    pub fn set_iid_to_oid(&self, batch: &mut WriteBatch, iid: StoreObjectIID, oid: StoreObjectOID) {
        let store_key = StoreKeyerBuilder::iid_to_oid(&self.bucket, iid);

        tracing::debug!("store set iid-to-oid: {store_key}");

        batch.put(&store_key.as_bytes(), oid.as_bytes())
    }

    pub fn delete_iid_to_oid(&self, batch: &mut WriteBatch, iid: StoreObjectIID) {
        let store_key = StoreKeyerBuilder::iid_to_oid(&self.bucket, iid);

        tracing::debug!("store delete iid-to-oid: {store_key}");

        batch.delete(&store_key.as_bytes())
    }

    /// IID-to-Terms mapper
    ///
    /// [IDX=4] ((iid)) ~> [((term))]
    pub fn get_iid_to_terms(
        &self,
        iid: StoreObjectIID,
    ) -> Result<Option<Vec<StoreTermHashed>>, ()> {
        self.to_read_only().get_iid_to_terms(iid)
    }

    pub fn set_iid_to_terms(
        &self,
        batch: &mut WriteBatch,
        iid: StoreObjectIID,
        terms_hashed: impl ExactSizeIterator<Item = u32>,
    ) {
        let store_key = StoreKeyerBuilder::iid_to_terms(&self.bucket, iid);

        tracing::debug!("store set iid-to-terms: {store_key}");

        // Encode term list into storage serialized format
        let terms_hashed_encoded = encode_u32_list(terms_hashed);

        tracing::debug!(
            "store set iid-to-terms: {store_key} with encoded value: {terms_hashed_encoded:?}"
        );

        batch.put(&store_key.as_bytes(), &terms_hashed_encoded)
    }

    pub fn add_iid_to_terms(
        &self,
        batch: &mut WriteBatch,
        iid: StoreObjectIID,
        terms_hashed: impl Iterator<Item = u32>,
    ) {
        let store_key = StoreKeyerBuilder::iid_to_terms(&self.bucket, iid);

        tracing::debug!("store add iid-to-terms: {store_key}");

        for term_hash in terms_hashed {
            batch.merge(&store_key.as_bytes(), encode_u32(term_hash));
        }
    }

    pub fn delete_iid_to_terms(&self, batch: &mut WriteBatch, iid: StoreObjectIID) {
        let store_key = StoreKeyerBuilder::iid_to_terms(&self.bucket, iid);

        tracing::debug!("store delete iid-to-terms: {store_key}");

        batch.delete(&store_key.as_bytes())
    }

    pub fn batch_flush_bucket(
        &self,
        batch: &mut WriteBatch,
        iid: StoreObjectIID,
        oid: StoreObjectOID,
        iid_terms_hashed: &[StoreTermHashed],
    ) -> u32 {
        let mut count = 0;

        tracing::debug!("store batch flush bucket: {iid} with hashed terms: {iid_terms_hashed:?}");

        // Delete OID <> IID association
        self.delete_oid_to_iid(batch, oid);
        self.delete_iid_to_oid(batch, iid);
        self.delete_iid_to_terms(batch, iid);

        // Delete IID from each associated term
        for iid_term in iid_terms_hashed {
            let Ok(Some(mut iid_term_iids)) = self.get_term_to_iids(*iid_term) else {
                continue;
            };

            if iid_term_iids.contains(&iid) {
                count += 1;

                // Remove IID from list of IIDs
                iid_term_iids.retain(|&cur_iid| cur_iid != iid);
            }

            if iid_term_iids.is_empty() {
                self.delete_term_to_iids(batch, *iid_term)
            } else {
                self.set_term_to_iids(batch, *iid_term, iid_term_iids.into_iter())
            };
        }

        count
    }

    pub fn batch_erase_bucket(&self) -> Result<u32, ()> {
        let bucket = self.bucket.as_str();

        // Generate all key prefix values (with dummy post-prefix values; we dont care)
        let (k_meta_to_value, k_term_to_iids, k_oid_to_iid, k_iid_to_oid, k_iid_to_terms) = (
            StoreKeyerBuilder::meta_to_value(bucket, &StoreMetaKey::IIDIncr),
            StoreKeyerBuilder::term_to_iids(bucket, 0),
            StoreKeyerBuilder::oid_to_iid(bucket, ""),
            StoreKeyerBuilder::iid_to_oid(bucket, 0),
            StoreKeyerBuilder::iid_to_terms(bucket, 0),
        );

        let key_prefixes: [StoreKeyerPrefix; 5] = [
            k_meta_to_value.as_prefix(),
            k_term_to_iids.as_prefix(),
            k_oid_to_iid.as_prefix(),
            k_iid_to_oid.as_prefix(),
            k_iid_to_terms.as_prefix(),
        ];

        // Scan all keys per-prefix and nuke them right away
        for key_prefix in &key_prefixes {
            tracing::debug!("store batch erase bucket: {bucket} for prefix: {key_prefix:?}");

            // Generate start and end prefix for batch delete (in other words,
            // the minimum key value possible, and the highest key value possible)
            let key_prefix_start: StoreKeyerKey = [
                key_prefix[0],
                key_prefix[1],
                key_prefix[2],
                key_prefix[3],
                key_prefix[4],
                0,
                0,
                0,
                0,
            ];
            let key_prefix_end: StoreKeyerKey = [
                key_prefix[0],
                key_prefix[1],
                key_prefix[2],
                key_prefix[3],
                key_prefix[4],
                255,
                255,
                255,
                255,
            ];

            // TODO: Move the batch outside the for loop?
            let mut batch = WriteBatch::default();

            // Batch-delete keys matching range
            batch.delete_range(&key_prefix_start, &key_prefix_end);

            // Ensure last key is deleted (as RocksDB end key is exclusive;
            // while start key is inclusive, we need to ensure the end-of-range
            // key is deleted)
            batch.delete(&key_prefix_end);

            // Commit operation to database
            if let Err(err) = self.write(batch) {
                tracing::error!("failed in store batch erase bucket: {bucket} with error: {err}");
                continue;
            }

            tracing::debug!("succeeded in store batch erase bucket: {bucket}");
        }

        tracing::info!("done processing store batch erase bucket: {bucket}");

        Ok(1)
    }
}

fn encode_u32(decoded: u32) -> [u8; 4] {
    let mut encoded = [0; 4];

    LittleEndian::write_u32(&mut encoded, decoded);

    encoded
}

fn decode_u32(encoded: &[u8]) -> Result<u32, ()> {
    Cursor::new(encoded).read_u32::<LittleEndian>().or(Err(()))
}

fn encode_u32_list(decoded: impl ExactSizeIterator<Item = u32>) -> Vec<u8> {
    // Pre-reserve required capacity as to avoid heap resizes (50%
    // performance gain relative to initializing this with a zero-capacity)
    let mut encoded = Vec::with_capacity(decoded.len() * 4);

    for decoded_item in decoded {
        encoded.extend(&encode_u32(decoded_item))
    }

    encoded
}

fn decode_u32_list(encoded: &[u8]) -> Result<Vec<u32>, ()> {
    // Pre-reserve required capacity as to avoid heap resizes (50%
    // performance gain relative to initializing this with a zero-capacity)
    let mut decoded = Vec::with_capacity(encoded.len() / 4);

    for encoded_chunk in encoded.chunks(4) {
        match decode_u32(encoded_chunk) {
            Ok(decoded_chunk) => {
                decoded.push(decoded_chunk);
            }
            Err(_err) => return Err(()),
        }
    }

    Ok(decoded)
}

fn default_merge_operator(
    key: &[u8],
    existing_val: Option<&[u8]>,
    operands: &MergeOperands,
) -> Option<Vec<u8>> {
    match key[0] {
        // StoreKeyerIdx::TermToIIDs | StoreKeyerIdx::IIDToTerms
        1 | 4 => {
            // eprintln!(
            //     "prepend_u32_list({}): {}/{}",
            //     &key[0],
            //     existing_val.map_or(0, <[u8]>::len),
            //     operands.len()
            // );
            prepend_u32_list(existing_val, operands)
        }
        _ => unreachable!(),
    }
}

/// This efficiently prepends new u32 values to an existing slice, removing
/// duplicates along the way.
fn prepend_u32_list(existing_val: Option<&[u8]>, operands: &MergeOperands) -> Option<Vec<u8>> {
    const WORD_LEN: usize = 4;

    let current: &[u8] = existing_val.unwrap_or_default();

    let operands_total_len = operands.iter().fold(0, |acc, op| acc + op.len());

    let mut res: Vec<u8> = Vec::with_capacity(current.len() + operands_total_len);

    // PERF: This is just a fancy way to preprend without extra allocation nor
    //   reverse iteration.
    let mut cursor = operands_total_len;
    res.extend_from_slice(vec![0; cursor].as_slice());

    // TODO(perf): We might be able to make this a tiny bit faster by using a
    //   custom hasher that only maps `&[u8]` to a `u32`. When there is a high
    //   chance that values are close to each other (e.g. for IIDs), we could
    //   use `% capacity` to spread the values better. BENCHMARK THIS ANYWAY!
    let mut seen: HashSet<&[u8]> = HashSet::with_capacity(operands_total_len / WORD_LEN);

    for op in operands {
        for chunk in op.chunks(WORD_LEN) {
            // Filter duplicate operands.
            // NOTE: In benchmarks, `operands` showed a length of `13761` for
            //   example, so we _have_ to keep this at most `O(n*log(n))`!
            if seen.insert(chunk) {
                let start = cursor.checked_sub(WORD_LEN).unwrap();
                res[start..cursor].copy_from_slice(chunk);
                cursor = start;
            }
        }
    }

    // Trim unused bytes at the start (because of duplicate operands).
    res = res.split_off(cursor);

    for existing in current.chunks(WORD_LEN) {
        // Skip already inserted operands.
        // See reason in <https://github.com/valeriansaliou/sonic/issues/389#issuecomment-5374968203>.
        if !seen.contains(existing) {
            res.extend_from_slice(existing);
        }
    }

    assert!(!res.is_empty());

    Some(res)
}

impl StoreKVKey {
    pub fn from_atom(collection_hash: StoreKVAtom) -> StoreKVKey {
        StoreKVKey { collection_hash }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(collection_str: &str) -> StoreKVKey {
        StoreKVKey {
            collection_hash: StoreKeyerHasher::to_compact(collection_str),
        }
    }
}

impl fmt::Display for StoreKVKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<{:x}>", self.collection_hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_acquires_database() {
        let kv_store_config = test_kv_store_config();
        let kv_pool = StoreKVPool::new(kv_store_config);

        assert!(kv_pool.acquire(StoreKVAcquireMode::Any, "c:test:1").is_ok());
    }

    #[test]
    fn it_janitors_database() {
        let kv_store_config = test_kv_store_config();
        let kv_pool = StoreKVPool::new(kv_store_config);

        kv_pool.janitor();
    }

    #[test]
    fn it_proceeds_actions() {
        let kv_store_config = test_kv_store_config();
        let kv_pool = StoreKVPool::new(kv_store_config);

        let store = kv_pool
            .acquire(StoreKVAcquireMode::Any, "c:test:3")
            .unwrap()
            .unwrap();
        let action = StoreKVActionBuilder::access_read_write(
            StoreItemPart::from_str("b:test:3").unwrap(),
            store,
        );

        assert!(action.get_meta_to_value(StoreMetaKey::IIDIncr).is_ok());
        assert!({
            let mut batch = WriteBatch::default();
            action.set_meta_to_value(
                &mut batch,
                StoreMetaKey::IIDIncr,
                StoreMetaValue::IIDIncr(1),
            );
            action.write(batch).is_ok()
        });

        assert!(action.get_term_to_iids(1).is_ok());
        assert!({
            let mut batch = WriteBatch::default();
            action.set_term_to_iids(&mut batch, 1, [0, 1, 2].into_iter());
            action.write(batch).is_ok()
        });
        assert!({
            let mut batch = WriteBatch::default();
            action.delete_term_to_iids(&mut batch, 1);
            action.write(batch).is_ok()
        });

        assert!(action.get_oid_to_iid(&"s".to_string()).is_ok());
        assert!({
            let mut batch = WriteBatch::default();
            action.set_oid_to_iid(&mut batch, &"s".to_string(), 4);
            action.write(batch).is_ok()
        });
        assert!({
            let mut batch = WriteBatch::default();
            action.delete_oid_to_iid(&mut batch, &"s".to_string());
            action.write(batch).is_ok()
        });

        assert!(action.get_iid_to_oid(4).is_ok());
        assert!({
            let mut batch = WriteBatch::default();
            action.set_iid_to_oid(&mut batch, 4, &"s".to_string());
            action.write(batch).is_ok()
        });
        assert!({
            let mut batch = WriteBatch::default();
            action.delete_iid_to_oid(&mut batch, 4);
            action.write(batch).is_ok()
        });

        assert!(action.get_iid_to_terms(4).is_ok());
        assert!({
            let mut batch = WriteBatch::default();
            action.set_iid_to_terms(&mut batch, 4, [45402].into_iter());
            action.write(batch).is_ok()
        });
        assert!({
            let mut batch = WriteBatch::default();
            action.delete_iid_to_terms(&mut batch, 4);
            action.write(batch).is_ok()
        });
    }

    #[test]
    fn it_encodes_atom() {
        assert_eq!(encode_u32(0), [0, 0, 0, 0]);
        assert_eq!(encode_u32(1), [1, 0, 0, 0]);
        assert_eq!(encode_u32(45402), [90, 177, 0, 0]);
    }

    #[test]
    fn it_decodes_atom() {
        assert_eq!(decode_u32(&[0, 0, 0, 0]), Ok(0));
        assert_eq!(decode_u32(&[1, 0, 0, 0]), Ok(1));
        assert_eq!(decode_u32(&[90, 177, 0, 0]), Ok(45402));
    }

    #[test]
    fn it_encodes_atom_list() {
        assert_eq!(
            encode_u32_list([0, 2, 3].into_iter()),
            [0, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0]
        );
        assert_eq!(encode_u32_list([45402].into_iter()), [90, 177, 0, 0]);
    }

    #[test]
    fn it_decodes_atom_list() {
        assert_eq!(
            decode_u32_list(&[0, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0]),
            Ok(vec![0, 2, 3])
        );
        assert_eq!(decode_u32_list(&[90, 177, 0, 0]), Ok(vec![45402]));
    }

    fn test_kv_store_config() -> Arc<crate::config::ConfigStoreKV> {
        Arc::new(
            config::Config::builder()
                .add_source(config::File::from_str(
                    crate::config::tests::defaults_toml(),
                    config::FileFormat::Toml,
                ))
                .build()
                .unwrap()
                .get::<crate::config::ConfigStoreKV>("store.kv")
                .unwrap(),
        )
    }
}

#[cfg(all(feature = "benchmark", test))]
mod benches {
    extern crate test;

    use super::*;
    use test::Bencher;

    #[bench]
    fn bench_encode_atom(b: &mut Bencher) {
        b.iter(|| StoreKVAction::encode_u32(0));
    }

    #[bench]
    fn bench_decode_atom(b: &mut Bencher) {
        let encoded_atom = [0, 0, 0, 0];

        b.iter(|| StoreKVAction::decode_u32(&encoded_atom));
    }

    #[bench]
    fn bench_encode_atom_list(b: &mut Bencher) {
        let atom_list = [0, 2, 3];

        b.iter(|| StoreKVAction::encode_u32_list(&atom_list));
    }

    #[bench]
    fn bench_decode_atom_list(b: &mut Bencher) {
        let encoded_atom_list = [0, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0];

        b.iter(|| StoreKVAction::decode_u32_list(&encoded_atom_list));
    }
}

// MARK: - Boilerplate

impl fmt::Debug for StoreKVPool {
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

        f.debug_struct("StoreKVPool")
            .field("pool", &AsPrettyRwLock(pool))
            .field("store_access_lock", &AsPrettyRwLock(store_access_lock))
            .field("store_acquire_lock", &AsPrettyMutex(store_acquire_lock))
            .field("store_flush_lock", &AsPrettyMutex(store_flush_lock))
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for StoreKVKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self, f)
    }
}

impl fmt::Debug for StoreKV {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use crate::util::fmt::AsPrettyRwLock;

        // NOTE: Deconstructing to future-proof this function.
        let Self {
            database,
            last_used,
            last_flushed,
            lock,
            // NOTE: We don’t care about the configuration,
            //   we can see it elsewhere if needed.
            kv_store_config: _kv_store_config,
        } = self;

        f.debug_struct("StoreKV")
            .field("database", database)
            .field("last_used", &AsPrettyRwLock(last_used))
            .field("last_flushed", &AsPrettyRwLock(last_flushed))
            .field("lock", &AsPrettyRwLock(lock))
            .finish_non_exhaustive()
    }
}
