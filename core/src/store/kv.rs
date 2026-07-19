// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use byteorder::{ByteOrder, LittleEndian, ReadBytesExt};
use hashbrown::{HashMap, HashSet};
use radix::RadixNum;
use rocksdb::backup::{
    BackupEngine as DBBackupEngine, BackupEngineOptions as DBBackupEngineOptions,
    RestoreOptions as DBRestoreOptions,
};
use rocksdb::{
    BlockBasedOptions, ColumnFamilyDescriptor, DB, DBCompactionStyle, DBCompressionType, Direction,
    Env as DBEnv, Error as DBError, FlushOptions, IteratorMode, Options as DBOptions, WriteBatch,
    WriteBatchIteratorCf, WriteOptions,
};
use std::fmt;
use std::fs;
use std::io::{self, BufWriter, Cursor, Write};
use std::path::{Path, PathBuf};
use std::str;
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use super::document::{
    StoreDocument, StoreDocumentRecord, StoreFreshBatchResult, StoreFreshBatchTimings,
    StoreKVHealth, TIME_SLICE_MS,
};
use super::generic::{
    StoreGeneric, StoreGenericActionBuilder, StoreGenericBuilder, StoreGenericPool,
};
use super::identifiers::*;
use super::item::StoreItemPart;
use super::keyer::{StoreKeyerBuilder, StoreKeyerHasher, StoreKeyerPrefix};
use super::posting::StorePosting;
use super::stats::{
    StoreCollectionStats, StoreColumnFamilyStats, StoreIndexFamilyStats, StoreLogicalStats,
    StorePostingStats,
};

// NOTE: This type cannot be generic over a lifetime as spawning threads would
//   force it to be `'static`.
#[derive(Clone)]
pub struct StoreKVPool {
    pool: Arc<RwLock<HashMap<StoreKVKey, StoreKVBox>>>,
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
    last_used: Arc<RwLock<SystemTime>>,
    last_flushed: Arc<RwLock<SystemTime>>,
    pub lock: RwLock<bool>,
    kv_store_config: Arc<crate::config::ConfigStoreKV>,
}

pub struct StoreKVActionBuilder<'build> {
    pub kv_pool: &'build StoreKVPool,
}

pub struct StoreKVAction<'a> {
    store: Option<StoreKVBox>,
    bucket: StoreItemPart<'a>,
    bucket_id: Option<StoreBucketID>,
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
type StoreKVBox = Arc<StoreKV>;

const ATOM_HASH_RADIX: usize = 16;
const STORE_SCHEMA_VERSION: u32 = 13;
const DOCUMENTS_CF: &str = "documents";
const POSTINGS_CF: &str = "postings";
const DEFAULT_CF_ID: u32 = 0;
const DOCUMENTS_CF_ID: u32 = 1;
const POSTINGS_CF_ID: u32 = 2;

#[derive(Default)]
struct StoreWriteProfile {
    wal_nanos: u64,
    memtable_nanos: u64,
    delay_nanos: u64,
    pre_and_post_nanos: u64,
    db_mutex_nanos: u64,
    db_condition_wait_nanos: u64,
    merge_operator_nanos: u64,
    batch: StoreWriteBatchProfile,
}

#[derive(Default)]
struct StoreWriteBatchProfile {
    bytes: [u64; 3],
    puts: u64,
    deletes: u64,
    merges: u64,
}

impl StoreWriteBatchProfile {
    fn add_bytes(&mut self, cf_id: u32, bytes: usize) {
        if let Some(total) = self.bytes.get_mut(cf_id as usize) {
            *total += bytes as u64;
        }
    }
}

impl WriteBatchIteratorCf for StoreWriteBatchProfile {
    fn put_cf(&mut self, cf_id: u32, key: &[u8], value: &[u8]) {
        self.add_bytes(cf_id, key.len() + value.len());
        self.puts += 1;
    }

    fn delete_cf(&mut self, cf_id: u32, key: &[u8]) {
        self.add_bytes(cf_id, key.len());
        self.deletes += 1;
    }

    fn merge_cf(&mut self, cf_id: u32, key: &[u8], value: &[u8]) {
        self.add_bytes(cf_id, key.len() + value.len());
        self.merges += 1;
    }
}

fn profile_started(enabled: bool) -> Option<Instant> {
    enabled.then(Instant::now)
}

fn profile_elapsed(started: Option<Instant>) -> Duration {
    started.map(|started| started.elapsed()).unwrap_or_default()
}

fn merge_postings(
    _key: &[u8],
    existing: Option<&[u8]>,
    operands: &rocksdb::MergeOperands,
) -> Option<Vec<u8>> {
    let mut merged = existing
        .map(StorePosting::decode)
        .transpose()
        .ok()?
        .unwrap_or_default();
    for operand in operands {
        merged.union_with(&StorePosting::decode(operand).ok()?);
    }
    Some(merged.encode())
}

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
    ) -> Result<Option<StoreKVBox>, ()> {
        let collection = collection.as_ref();
        let pool_key = StoreKVKey::from_str(collection);

        // Freeze acquire lock, and reference it in context
        // Notice: this prevents two databases on the same collection to be opened at the same time.
        let _acquire = self.store_acquire_lock.lock().unwrap();

        // Acquire a thread-safe store pool reference in read mode
        let store_pool_read = self.pool.read().unwrap();

        if let Some(store_kv) = store_pool_read.get(&pool_key) {
            Self::proceed_acquire_cache("kv", collection, pool_key, store_kv).map(Some)
        } else {
            tracing::info!(
                "kv store not in pool for collection: {} {}, opening it",
                collection,
                pool_key
            );

            // Important: we need to drop the read reference first, to avoid \
            //   dead-locking when acquiring the RWLock in write mode in this block.
            drop(store_pool_read);

            // Check if can open database?
            let can_open_db = if mode == StoreKVAcquireMode::OpenOnly {
                self.kv_store_config.path(pool_key.collection_hash).exists()
            } else {
                true
            };

            let builder = StoreKVBuilder {
                kv_store_config: Arc::clone(&self.kv_store_config),
            };

            // Open KV database? (ie. we do not need to create a new KV database file tree if \
            //   the database does not exist yet on disk and we are just looking to read data from \
            //   it)
            if can_open_db {
                Self::proceed_acquire_open("kv", collection, pool_key, &self.pool, &builder)
                    .map(Some)
            } else {
                Ok(None)
            }
        }
    }

    fn close(&self, collection_hash: StoreKVAtom) {
        tracing::debug!(
            "closing key-value database for collection: <{:x}>",
            collection_hash
        );

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
        tracing::debug!("backing up all kv stores to path: {:?}", path);

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
        tracing::debug!("restoring all kv stores from path: {:?}", path);

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

        {
            let store_pool_read = self.pool.read().unwrap();

            for (key, store) in &*store_pool_read {
                // Important: be lenient with system clock going back to a past duration, since \
                //   we may be running in a virtualized environment where clock is not guaranteed \
                //   to be monotonic. This is done to avoid poisoning associated mutexes by \
                //   crashing on unwrap().
                let not_flushed_for = store
                    .last_flushed
                    .read()
                    .unwrap()
                    .elapsed()
                    .unwrap_or_else(|err| {
                        tracing::error!(
                            "kv key: {} last flush duration clock issue, zeroing: {}",
                            key,
                            err
                        );

                        // Assuming a zero seconds fallback duration
                        Duration::from_secs(0)
                    })
                    .as_secs();

                if force || not_flushed_for >= self.kv_store_config.database.flush_after {
                    tracing::info!(
                        "kv key: {} not flushed for: {} seconds, may flush",
                        key,
                        not_flushed_for
                    );

                    keys_flush.push(*key);
                } else {
                    tracing::debug!(
                        "kv key: {} not flushed for: {} seconds, no flush",
                        key,
                        not_flushed_for
                    );
                }
            }
        }

        // Exit trap: Nothing to flush yet? Abort there.
        if keys_flush.is_empty() {
            tracing::info!("no kv store pool items need to be flushed at the moment");

            return;
        }

        // Step 2: Flush KVs, one-by-one (sequential locking; this avoids global locks)
        let mut count_flushed = 0;

        {
            for key in &keys_flush {
                {
                    // Prevent destructive pool operations without blocking ordinary access.
                    let _access = self.store_access_lock.read().unwrap();

                    if let Some(store) = self.pool.read().unwrap().get(key) {
                        tracing::debug!("kv key: {} flush started", key);

                        if let Err(err) = store.flush() {
                            tracing::error!("kv key: {} flush failed: {}", key, err);
                        } else {
                            count_flushed += 1;

                            tracing::debug!("kv key: {} flush complete", key);
                        }

                        // Bump 'last flushed' time
                        *store.last_flushed.write().unwrap() = SystemTime::now();
                    }
                }

                // Give a bit of time to other threads before continuing
                thread::yield_now();
            }
        }

        tracing::info!(
            "done scanning for kv store pool items to flush to disk (flushed: {})",
            count_flushed
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
        // Iterate on KV collections
        for collection in fs::read_dir(read_path)? {
            let collection = collection?;

            // Actual collection found?
            if let (Ok(collection_file_type), Some(collection_name)) =
                (collection.file_type(), collection.file_name().to_str())
            {
                if collection_file_type.is_dir() {
                    tracing::debug!("kv collection ongoing {}: {}", action, collection_name);

                    fn_item(self, write_path, &collection.path(), collection_name)?;
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
    ) -> Result<(), io::Error> {
        // Acquire access lock (in blocking write mode), and reference it in context
        // Notice: this prevents store to be acquired from any context
        let _access = self.store_access_lock.write().unwrap();

        // Generate path to KV backup
        let kv_backup_path = backup_path.join(collection_name);

        tracing::debug!(
            "kv collection: {} backing up to path: {:?}",
            collection_name,
            kv_backup_path
        );

        // Erase any previously-existing KV backup
        if kv_backup_path.exists() {
            fs::remove_dir_all(&kv_backup_path)?;
        }

        // Create backup folder for collection
        fs::create_dir_all(backup_path.join(collection_name))?;

        // Convert names to hashes (as names are hashes encoded as base-16 strings, but we need \
        //   them as proper integers)
        if let Ok(collection_radix) = RadixNum::from_str(collection_name, ATOM_HASH_RADIX) {
            if let Ok(collection_hash) = collection_radix.as_decimal() {
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

                let mut kv_backup_engine =
                    DBBackupEngine::open(&kv_backup_options, &kv_backup_environment)
                        .map_err(|_| io::Error::other("backup engine failure"))?;

                // Proceed actual KV database backup
                kv_backup_engine
                    .create_new_backup(&origin_kv)
                    .map_err(|_| io::Error::other("database backup failure"))?;

                tracing::info!(
                    "kv collection: {} backed up to path: {:?}",
                    collection_name,
                    kv_backup_path
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
    ) -> Result<(), io::Error> {
        // Acquire access lock (in blocking write mode), and reference it in context
        // Notice: this prevents store to be acquired from any context
        let _access = self.store_access_lock.write().unwrap();

        tracing::debug!(
            "kv collection: {} restoring from path: {:?}",
            collection_name,
            origin_path
        );

        // Convert names to hashes (as names are hashes encoded as base-16 strings, but we need \
        //   them as proper integers)
        if let Ok(collection_radix) = RadixNum::from_str(collection_name, ATOM_HASH_RADIX) {
            if let Ok(collection_hash) = collection_radix.as_decimal() {
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

                let mut kv_backup_engine =
                    DBBackupEngine::open(&kv_backup_options, &kv_backup_environment)
                        .map_err(|_| io::Error::other("backup engine failure"))?;

                kv_backup_engine
                    .restore_from_latest_backup(&kv_path, &kv_path, &DBRestoreOptions::default())
                    .map_err(|_| io::Error::other("database restore failure"))?;

                tracing::info!(
                    "kv collection: {} restored to path: {:?} from backup: {:?}",
                    collection_name,
                    kv_path,
                    origin_path
                );
            }
        }

        Ok(())
    }
}

impl StoreGenericPool<StoreKVKey, StoreKV, StoreKVBuilder> for StoreKVPool {}

impl StoreKVBuilder {
    fn open(&self, collection_hash: StoreKVAtom) -> Result<DB, DBError> {
        tracing::debug!(
            "opening key-value database for collection: <{:x}>",
            collection_hash
        );

        // Configure database options
        let db_options = self.configure();

        let documents_options = self.configure();
        let mut postings_options = self.configure();
        postings_options.set_merge_operator_associative("posting_union", merge_postings);
        DB::open_cf_descriptors(
            &db_options,
            self.kv_store_config.path(collection_hash),
            [
                ColumnFamilyDescriptor::new("default", db_options.clone()),
                ColumnFamilyDescriptor::new(DOCUMENTS_CF, documents_options),
                ColumnFamilyDescriptor::new(POSTINGS_CF, postings_options),
            ],
        )
    }

    fn configure(&self) -> DBOptions {
        tracing::debug!("configuring key-value database");

        let db_conf = &self.kv_store_config.database;

        // Make database options
        let mut db_options = DBOptions::default();

        // Set static options
        db_options.create_if_missing(true);
        db_options.create_missing_column_families(true);
        db_options.set_use_fsync(false);
        db_options.set_compaction_style(DBCompactionStyle::Level);
        db_options.set_min_write_buffer_number(1);
        db_options.set_max_write_buffer_number(2);
        // Without this, RocksDB keeps piling data into the last level until it grows large \
        //   enough to justify redistributing it across more levels; every L0->base compaction \
        //   in the meantime has to rewrite the entire (growing) base level alongside the new \
        //   L0 files, so write amplification grows with total data size until that one big \
        //   redistribution happens. Dynamic level bytes picks level target sizes based on \
        //   actual data size from the start instead, which RocksDB's own tuning guide \
        //   recommends enabling for essentially all new use cases.
        db_options.set_level_compaction_dynamic_level_bytes(true);
        let mut table_options = BlockBasedOptions::default();
        table_options.set_bloom_filter(10.0, false);
        table_options.set_whole_key_filtering(true);
        table_options.set_optimize_filters_for_memory(true);
        db_options.set_block_based_table_factory(&table_options);
        db_options.set_memtable_whole_key_filtering(true);

        // Set dynamic options
        if db_conf.compress {
            db_options.set_compression_type(DBCompressionType::Lz4);
            db_options.set_bottommost_compression_type(DBCompressionType::Zstd);
            db_options.set_bottommost_zstd_max_train_bytes(0, true);
        } else {
            db_options.set_compression_type(DBCompressionType::None);
        }

        db_options.set_max_open_files(if let Some(value) = db_conf.max_files {
            value as i32
        } else {
            -1
        });

        db_options.increase_parallelism(db_conf.parallelism as i32);
        db_options.set_max_subcompactions(db_conf.max_compactions as u32);
        db_options.set_max_background_jobs((db_conf.max_compactions + db_conf.max_flushes) as i32);
        db_options.set_write_buffer_size(db_conf.write_buffer * 1024);

        db_options
    }
}

impl crate::config::ConfigStoreKV {
    fn path(&self, collection_hash: StoreKVAtom) -> PathBuf {
        self.path.join(format!("{:x}", collection_hash))
    }
}

impl StoreGenericBuilder<StoreKVKey, StoreKV> for StoreKVBuilder {
    fn build(&self, pool_key: StoreKVKey) -> Result<StoreKV, ()> {
        let database = self.open(pool_key.collection_hash).map_err(|err| {
            tracing::error!("failed opening kv: {}", err);
        })?;
        let now = SystemTime::now();
        let store = StoreKV {
            database,
            last_used: Arc::new(RwLock::new(now)),
            last_flushed: Arc::new(RwLock::new(now)),
            lock: RwLock::new(false),
            kv_store_config: Arc::clone(&self.kv_store_config),
        };
        store.initialize_schema()?;

        Ok(store)
    }
}

impl StoreKV {
    pub fn count_buckets(&self) -> usize {
        self.count_prefix(&StoreKeyerBuilder::bucket_name_prefix())
    }

    pub fn stats(&self, collection: &str, deep: bool) -> Result<StoreCollectionStats, ()> {
        let default_cf = self.database.cf_handle("default").ok_or(())?;
        let documents_cf = self.database.cf_handle(DOCUMENTS_CF).ok_or(())?;
        let postings_cf = self.database.cf_handle(POSTINGS_CF).ok_or(())?;
        let schema_version = self
            .get(StoreKeyerBuilder::meta_to_value(0, &StoreMetaKey::SchemaVersion).as_bytes())
            .or(Err(()))?
            .and_then(|value| String::from_utf8(value).ok())
            .and_then(|value| value.parse().ok())
            .ok_or(())?;
        let mut stats = StoreCollectionStats {
            collection: collection.to_owned(),
            schema_version,
            index: self.cf_stats(default_cf)?,
            postings: self.cf_stats(postings_cf)?,
            documents: self.cf_stats(documents_cf)?,
            logical: None,
        };
        if !deep {
            return Ok(stats);
        }

        const FAMILY_NAMES: [&str; 9] = [
            "meta",
            "term_postings",
            "oid_to_iid",
            "iid_to_oid",
            "iid_to_terms",
            "bucket_name_to_id",
            "bucket_id_to_name",
            "iid_to_timestamp",
            "time_postings",
        ];
        let mut logical = StoreLogicalStats {
            families: FAMILY_NAMES
                .iter()
                .enumerate()
                .map(|(index, name)| StoreIndexFamilyStats {
                    index: index as u8,
                    name: (*name).to_owned(),
                    ..StoreIndexFamilyStats::default()
                })
                .collect(),
            ..StoreLogicalStats::default()
        };
        for item in self.database.iterator(IteratorMode::Start) {
            let (key, value) = item.or(Err(()))?;
            logical.index_key_bytes += key.len() as u64;
            logical.index_value_bytes += value.len() as u64;
            let Some(family_index) = StoreKeyerBuilder::family(&key) else {
                continue;
            };
            let Some(family) = logical.families.get_mut(usize::from(family_index)) else {
                continue;
            };
            family.keys += 1;
            family.key_bytes += key.len() as u64;
            family.value_bytes += value.len() as u64;
            match family_index {
                1 => Self::accumulate_posting(&mut logical.term_postings, &value)?,
                8 => Self::accumulate_posting(&mut logical.time_postings, &value)?,
                _ => {}
            }
        }
        for item in self.database.iterator_cf(postings_cf, IteratorMode::Start) {
            let (key, value) = item.or(Err(()))?;
            logical.index_key_bytes += key.len() as u64;
            logical.index_value_bytes += value.len() as u64;
            let Some(family_index) = StoreKeyerBuilder::family(&key) else {
                continue;
            };
            let Some(family) = logical.families.get_mut(usize::from(family_index)) else {
                continue;
            };
            family.keys += 1;
            family.key_bytes += key.len() as u64;
            family.value_bytes += value.len() as u64;
            match family_index {
                1 => Self::accumulate_posting(&mut logical.term_postings, &value)?,
                8 => Self::accumulate_posting(&mut logical.time_postings, &value)?,
                _ => {}
            }
        }
        for item in self.database.iterator_cf(documents_cf, IteratorMode::Start) {
            let (key, value) = item.or(Err(()))?;
            let (text_bytes, metadata_bytes) = StoreDocument::encoded_lengths(&value)?;
            logical.document_count += 1;
            logical.document_key_bytes += key.len() as u64;
            logical.document_encoded_bytes += value.len() as u64;
            logical.document_text_bytes += text_bytes as u64;
            logical.document_metadata_bytes += metadata_bytes as u64;
        }
        stats.logical = Some(logical);
        Ok(stats)
    }

    fn cf_stats(&self, cf: &impl rocksdb::AsColumnFamilyRef) -> Result<StoreColumnFamilyStats, ()> {
        let property = |name| {
            self.database
                .property_int_value_cf(cf, name)
                .or(Err(()))
                .map(|value| value.unwrap_or(0))
        };
        Ok(StoreColumnFamilyStats {
            live_data_bytes: property("rocksdb.estimate-live-data-size")?,
            sst_bytes: property("rocksdb.total-sst-files-size")?,
            memtable_bytes: property("rocksdb.cur-size-all-mem-tables")?,
            estimated_keys: property("rocksdb.estimate-num-keys")?,
        })
    }

    fn accumulate_posting(stats: &mut StorePostingStats, encoded: &[u8]) -> Result<(), ()> {
        let posting = StorePosting::decode(encoded)?;
        stats.fragments += 1;
        stats.encoded_bytes += encoded.len() as u64;
        stats.associations += posting.len() as u64;
        if posting.is_dense() {
            stats.dense_fragments += 1;
        } else {
            stats.sparse_fragments += 1;
        }
        Ok(())
    }

    // Streams one page of a bucket's documents over the wire (as opposed to `export_documents`, \
    //   which writes a full collection dump to a server-local file); this is what lets a router \
    //   proxy a dump request to a single backend without needing shared server-local storage.
    pub fn dump_bucket(
        &self,
        bucket: &str,
        offset: u64,
        limit: u64,
    ) -> Result<Vec<StoreDocumentRecord>, ()> {
        let Some(bucket_id) = self.resolve_bucket_id(bucket, false)? else {
            return Ok(Vec::new());
        };

        let cf = self.database.cf_handle(DOCUMENTS_CF).ok_or(())?;
        let prefix = StoreKeyerBuilder::document_prefix(bucket_id);
        let mut records = Vec::new();

        let iterator = self
            .database
            .iterator_cf(cf, IteratorMode::From(&prefix, Direction::Forward))
            .map_while(|item| item.ok())
            .take_while(|(key, _)| key.starts_with(prefix.as_slice()))
            .skip(offset as usize)
            .take(limit as usize);

        for (key, value) in iterator {
            if key.len() != 8 {
                return Err(());
            }

            let iid = StoreKeyerBuilder::decode_u32_ordered(&key[4..]).ok_or(())?;
            let oid = self
                .get(StoreKeyerBuilder::iid_to_oid(bucket_id, iid).as_bytes())
                .or(Err(()))?
                .and_then(|encoded| String::from_utf8(encoded).ok())
                .ok_or(())?;

            records.push(StoreDocumentRecord {
                bucket: bucket.to_owned(),
                document: StoreDocument::decode(oid, &value)?,
            });
        }

        Ok(records)
    }

    // Enumerates bucket names for a collection, one page at a time; used to let a client (or a \
    //   router, broadcasting across backends) discover the full bucket set before dumping it.
    pub fn list_buckets(&self, offset: u64, limit: u64) -> Result<Vec<String>, ()> {
        let prefix = StoreKeyerBuilder::bucket_name_prefix();

        self.database
            .iterator(IteratorMode::From(&prefix, Direction::Forward))
            .map_while(|item| item.ok())
            .take_while(|(key, _)| key.starts_with(prefix.as_slice()))
            .skip(offset as usize)
            .take(limit as usize)
            .map(|(key, _)| String::from_utf8(key[prefix.len()..].to_vec()).map_err(|_| ()))
            .collect()
    }

    pub fn export_documents(&self, bucket_filter: Option<&str>, path: &Path) -> Result<u64, ()> {
        let cf = self.database.cf_handle(DOCUMENTS_CF).ok_or(())?;
        let file = fs::File::create(path).map_err(|_| ())?;
        let writer = BufWriter::new(file);
        let mut encoder = zstd::stream::write::Encoder::new(writer, 3).map_err(|_| ())?;
        let mut count = 0;
        for item in self.database.iterator_cf(cf, IteratorMode::Start) {
            let (key, value) = item.map_err(|_| ())?;
            if key.len() != 8 {
                return Err(());
            }
            let bucket_id = StoreKeyerBuilder::decode_u32_ordered(&key[..4]).ok_or(())?;
            let iid = StoreKeyerBuilder::decode_u32_ordered(&key[4..]).ok_or(())?;
            let bucket = self
                .get(StoreKeyerBuilder::bucket_id_to_name(bucket_id).as_bytes())
                .or(Err(()))?
                .and_then(|encoded| String::from_utf8(encoded).ok())
                .ok_or(())?;
            if bucket_filter.is_some_and(|filter| filter != bucket) {
                continue;
            }
            let oid = self
                .get(StoreKeyerBuilder::iid_to_oid(bucket_id, iid).as_bytes())
                .or(Err(()))?
                .and_then(|encoded| String::from_utf8(encoded).ok())
                .ok_or(())?;
            let record = StoreDocumentRecord {
                bucket,
                document: StoreDocument::decode(oid, &value)?,
            };
            serde_json::to_writer(&mut encoder, &record).map_err(|_| ())?;
            encoder.write_all(b"\n").map_err(|_| ())?;
            count += 1;
        }
        encoder.finish().map_err(|_| ())?.flush().map_err(|_| ())?;
        Ok(count)
    }

    fn count_prefix(&self, prefix: &[u8]) -> usize {
        self.database
            .iterator(IteratorMode::From(prefix, Direction::Forward))
            .map_while(|item| item.ok())
            .take_while(|(key, _)| key.starts_with(prefix))
            .count()
    }

    fn initialize_schema(&self) -> Result<(), ()> {
        let key = StoreKeyerBuilder::meta_to_value(0, &StoreMetaKey::SchemaVersion);

        match self.get(key.as_bytes()).or(Err(()))? {
            Some(encoded) => {
                let version = String::from_utf8(encoded)
                    .ok()
                    .and_then(|value| value.parse::<u32>().ok());

                if version == Some(STORE_SCHEMA_VERSION) {
                    Ok(())
                } else {
                    tracing::error!(
                        "unsupported KV schema version: {:?}, expected {}",
                        version,
                        STORE_SCHEMA_VERSION
                    );
                    Err(())
                }
            }
            None => {
                if self.database.iterator(IteratorMode::Start).next().is_some() {
                    tracing::error!("legacy KV schema detected; full re-ingestion is required");
                    return Err(());
                }

                self.put(key.as_bytes(), STORE_SCHEMA_VERSION.to_string().as_bytes())
                    .or(Err(()))
            }
        }
    }

    fn resolve_bucket_id(&self, bucket: &str, create: bool) -> Result<Option<StoreBucketID>, ()> {
        let name_key = StoreKeyerBuilder::bucket_name_to_id(bucket);

        if let Some(encoded) = self.get(name_key.as_bytes()).or(Err(()))? {
            return Ok(StoreKVAction::decode_u32(&encoded).ok());
        }

        if !create {
            return Ok(None);
        }

        let counter_key = StoreKeyerBuilder::meta_to_value(0, &StoreMetaKey::BucketIDIncr);
        let next_id = self
            .get(counter_key.as_bytes())
            .or(Err(()))?
            .and_then(|encoded| String::from_utf8(encoded).ok())
            .and_then(|encoded| encoded.parse::<StoreBucketID>().ok())
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(())?;
        let encoded_id = StoreKVAction::encode_u32(next_id);
        let reverse_key = StoreKeyerBuilder::bucket_id_to_name(next_id);

        let mut batch = WriteBatch::default();
        batch.put(counter_key.as_bytes(), next_id.to_string().as_bytes());
        batch.put(name_key.as_bytes(), encoded_id);
        batch.put(reverse_key.as_bytes(), bucket.as_bytes());
        self.do_write(batch).or(Err(()))?;

        Ok(Some(next_id))
    }

    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, DBError> {
        self.database.get(key)
    }

    pub fn put(&self, key: &[u8], data: &[u8]) -> Result<(), DBError> {
        let mut batch = WriteBatch::default();

        batch.put(key, data);

        self.do_write(batch)
    }

    pub fn delete(&self, key: &[u8]) -> Result<(), DBError> {
        let mut batch = WriteBatch::default();

        batch.delete(key);

        self.do_write(batch)
    }

    fn flush(&self) -> Result<(), DBError> {
        // Generate flush options
        let mut flush_options = FlushOptions::default();

        flush_options.set_wait(true);

        let default_cf = self
            .database
            .cf_handle("default")
            .expect("default column family missing");
        let documents_cf = self
            .database
            .cf_handle(DOCUMENTS_CF)
            .expect("documents column family missing");
        let postings_cf = self
            .database
            .cf_handle(POSTINGS_CF)
            .expect("postings column family missing");
        self.database
            .flush_cfs_opt(&[default_cf, documents_cf, postings_cf], &flush_options)
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

    fn do_write_profiled(&self, batch: WriteBatch) -> Result<StoreWriteProfile, DBError> {
        use rocksdb::{PerfContext, PerfMetric, PerfStatsLevel};

        let mut profile = StoreWriteProfile::default();
        batch.iterate_cf(&mut profile.batch);
        rocksdb::perf::set_perf_stats(PerfStatsLevel::EnableTime);
        let mut context = PerfContext::default();
        context.reset();
        let result = self.do_write(batch);
        profile.wal_nanos = context.metric(PerfMetric::WriteWalTime);
        profile.memtable_nanos = context.metric(PerfMetric::WriteMemtableTime);
        profile.delay_nanos = context.metric(PerfMetric::WriteDelayTime);
        profile.pre_and_post_nanos = context.metric(PerfMetric::WritePreAndPostProcessTime);
        profile.db_mutex_nanos = context.metric(PerfMetric::DbMutexLockNanos);
        profile.db_condition_wait_nanos = context.metric(PerfMetric::DbConditionWaitNanos);
        profile.merge_operator_nanos = context.metric(PerfMetric::MergeOperatorTimeNanos);
        rocksdb::perf::set_perf_stats(PerfStatsLevel::Disable);
        result.map(|()| profile)
    }

    // Snapshot compaction pressure after a profiled bulk write.
    fn health(&self) -> StoreKVHealth {
        let Some(index_cf) = self.database.cf_handle("default") else {
            return StoreKVHealth::default();
        };
        let Some(documents_cf) = self.database.cf_handle(DOCUMENTS_CF) else {
            return StoreKVHealth::default();
        };
        let Some(postings_cf) = self.database.cf_handle(POSTINGS_CF) else {
            return StoreKVHealth::default();
        };
        let cf_property = |cf, name| self.database.property_int_value_cf(cf, name).ok().flatten();

        StoreKVHealth {
            index_l0_files: cf_property(&index_cf, "rocksdb.num-files-at-level0"),
            postings_l0_files: cf_property(&postings_cf, "rocksdb.num-files-at-level0"),
            documents_l0_files: cf_property(&documents_cf, "rocksdb.num-files-at-level0"),
            index_pending_compaction_bytes: cf_property(
                &index_cf,
                "rocksdb.estimate-pending-compaction-bytes",
            ),
            documents_pending_compaction_bytes: cf_property(
                &documents_cf,
                "rocksdb.estimate-pending-compaction-bytes",
            ),
            postings_pending_compaction_bytes: cf_property(
                &postings_cf,
                "rocksdb.estimate-pending-compaction-bytes",
            ),
            delayed_write_rate: self
                .database
                .property_int_value("rocksdb.actual-delayed-write-rate")
                .ok()
                .flatten(),
            write_stopped: self
                .database
                .property_int_value("rocksdb.is-write-stopped")
                .ok()
                .flatten(),
        }
    }
}

impl StoreGeneric for StoreKV {
    fn ref_last_used(&self) -> &RwLock<SystemTime> {
        &self.last_used
    }
}

impl<'build> StoreKVActionBuilder<'build> {
    pub fn access(bucket: StoreItemPart, store: Option<StoreKVBox>) -> StoreKVAction {
        Self::build(bucket, store, false)
    }

    pub fn access_or_create(bucket: StoreItemPart, store: Option<StoreKVBox>) -> StoreKVAction {
        Self::build(bucket, store, true)
    }

    pub fn erase<T: AsRef<str>>(&self, collection: T, bucket: Option<T>) -> Result<u32, ()> {
        self.dispatch_erase("kv", collection, bucket)
    }

    fn build(bucket: StoreItemPart, store: Option<StoreKVBox>, create: bool) -> StoreKVAction {
        let bucket_id = store.as_ref().and_then(|store| {
            store
                .resolve_bucket_id(bucket.as_str(), create)
                .inspect_err(|_| {
                    tracing::error!("failed resolving bucket identifier for {}", bucket.as_str())
                })
                .ok()
                .flatten()
        });

        StoreKVAction {
            store,
            bucket,
            bucket_id,
        }
    }
}

impl<'build> StoreGenericActionBuilder for StoreKVActionBuilder<'build> {
    fn proceed_erase_collection(&self, collection_str: &str) -> Result<u32, ()> {
        let collection_atom = StoreKeyerHasher::to_compact(collection_str);
        let collection_path = self.kv_pool.kv_store_config.path(collection_atom);

        // Force a KV store close
        self.kv_pool.close(collection_atom);

        if collection_path.exists() {
            tracing::debug!(
                "kv collection store exists, erasing: {}/* at path: {:?}",
                collection_str,
                &collection_path
            );

            // Remove KV store storage from filesystem
            let erase_result = fs::remove_dir_all(&collection_path);

            if erase_result.is_ok() {
                tracing::debug!("done with kv collection erasure");

                Ok(1)
            } else {
                Err(())
            }
        } else {
            tracing::debug!(
                "kv collection store does not exist, consider already erased: {}/* at path: {:?}",
                collection_str,
                &collection_path
            );

            Ok(0)
        }
    }

    fn proceed_erase_bucket(&self, _collection: &str, _bucket: &str) -> Result<u32, ()> {
        // This one is not implemented, as we need to acquire the collection; which would cause \
        //   a party-killer dead-lock.
        Err(())
    }
}

impl<'a> StoreKVAction<'a> {
    pub fn bucket_id(&self) -> Option<StoreBucketID> {
        self.bucket_id
    }

    pub fn count_terms(&self) -> usize {
        let (Some(store), Some(bucket_id)) = (&self.store, self.bucket_id) else {
            return 0;
        };
        let Some(postings_cf) = store.database.cf_handle(POSTINGS_CF) else {
            return 0;
        };
        let prefix = StoreKeyerBuilder::term_posting_family_prefix(bucket_id);
        let mut previous = None;
        let mut count = 0;
        for item in store
            .database
            .iterator_cf(postings_cf, IteratorMode::From(&prefix, Direction::Forward))
        {
            let Ok((key, _)) = item else { break };
            if !key.starts_with(&prefix) {
                break;
            }
            let Some(term) =
                StoreKeyerBuilder::decode_term_route_with_suffix(&key[prefix.len()..], 2)
            else {
                continue;
            };
            if previous.as_deref() != Some(term) {
                count += 1;
                previous = Some(term.to_owned());
            }
        }
        count
    }

    pub fn list_terms(&self, limit: usize, offset: usize) -> Vec<String> {
        let (Some(store), Some(bucket_id)) = (&self.store, self.bucket_id) else {
            return Vec::new();
        };
        let Some(postings_cf) = store.database.cf_handle(POSTINGS_CF) else {
            return Vec::new();
        };
        let prefix = StoreKeyerBuilder::term_posting_family_prefix(bucket_id);
        let mut previous = None;
        let mut skipped = 0;
        let mut terms = Vec::with_capacity(limit);
        for item in store
            .database
            .iterator_cf(postings_cf, IteratorMode::From(&prefix, Direction::Forward))
        {
            let Ok((key, _)) = item else { break };
            if !key.starts_with(&prefix) {
                break;
            }
            let Some(term) =
                StoreKeyerBuilder::decode_term_route_with_suffix(&key[prefix.len()..], 2)
            else {
                continue;
            };
            if previous.as_deref() == Some(term) {
                continue;
            }
            previous = Some(term.to_owned());
            if skipped < offset {
                skipped += 1;
                continue;
            }
            terms.push(term.to_owned());
            if terms.len() == limit {
                break;
            }
        }
        terms
    }

    /// Meta-to-Value mapper
    ///
    /// [IDX=0] ((meta)) ~> ((value))
    pub fn get_meta_to_value(&self, meta: StoreMetaKey) -> Result<Option<StoreMetaValue>, ()> {
        if let Some(ref store) = self.store {
            let store_key = StoreKeyerBuilder::meta_to_value(self.bucket_id.ok_or(())?, &meta);

            tracing::debug!("store get meta-to-value: {}", store_key);

            match store.get(&store_key.as_bytes()) {
                Ok(Some(value)) => {
                    tracing::debug!("got meta-to-value: {}", store_key);

                    Ok(if let Ok(value) = str::from_utf8(&value) {
                        match meta {
                            StoreMetaKey::IIDIncr => value
                                .parse::<StoreObjectIID>()
                                .ok()
                                .map(StoreMetaValue::IIDIncr)
                                .or(None),
                            StoreMetaKey::BucketIDIncr => value
                                .parse::<StoreBucketID>()
                                .ok()
                                .map(StoreMetaValue::BucketIDIncr)
                                .or(None),
                            StoreMetaKey::SchemaVersion => value
                                .parse::<u32>()
                                .ok()
                                .map(StoreMetaValue::SchemaVersion)
                                .or(None),
                        }
                    } else {
                        None
                    })
                }
                Ok(None) => {
                    tracing::debug!("no meta-to-value found: {}", store_key);

                    Ok(None)
                }
                Err(err) => {
                    tracing::error!(
                        "error getting meta-to-value: {} with trace: {}",
                        store_key,
                        err
                    );

                    Err(())
                }
            }
        } else {
            Ok(None)
        }
    }

    pub fn set_meta_to_value(&self, meta: StoreMetaKey, value: StoreMetaValue) -> Result<(), ()> {
        if let Some(ref store) = self.store {
            let store_key = StoreKeyerBuilder::meta_to_value(self.bucket_id.ok_or(())?, &meta);

            tracing::debug!("store set meta-to-value: {}", store_key);

            let value_string = match value {
                StoreMetaValue::IIDIncr(iid_incr) => iid_incr.to_string(),
                StoreMetaValue::BucketIDIncr(bucket_id_incr) => bucket_id_incr.to_string(),
                StoreMetaValue::SchemaVersion(version) => version.to_string(),
            };

            store
                .put(&store_key.as_bytes(), value_string.as_bytes())
                .or(Err(()))
        } else {
            Err(())
        }
    }

    /// Read a bounded posting list in reverse IID order.
    pub fn get_term_iids_desc(&self, term: &str, limit: usize) -> Result<Vec<StoreObjectIID>, ()> {
        let Some(ref store) = self.store else {
            return Ok(Vec::new());
        };
        let postings_cf = store.database.cf_handle(POSTINGS_CF).ok_or(())?;
        let bucket_id = self.bucket_id.ok_or(())?;
        let prefix = StoreKeyerBuilder::term_posting_prefix(bucket_id, term);
        let end = Self::prefix_end(&prefix).ok_or(())?;
        let mut iids = Vec::with_capacity(limit.min(256));

        for item in store
            .database
            .iterator_cf(postings_cf, IteratorMode::From(&end, Direction::Reverse))
        {
            let (key, value) = item.or(Err(()))?;
            if !key.starts_with(&prefix) {
                if key.as_ref() < prefix.as_slice() {
                    break;
                }
                continue;
            }
            let shard = key
                .get(prefix.len()..prefix.len() + 2)
                .and_then(|encoded| encoded.try_into().ok())
                .map(u16::from_be_bytes)
                .ok_or(())?;
            let posting = StorePosting::decode(&value)?;
            for offset in posting.offsets_desc() {
                iids.push(StorePosting::iid(shard, offset));
                if iids.len() == limit {
                    return Ok(iids);
                }
            }
        }

        Ok(iids)
    }

    pub fn get_term_iids_in_time_range(
        &self,
        term: &str,
        from_ms: u64,
        to_ms: u64,
        limit: usize,
    ) -> Result<Vec<StoreObjectIID>, ()> {
        let Some(ref store) = self.store else {
            return Ok(Vec::new());
        };
        let postings_cf = store.database.cf_handle(POSTINGS_CF).ok_or(())?;
        let bucket_id = self.bucket_id.ok_or(())?;
        let prefix = StoreKeyerBuilder::time_posting_prefix(bucket_id);
        let from_slice = from_ms / TIME_SLICE_MS;
        let to_slice = to_ms / TIME_SLICE_MS;
        let start = StoreKeyerBuilder::time_posting(bucket_id, to_slice, u16::MAX);
        let mut iids = Vec::new();
        let mut accepted_slice = None;

        for item in store.database.iterator_cf(
            postings_cf,
            IteratorMode::From(start.as_bytes(), Direction::Reverse),
        ) {
            let (key, value) = item.or(Err(()))?;
            if !key.starts_with(&prefix) {
                break;
            }
            let route = key.get(prefix.len()..).ok_or(())?;
            if route.len() != 10 {
                return Err(());
            }
            let time_slice = u64::from_be_bytes(route[..8].try_into().map_err(|_| ())?);
            if time_slice < from_slice {
                break;
            }
            if iids.len() >= limit && accepted_slice != Some(time_slice) {
                break;
            }
            accepted_slice = Some(time_slice);
            let shard = u16::from_be_bytes(route[8..].try_into().map_err(|_| ())?);
            let time_posting = StorePosting::decode(&value)?;
            let term_key = StoreKeyerBuilder::term_posting(bucket_id, term, shard);
            let Some(term_encoded) = store
                .database
                .get_cf(postings_cf, term_key.as_bytes())
                .or(Err(()))?
            else {
                continue;
            };
            let term_posting = StorePosting::decode(&term_encoded)?;
            for offset in time_posting.intersection_offsets_desc(&term_posting) {
                let iid = StorePosting::iid(shard, offset);
                if self
                    .get_iid_timestamp(iid)?
                    .is_some_and(|timestamp| timestamp >= from_ms && timestamp <= to_ms)
                {
                    iids.push(iid);
                }
            }
        }

        iids.sort_unstable_by(|left, right| {
            let left_ts = self.get_iid_timestamp(*left).ok().flatten().unwrap_or(0);
            let right_ts = self.get_iid_timestamp(*right).ok().flatten().unwrap_or(0);
            right_ts.cmp(&left_ts).then_with(|| right.cmp(left))
        });
        iids.truncate(limit);
        Ok(iids)
    }

    pub fn get_term_frequency(&self, term: &str) -> Result<u32, ()> {
        let store = self.store.as_ref().ok_or(())?;
        let key = StoreKeyerBuilder::term_frequency(self.bucket_id.ok_or(())?, term);
        store
            .get(key.as_bytes())
            .or(Err(()))?
            .map(|encoded| Self::decode_u32(&encoded))
            .transpose()
            .map(|frequency| frequency.unwrap_or(0))
    }

    pub fn insert_term_iid(&self, term: &str, iid: StoreObjectIID) -> Result<(bool, u32), ()> {
        let Some(ref store) = self.store else {
            return Err(());
        };
        let mut batch = WriteBatch::default();
        let result = self.append_insert_term_iid(&mut batch, term, iid)?;
        if result.0 {
            store.do_write(batch).or(Err(()))?;
        }
        Ok(result)
    }

    pub fn batch_insert_iid_terms(
        &self,
        iid: StoreObjectIID,
        existing_terms: &[String],
        new_terms: &[String],
    ) -> Result<Vec<(String, u32)>, ()> {
        let Some(ref store) = self.store else {
            return Err(());
        };
        let mut batch = WriteBatch::default();
        let mut terms = existing_terms.to_vec();
        let mut frequencies = Vec::with_capacity(new_terms.len());

        for term in new_terms {
            let (inserted, frequency) = self.append_insert_term_iid(&mut batch, term, iid)?;
            if inserted {
                terms.push(term.clone());
                frequencies.push((term.clone(), frequency));
            }
        }

        if !frequencies.is_empty() {
            let key = StoreKeyerBuilder::iid_to_terms(self.bucket_id.ok_or(())?, iid);
            batch.put(key.as_bytes(), Self::encode_terms(&terms)?);
            store.do_write(batch).or(Err(()))?;
        }
        Ok(frequencies)
    }

    pub fn remove_term_iid(&self, term: &str, iid: StoreObjectIID) -> Result<(bool, u32), ()> {
        let Some(ref store) = self.store else {
            return Err(());
        };
        let mut batch = WriteBatch::default();
        let result = self.append_remove_term_iid(&mut batch, term, iid)?;
        if result.0 {
            store.do_write(batch).or(Err(()))?;
        }
        Ok(result)
    }

    pub fn batch_remove_iid_terms(
        &self,
        iid: StoreObjectIID,
        remaining_terms: &[String],
        removed_terms: &[String],
    ) -> Result<Vec<(String, u32)>, ()> {
        let Some(ref store) = self.store else {
            return Err(());
        };
        let mut batch = WriteBatch::default();
        let mut frequencies = Vec::with_capacity(removed_terms.len());
        for term in removed_terms {
            let (removed, frequency) = self.append_remove_term_iid(&mut batch, term, iid)?;
            if removed {
                frequencies.push((term.clone(), frequency));
            }
        }
        if !frequencies.is_empty() {
            let key = StoreKeyerBuilder::iid_to_terms(self.bucket_id.ok_or(())?, iid);
            batch.put(key.as_bytes(), Self::encode_terms(remaining_terms)?);
            store.do_write(batch).or(Err(()))?;
        }
        Ok(frequencies)
    }

    fn append_insert_term_iid(
        &self,
        batch: &mut WriteBatch,
        term: &str,
        iid: StoreObjectIID,
    ) -> Result<(bool, u32), ()> {
        let store = self.store.as_ref().ok_or(())?;
        let postings_cf = store.database.cf_handle(POSTINGS_CF).ok_or(())?;
        let bucket_id = self.bucket_id.ok_or(())?;
        let posting_key =
            StoreKeyerBuilder::term_posting(bucket_id, term, StorePosting::shard(iid));
        let posting = store
            .database
            .get_cf(postings_cf, posting_key.as_bytes())
            .or(Err(()))?
            .map(|encoded| StorePosting::decode(&encoded))
            .transpose()?
            .unwrap_or_default();
        let old_frequency = self.get_term_frequency(term)?;
        let offset = StorePosting::offset(iid);
        if posting.contains(offset) {
            return Ok((false, old_frequency));
        }
        let frequency = old_frequency.checked_add(1).ok_or(())?;
        let mut delta = StorePosting::default();
        delta.insert(offset);
        batch.merge_cf(postings_cf, posting_key.as_bytes(), delta.encode());
        batch.put(
            StoreKeyerBuilder::term_frequency(bucket_id, term).as_bytes(),
            Self::encode_u32(frequency),
        );
        Ok((true, frequency))
    }

    fn append_remove_term_iid(
        &self,
        batch: &mut WriteBatch,
        term: &str,
        iid: StoreObjectIID,
    ) -> Result<(bool, u32), ()> {
        let store = self.store.as_ref().ok_or(())?;
        let postings_cf = store.database.cf_handle(POSTINGS_CF).ok_or(())?;
        let bucket_id = self.bucket_id.ok_or(())?;
        let posting_key =
            StoreKeyerBuilder::term_posting(bucket_id, term, StorePosting::shard(iid));
        let Some(encoded) = store
            .database
            .get_cf(postings_cf, posting_key.as_bytes())
            .or(Err(()))?
        else {
            return Ok((false, self.get_term_frequency(term)?));
        };
        let mut posting = StorePosting::decode(&encoded)?;
        if !posting.remove(StorePosting::offset(iid)) {
            return Ok((false, self.get_term_frequency(term)?));
        }
        let frequency = self.get_term_frequency(term)?.checked_sub(1).ok_or(())?;
        if posting.is_empty() {
            batch.delete_cf(postings_cf, posting_key.as_bytes());
        } else {
            batch.put_cf(postings_cf, posting_key.as_bytes(), posting.encode());
        }
        let frequency_key = StoreKeyerBuilder::term_frequency(bucket_id, term);
        if frequency == 0 {
            batch.delete(frequency_key.as_bytes());
        } else {
            batch.put(frequency_key.as_bytes(), Self::encode_u32(frequency));
        }
        Ok((true, frequency))
    }

    pub fn get_document_by_iid(&self, iid: StoreObjectIID) -> Result<Option<StoreDocument>, ()> {
        let store = self.store.as_ref().ok_or(())?;
        let bucket_id = self.bucket_id.ok_or(())?;
        let cf = store.database.cf_handle(DOCUMENTS_CF).ok_or(())?;
        let key = StoreKeyerBuilder::document(bucket_id, iid);
        let Some(encoded) = store.database.get_cf(cf, key.as_bytes()).or(Err(()))? else {
            return Ok(None);
        };
        let oid = self.get_iid_to_oid(iid)?.ok_or(())?;
        StoreDocument::decode(oid, &encoded).map(Some)
    }

    pub fn get_document(&self, oid: StoreObjectOID<'a>) -> Result<Option<StoreDocument>, ()> {
        let Some(iid) = self.get_oid_to_iid(oid)? else {
            return Ok(None);
        };
        self.get_document_by_iid(iid)
    }

    pub fn get_iid_timestamp(&self, iid: StoreObjectIID) -> Result<Option<u64>, ()> {
        let store = self.store.as_ref().ok_or(())?;
        let key = StoreKeyerBuilder::iid_to_timestamp(self.bucket_id.ok_or(())?, iid);
        store
            .get(key.as_bytes())
            .or(Err(()))?
            .map(|encoded| Self::decode_u64(&encoded))
            .transpose()
    }

    pub fn batch_upsert_document(
        &self,
        iid: StoreObjectIID,
        oid: StoreObjectOID<'a>,
        is_new_iid: bool,
        old_terms: &[String],
        new_terms: &[String],
        document: &StoreDocument,
    ) -> Result<Vec<(String, u32)>, ()> {
        let store = self.store.as_ref().ok_or(())?;
        let bucket_id = self.bucket_id.ok_or(())?;
        let document_cf = store.database.cf_handle(DOCUMENTS_CF).ok_or(())?;
        let old_term_set = old_terms.iter().collect::<HashSet<_>>();
        let new_term_set = new_terms.iter().collect::<HashSet<_>>();
        let mut frequencies = Vec::new();
        let mut batch = WriteBatch::default();

        if is_new_iid {
            batch.put(
                StoreKeyerBuilder::meta_to_value(bucket_id, &StoreMetaKey::IIDIncr).as_bytes(),
                iid.to_string().as_bytes(),
            );
            batch.put(
                StoreKeyerBuilder::oid_to_iid(bucket_id, oid).as_bytes(),
                Self::encode_u32(iid),
            );
            batch.put(
                StoreKeyerBuilder::iid_to_oid(bucket_id, iid).as_bytes(),
                oid.as_bytes(),
            );
        }

        for term in old_term_set.difference(&new_term_set) {
            let (removed, frequency) = self.append_remove_term_iid(&mut batch, term, iid)?;
            if removed {
                frequencies.push(((*term).clone(), frequency));
            }
        }
        for term in new_term_set.difference(&old_term_set) {
            let (inserted, frequency) = self.append_insert_term_iid(&mut batch, term, iid)?;
            if inserted {
                frequencies.push(((*term).clone(), frequency));
            }
        }

        if let Some(old_timestamp) = self.get_iid_timestamp(iid)? {
            if old_timestamp != document.timestamp_ms {
                self.append_remove_time_iid(&mut batch, old_timestamp, iid)?;
                self.append_insert_time_iid(&mut batch, document.timestamp_ms, iid)?;
            }
        } else {
            self.append_insert_time_iid(&mut batch, document.timestamp_ms, iid)?;
        }

        batch.put(
            StoreKeyerBuilder::iid_to_terms(bucket_id, iid).as_bytes(),
            Self::encode_terms(new_terms)?,
        );
        batch.put(
            StoreKeyerBuilder::iid_to_timestamp(bucket_id, iid).as_bytes(),
            Self::encode_u64(document.timestamp_ms),
        );
        batch.put_cf(
            document_cf,
            StoreKeyerBuilder::document(bucket_id, iid).as_bytes(),
            document.encode()?,
        );
        store.do_write(batch).or(Err(()))?;
        Ok(frequencies)
    }

    pub(crate) fn batch_insert_fresh_documents(
        &self,
        documents: &[(StoreDocument, Vec<String>)],
        profiling: bool,
    ) -> Result<StoreFreshBatchResult, ()> {
        let store = self.store.as_ref().ok_or(())?;
        let bucket_id = self.bucket_id.ok_or(())?;
        let document_cf = store.database.cf_handle(DOCUMENTS_CF).ok_or(())?;
        let postings_cf = store.database.cf_handle(POSTINGS_CF).ok_or(())?;
        let mut timings = StoreFreshBatchTimings::default();
        let mut batch = WriteBatch::default();
        let mut postings = HashMap::<(String, StoreIIDShard), StorePosting>::new();
        let mut frequencies = HashMap::<String, u32>::new();
        let mut time_postings = HashMap::<(u64, StoreIIDShard), StorePosting>::new();
        let mut seen_oids = HashSet::<String>::new();
        let metadata_started = profile_started(profiling);
        let mut iid_counter = match self.get_meta_to_value(StoreMetaKey::IIDIncr)? {
            Some(StoreMetaValue::IIDIncr(value)) => Some(value),
            Some(_) => return Err(()),
            None => None,
        };
        timings.metadata_reads = profile_elapsed(metadata_started);
        let empty_bucket = iid_counter.is_none();
        let mut written = 0;
        let mut rejected = 0;

        for (document, terms) in documents {
            let document_encode_started = profile_started(profiling);
            let encoded_document = match document.encode() {
                Ok(encoded) => encoded,
                Err(()) => {
                    timings.document_encode += profile_elapsed(document_encode_started);
                    rejected += 1;
                    continue;
                }
            };
            timings.document_encode += profile_elapsed(document_encode_started);
            if !seen_oids.insert(document.oid.clone()) {
                rejected += 1;
                continue;
            }
            if !empty_bucket {
                let oid_read_started = profile_started(profiling);
                let oid_exists = self.get_oid_to_iid(&document.oid)?.is_some();
                timings.oid_reads += profile_elapsed(oid_read_started);
                timings.oid_read_count += 1;
                if oid_exists {
                    rejected += 1;
                    continue;
                }
            }
            let iid = match iid_counter {
                Some(value) => value.checked_add(1).ok_or(())?,
                None => 0,
            };
            iid_counter = Some(iid);
            let mut document_terms = Vec::new();

            for term in terms {
                if document_terms.contains(term) {
                    continue;
                }
                document_terms.push(term.clone());
                let shard = StorePosting::shard(iid);
                let posting = match postings.entry((term.clone(), shard)) {
                    hashbrown::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                    hashbrown::hash_map::Entry::Vacant(entry) => {
                        entry.insert(StorePosting::default())
                    }
                };
                if posting.insert(StorePosting::offset(iid)) {
                    if let hashbrown::hash_map::Entry::Vacant(entry) =
                        frequencies.entry(term.clone())
                    {
                        entry.insert(if empty_bucket {
                            0
                        } else {
                            let frequency_read_started = profile_started(profiling);
                            let frequency = self.get_term_frequency(term)?;
                            timings.frequency_reads += profile_elapsed(frequency_read_started);
                            timings.frequency_read_count += 1;
                            frequency
                        });
                    }
                    let frequency = frequencies.get_mut(term).ok_or(())?;
                    *frequency = frequency.checked_add(1).ok_or(())?;
                }
            }

            let time_slice = document.timestamp_ms / TIME_SLICE_MS;
            let shard = StorePosting::shard(iid);
            let time_posting = match time_postings.entry((time_slice, shard)) {
                hashbrown::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                hashbrown::hash_map::Entry::Vacant(entry) => entry.insert(StorePosting::default()),
            };
            time_posting.insert(StorePosting::offset(iid));
            batch.put(
                StoreKeyerBuilder::oid_to_iid(bucket_id, &document.oid).as_bytes(),
                Self::encode_u32(iid),
            );
            batch.put(
                StoreKeyerBuilder::iid_to_oid(bucket_id, iid).as_bytes(),
                document.oid.as_bytes(),
            );
            batch.put(
                StoreKeyerBuilder::iid_to_terms(bucket_id, iid).as_bytes(),
                Self::encode_terms(&document_terms)?,
            );
            batch.put(
                StoreKeyerBuilder::iid_to_timestamp(bucket_id, iid).as_bytes(),
                Self::encode_u64(document.timestamp_ms),
            );
            batch.put_cf(
                document_cf,
                StoreKeyerBuilder::document(bucket_id, iid).as_bytes(),
                encoded_document,
            );
            written += 1;
        }

        let batch_finalize_started = profile_started(profiling);
        for ((term, shard), posting) in postings {
            batch.merge_cf(
                postings_cf,
                StoreKeyerBuilder::term_posting(bucket_id, &term, shard).as_bytes(),
                posting.encode(),
            );
        }
        for ((time_slice, shard), posting) in time_postings {
            batch.merge_cf(
                postings_cf,
                StoreKeyerBuilder::time_posting(bucket_id, time_slice, shard).as_bytes(),
                posting.encode(),
            );
        }
        for (term, frequency) in &frequencies {
            batch.put(
                StoreKeyerBuilder::term_frequency(bucket_id, term).as_bytes(),
                Self::encode_u32(*frequency),
            );
        }
        if let Some(iid_counter) = iid_counter {
            batch.put(
                StoreKeyerBuilder::meta_to_value(bucket_id, &StoreMetaKey::IIDIncr).as_bytes(),
                iid_counter.to_string().as_bytes(),
            );
        }
        timings.batch_finalize = profile_elapsed(batch_finalize_started);
        if written > 0 {
            let database_write_started = profile_started(profiling);
            if profiling {
                let write_profile = store.do_write_profiled(batch).or(Err(()))?;
                timings.write_wal = Duration::from_nanos(write_profile.wal_nanos);
                timings.write_memtable = Duration::from_nanos(write_profile.memtable_nanos);
                timings.write_delay = Duration::from_nanos(write_profile.delay_nanos);
                timings.write_pre_and_post = Duration::from_nanos(write_profile.pre_and_post_nanos);
                timings.write_db_mutex = Duration::from_nanos(write_profile.db_mutex_nanos);
                timings.write_db_condition_wait =
                    Duration::from_nanos(write_profile.db_condition_wait_nanos);
                timings.write_merge_operator =
                    Duration::from_nanos(write_profile.merge_operator_nanos);
                timings.batch_index_bytes = write_profile.batch.bytes[DEFAULT_CF_ID as usize];
                timings.batch_documents_bytes = write_profile.batch.bytes[DOCUMENTS_CF_ID as usize];
                timings.batch_postings_bytes = write_profile.batch.bytes[POSTINGS_CF_ID as usize];
                timings.batch_put_count = write_profile.batch.puts;
                timings.batch_delete_count = write_profile.batch.deletes;
                timings.batch_merge_count = write_profile.batch.merges;
            } else {
                store.do_write(batch).or(Err(()))?;
            }
            timings.database_write = profile_elapsed(database_write_started);
        }
        let health = if profiling {
            store.health()
        } else {
            StoreKVHealth::default()
        };
        Ok(StoreFreshBatchResult {
            written,
            rejected,
            frequencies: frequencies.into_iter().collect(),
            timings,
            health,
        })
    }

    fn append_insert_time_iid(
        &self,
        batch: &mut WriteBatch,
        timestamp_ms: u64,
        iid: StoreObjectIID,
    ) -> Result<(), ()> {
        let store = self.store.as_ref().ok_or(())?;
        let postings_cf = store.database.cf_handle(POSTINGS_CF).ok_or(())?;
        let key = StoreKeyerBuilder::time_posting(
            self.bucket_id.ok_or(())?,
            timestamp_ms / TIME_SLICE_MS,
            StorePosting::shard(iid),
        );
        let mut delta = StorePosting::default();
        delta.insert(StorePosting::offset(iid));
        batch.merge_cf(postings_cf, key.as_bytes(), delta.encode());
        Ok(())
    }

    fn append_remove_time_iid(
        &self,
        batch: &mut WriteBatch,
        timestamp_ms: u64,
        iid: StoreObjectIID,
    ) -> Result<(), ()> {
        let store = self.store.as_ref().ok_or(())?;
        let postings_cf = store.database.cf_handle(POSTINGS_CF).ok_or(())?;
        let key = StoreKeyerBuilder::time_posting(
            self.bucket_id.ok_or(())?,
            timestamp_ms / TIME_SLICE_MS,
            StorePosting::shard(iid),
        );
        let Some(encoded) = store
            .database
            .get_cf(postings_cf, key.as_bytes())
            .or(Err(()))?
        else {
            return Ok(());
        };
        let mut posting = StorePosting::decode(&encoded)?;
        if posting.remove(StorePosting::offset(iid)) {
            if posting.is_empty() {
                batch.delete_cf(postings_cf, key.as_bytes());
            } else {
                batch.put_cf(postings_cf, key.as_bytes(), posting.encode());
            }
        }
        Ok(())
    }

    /// OID-to-IID mapper
    ///
    /// [IDX=2] ((oid)) ~> ((iid))
    pub fn get_oid_to_iid(&self, oid: StoreObjectOID<'a>) -> Result<Option<StoreObjectIID>, ()> {
        if let Some(ref store) = self.store {
            let store_key = StoreKeyerBuilder::oid_to_iid(self.bucket_id.ok_or(())?, oid);

            tracing::debug!("store get oid-to-iid: {}", store_key);

            match store.get(&store_key.as_bytes()) {
                Ok(Some(value)) => {
                    tracing::debug!(
                        "got oid-to-iid: {} with encoded value: {:?}",
                        store_key,
                        &*value
                    );

                    Self::decode_u32(&*value).or(Err(())).map(|value_decoded| {
                        tracing::debug!(
                            "got oid-to-iid: {} with decoded value: {:?}",
                            store_key,
                            &value_decoded
                        );

                        Some(value_decoded)
                    })
                }
                Ok(None) => {
                    tracing::debug!("no oid-to-iid found: {}", store_key);

                    Ok(None)
                }
                Err(err) => {
                    tracing::error!(
                        "error getting oid-to-iid: {} with trace: {}",
                        store_key,
                        err
                    );

                    Err(())
                }
            }
        } else {
            Ok(None)
        }
    }

    pub fn get_or_create_iid(&self, oid: StoreObjectOID<'a>) -> Result<StoreObjectIID, ()> {
        if let Some(iid) = self.get_oid_to_iid(oid)? {
            return Ok(iid);
        }
        let store = self.store.as_ref().ok_or(())?;
        let bucket_id = self.bucket_id.ok_or(())?;
        let iid = match self.get_meta_to_value(StoreMetaKey::IIDIncr)? {
            Some(StoreMetaValue::IIDIncr(value)) => value.checked_add(1).ok_or(())?,
            Some(_) => return Err(()),
            None => 0,
        };
        let mut batch = WriteBatch::default();
        batch.put(
            StoreKeyerBuilder::meta_to_value(bucket_id, &StoreMetaKey::IIDIncr).as_bytes(),
            iid.to_string().as_bytes(),
        );
        batch.put(
            StoreKeyerBuilder::oid_to_iid(bucket_id, oid).as_bytes(),
            Self::encode_u32(iid),
        );
        batch.put(
            StoreKeyerBuilder::iid_to_oid(bucket_id, iid).as_bytes(),
            oid.as_bytes(),
        );
        store.do_write(batch).or(Err(()))?;
        Ok(iid)
    }

    pub fn resolve_or_reserve_iid(
        &self,
        oid: StoreObjectOID<'a>,
    ) -> Result<(StoreObjectIID, bool), ()> {
        if let Some(iid) = self.get_oid_to_iid(oid)? {
            return Ok((iid, false));
        }
        let iid = match self.get_meta_to_value(StoreMetaKey::IIDIncr)? {
            Some(StoreMetaValue::IIDIncr(value)) => value.checked_add(1).ok_or(())?,
            Some(_) => return Err(()),
            None => 0,
        };
        Ok((iid, true))
    }

    pub fn set_oid_to_iid(&self, oid: StoreObjectOID<'a>, iid: StoreObjectIID) -> Result<(), ()> {
        if let Some(ref store) = self.store {
            let store_key = StoreKeyerBuilder::oid_to_iid(self.bucket_id.ok_or(())?, oid);

            tracing::debug!("store set oid-to-iid: {}", store_key);

            // Encode IID
            let iid_encoded = Self::encode_u32(iid);

            tracing::debug!(
                "store set oid-to-iid: {} with encoded value: {:?}",
                store_key,
                iid_encoded
            );

            store.put(&store_key.as_bytes(), &iid_encoded).or(Err(()))
        } else {
            Err(())
        }
    }

    pub fn delete_oid_to_iid(&self, oid: StoreObjectOID<'a>) -> Result<(), ()> {
        if let Some(ref store) = self.store {
            let store_key = StoreKeyerBuilder::oid_to_iid(self.bucket_id.ok_or(())?, oid);

            tracing::debug!("store delete oid-to-iid: {}", store_key);

            store.delete(&store_key.as_bytes()).or(Err(()))
        } else {
            Err(())
        }
    }

    /// IID-to-OID mapper
    ///
    /// [IDX=3] ((iid)) ~> ((oid))
    pub fn get_iid_to_oid(&self, iid: StoreObjectIID) -> Result<Option<String>, ()> {
        if let Some(ref store) = self.store {
            let store_key = StoreKeyerBuilder::iid_to_oid(self.bucket_id.ok_or(())?, iid);

            tracing::debug!("store get iid-to-oid: {}", store_key);

            match store.get(&store_key.as_bytes()) {
                Ok(Some(value)) => Ok(str::from_utf8(&value).ok().map(|value| value.to_string())),
                Ok(None) => Ok(None),
                Err(_) => Err(()),
            }
        } else {
            Ok(None)
        }
    }

    pub fn set_iid_to_oid(&self, iid: StoreObjectIID, oid: StoreObjectOID<'a>) -> Result<(), ()> {
        if let Some(ref store) = self.store {
            let store_key = StoreKeyerBuilder::iid_to_oid(self.bucket_id.ok_or(())?, iid);

            tracing::debug!("store set iid-to-oid: {}", store_key);

            store.put(&store_key.as_bytes(), oid.as_bytes()).or(Err(()))
        } else {
            Err(())
        }
    }

    pub fn delete_iid_to_oid(&self, iid: StoreObjectIID) -> Result<(), ()> {
        if let Some(ref store) = self.store {
            let store_key = StoreKeyerBuilder::iid_to_oid(self.bucket_id.ok_or(())?, iid);

            tracing::debug!("store delete iid-to-oid: {}", store_key);

            store.delete(&store_key.as_bytes()).or(Err(()))
        } else {
            Err(())
        }
    }

    /// IID-to-Terms mapper
    ///
    /// [IDX=4] ((iid)) ~> [((term))]
    pub fn get_iid_to_terms(&self, iid: StoreObjectIID) -> Result<Option<Vec<String>>, ()> {
        if let Some(ref store) = self.store {
            let store_key = StoreKeyerBuilder::iid_to_terms(self.bucket_id.ok_or(())?, iid);

            tracing::debug!("store get iid-to-terms: {}", store_key);

            match store.get(&store_key.as_bytes()) {
                Ok(Some(value)) => {
                    tracing::debug!(
                        "got iid-to-terms: {} with encoded value: {:?}",
                        store_key,
                        &*value
                    );

                    Self::decode_terms(&value).or(Err(())).map(|value_decoded| {
                        tracing::debug!(
                            "got iid-to-terms: {} with decoded value: {:?}",
                            store_key,
                            &value_decoded
                        );

                        if !value_decoded.is_empty() {
                            Some(value_decoded)
                        } else {
                            None
                        }
                    })
                }
                Ok(None) => Ok(None),
                Err(_) => Err(()),
            }
        } else {
            Ok(None)
        }
    }

    pub fn set_iid_to_terms(&self, iid: StoreObjectIID, terms: &[String]) -> Result<(), ()> {
        if let Some(ref store) = self.store {
            let store_key = StoreKeyerBuilder::iid_to_terms(self.bucket_id.ok_or(())?, iid);

            tracing::debug!("store set iid-to-terms: {}", store_key);

            // Encode term list into storage serialized format
            let terms_hashed_encoded = Self::encode_terms(terms)?;

            tracing::debug!(
                "store set iid-to-terms: {} with encoded value: {:?}",
                store_key,
                terms_hashed_encoded
            );

            store
                .put(&store_key.as_bytes(), &terms_hashed_encoded)
                .or(Err(()))
        } else {
            Err(())
        }
    }

    pub fn delete_iid_to_terms(&self, iid: StoreObjectIID) -> Result<(), ()> {
        if let Some(ref store) = self.store {
            let store_key = StoreKeyerBuilder::iid_to_terms(self.bucket_id.ok_or(())?, iid);

            tracing::debug!("store delete iid-to-terms: {}", store_key);

            store.delete(&store_key.as_bytes()).or(Err(()))
        } else {
            Err(())
        }
    }

    pub fn batch_flush_bucket(
        &self,
        iid: StoreObjectIID,
        oid: StoreObjectOID<'a>,
        iid_terms: &[String],
    ) -> Result<u32, ()> {
        let Some(ref store) = self.store else {
            return Err(());
        };
        let bucket_id = self.bucket_id.ok_or(())?;
        let mut count = 0;

        tracing::debug!(
            "store batch flush bucket: {} with terms: {:?}",
            iid,
            iid_terms
        );

        let mut batch = WriteBatch::default();
        batch.delete(StoreKeyerBuilder::oid_to_iid(bucket_id, oid).as_bytes());
        batch.delete(StoreKeyerBuilder::iid_to_oid(bucket_id, iid).as_bytes());
        batch.delete(StoreKeyerBuilder::iid_to_terms(bucket_id, iid).as_bytes());
        if let Some(timestamp_ms) = self.get_iid_timestamp(iid)? {
            self.append_remove_time_iid(&mut batch, timestamp_ms, iid)?;
            batch.delete(StoreKeyerBuilder::iid_to_timestamp(bucket_id, iid).as_bytes());
        }
        let document_cf = store.database.cf_handle(DOCUMENTS_CF).ok_or(())?;
        batch.delete_cf(
            document_cf,
            StoreKeyerBuilder::document(bucket_id, iid).as_bytes(),
        );
        for term in iid_terms {
            if self.append_remove_term_iid(&mut batch, term, iid)?.0 {
                count += 1;
            }
        }
        store.do_write(batch).or(Err(()))?;
        Ok(count)
    }

    pub fn batch_erase_bucket(&self) -> Result<u32, ()> {
        let Some(ref store) = self.store else {
            return Err(());
        };
        let Some(bucket_id) = self.bucket_id else {
            return Ok(0);
        };

        let mut batch = WriteBatch::default();
        for index in StoreKeyerBuilder::bucket_indexes()
            .iter()
            .filter(|index| !StoreKeyerBuilder::posting_indexes().contains(index))
        {
            let prefix = StoreKeyerBuilder::bucket_prefix(*index, bucket_id);
            let end = Self::prefix_end(&prefix).ok_or(())?;
            batch.delete_range(&prefix, &end);
        }
        let postings_cf = store.database.cf_handle(POSTINGS_CF).ok_or(())?;
        for index in StoreKeyerBuilder::posting_indexes() {
            let prefix = StoreKeyerBuilder::bucket_prefix(*index, bucket_id);
            let end = Self::prefix_end(&prefix).ok_or(())?;
            batch.delete_range_cf(postings_cf, &prefix, &end);
        }
        let document_prefix = StoreKeyerBuilder::document_prefix(bucket_id);
        let document_end = Self::prefix_end(&document_prefix).ok_or(())?;
        let document_cf = store.database.cf_handle(DOCUMENTS_CF).ok_or(())?;
        batch.delete_range_cf(document_cf, &document_prefix, &document_end);
        batch.delete(StoreKeyerBuilder::bucket_name_to_id(self.bucket.as_str()).as_bytes());
        batch.delete(StoreKeyerBuilder::bucket_id_to_name(bucket_id).as_bytes());
        store.do_write(batch).or(Err(()))?;

        Ok(1)
    }

    fn prefix_end(prefix: &[u8]) -> Option<StoreKeyerPrefix> {
        let mut end = prefix.to_vec();

        for index in (0..end.len()).rev() {
            if end[index] != u8::MAX {
                end[index] += 1;
                end.truncate(index + 1);
                return Some(end);
            }
        }

        None
    }

    fn encode_u32(decoded: u32) -> [u8; 4] {
        let mut encoded = [0; 4];

        LittleEndian::write_u32(&mut encoded, decoded);

        encoded
    }

    fn decode_u32(encoded: &[u8]) -> Result<u32, ()> {
        Cursor::new(encoded).read_u32::<LittleEndian>().or(Err(()))
    }

    fn encode_u64(decoded: u64) -> [u8; 8] {
        decoded.to_le_bytes()
    }

    fn decode_u64(encoded: &[u8]) -> Result<u64, ()> {
        encoded.try_into().map(u64::from_le_bytes).map_err(|_| ())
    }

    fn encode_terms(terms: &[String]) -> Result<Vec<u8>, ()> {
        let mut terms = terms.to_vec();
        terms.sort_unstable();
        terms.dedup();
        let mut encoded = Vec::new();
        for term in terms {
            let length = u32::try_from(term.len()).map_err(|_| ())?;
            encoded.extend_from_slice(&length.to_be_bytes());
            encoded.extend_from_slice(term.as_bytes());
        }
        Ok(encoded)
    }

    fn decode_terms(encoded: &[u8]) -> Result<Vec<String>, ()> {
        let mut terms = Vec::new();
        let mut cursor = 0;
        while cursor < encoded.len() {
            let length_end = cursor.checked_add(4).ok_or(())?;
            let length = u32::from_be_bytes(
                encoded
                    .get(cursor..length_end)
                    .ok_or(())?
                    .try_into()
                    .map_err(|_| ())?,
            ) as usize;
            let term_end = length_end.checked_add(length).ok_or(())?;
            let term = str::from_utf8(encoded.get(length_end..term_end).ok_or(())?)
                .map_err(|_| ())?
                .to_owned();
            terms.push(term);
            cursor = term_end;
        }
        Ok(terms)
    }
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_STORE_ID: AtomicUsize = AtomicUsize::new(0);

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
    fn it_proceeds_primitives() {
        let kv_store_config = test_kv_store_config();
        let kv_pool = StoreKVPool::new(kv_store_config);

        let store = kv_pool
            .acquire(StoreKVAcquireMode::Any, "c:test:2")
            .unwrap()
            .unwrap();

        assert!(store.get(&[0]).is_ok());
        assert!(store.put(&[0], &[1, 0, 0, 0]).is_ok());
        assert!(store.delete(&[0]).is_ok());
    }

    #[test]
    fn it_profiles_write_batches_by_column_family() {
        let kv_pool = StoreKVPool::new(test_kv_store_config());
        let store = kv_pool
            .acquire(StoreKVAcquireMode::Any, "c:test:profile")
            .unwrap()
            .unwrap();
        let documents_cf = store.database.cf_handle(DOCUMENTS_CF).unwrap();
        let postings_cf = store.database.cf_handle(POSTINGS_CF).unwrap();
        let mut batch = WriteBatch::default();
        batch.put(b"i", b"index");
        batch.put_cf(documents_cf, b"d", b"document");
        batch.merge_cf(postings_cf, b"p", StorePosting::default().encode());

        let profile = store.do_write_profiled(batch).unwrap();
        assert_eq!(profile.batch.puts, 2);
        assert_eq!(profile.batch.merges, 1);
        assert_eq!(profile.batch.bytes[DEFAULT_CF_ID as usize], 6);
        assert_eq!(profile.batch.bytes[DOCUMENTS_CF_ID as usize], 9);
        assert_eq!(profile.batch.bytes[POSTINGS_CF_ID as usize], 2);
    }

    #[test]
    fn it_proceeds_actions() {
        let kv_store_config = test_kv_store_config();
        let kv_pool = StoreKVPool::new(kv_store_config);

        let store = kv_pool
            .acquire(StoreKVAcquireMode::Any, "c:test:3")
            .unwrap();
        let action = StoreKVActionBuilder::access_or_create(
            StoreItemPart::from_str("b:test:3").unwrap(),
            store.clone(),
        );
        assert_eq!(action.bucket_id(), Some(1));
        let other_action = StoreKVActionBuilder::access_or_create(
            StoreItemPart::from_str("b:test:4").unwrap(),
            store.clone(),
        );
        assert_eq!(other_action.bucket_id(), Some(2));

        assert!(action.get_meta_to_value(StoreMetaKey::IIDIncr).is_ok());
        assert!(
            action
                .set_meta_to_value(StoreMetaKey::IIDIncr, StoreMetaValue::IIDIncr(1))
                .is_ok()
        );

        assert_eq!(action.insert_term_iid("hello", 1), Ok((true, 1)));
        assert_eq!(action.count_terms(), 1);
        assert_eq!(other_action.count_terms(), 0);
        assert_eq!(action.insert_term_iid("hello", 65_536), Ok((true, 2)));
        assert_eq!(action.insert_term_iid("hello", 1), Ok((false, 2)));
        assert_eq!(action.get_term_iids_desc("hello", 10), Ok(vec![65_536, 1]));
        assert_eq!(action.remove_term_iid("hello", 65_536), Ok((true, 1)));
        assert_eq!(action.remove_term_iid("hello", 65_536), Ok((false, 1)));
        assert_eq!(action.get_term_iids_desc("hello", 10), Ok(vec![1]));
        assert_eq!(action.remove_term_iid("hello", 1), Ok((true, 0)));
        assert_eq!(action.get_term_frequency("hello"), Ok(0));
        assert_eq!(action.get_term_iids_desc("hello", 10), Ok(vec![]));

        let first_batch = vec![(
            StoreDocument::new("bulk:1", 1_000, "bulk", serde_json::json!({})).unwrap(),
            vec!["bulk".to_owned()],
        )];
        let second_batch = vec![(
            StoreDocument::new("bulk:2", 2_000, "bulk", serde_json::json!({})).unwrap(),
            vec!["bulk".to_owned()],
        )];
        assert_eq!(
            action
                .batch_insert_fresh_documents(&first_batch, false)
                .unwrap()
                .frequencies,
            vec![("bulk".to_owned(), 1)]
        );
        assert_eq!(
            action
                .batch_insert_fresh_documents(&second_batch, false)
                .unwrap()
                .frequencies,
            vec![("bulk".to_owned(), 2)]
        );
        assert_eq!(action.get_term_frequency("bulk"), Ok(2));

        assert!(action.get_oid_to_iid(&"s".to_string()).is_ok());
        assert!(action.set_oid_to_iid(&"s".to_string(), 4).is_ok());
        assert!(action.set_oid_to_iid("another-object", 5).is_ok());
        assert_eq!(action.get_oid_to_iid("s"), Ok(Some(4)));
        assert_eq!(action.get_oid_to_iid("another-object"), Ok(Some(5)));
        assert!(action.delete_oid_to_iid(&"s".to_string()).is_ok());

        assert!(action.get_iid_to_oid(4).is_ok());
        assert!(action.set_iid_to_oid(4, &"s".to_string()).is_ok());
        assert!(action.delete_iid_to_oid(4).is_ok());

        assert!(action.get_iid_to_terms(4).is_ok());
        assert!(
            action
                .set_iid_to_terms(4, &["hello".to_owned(), "world".to_owned()])
                .is_ok()
        );
        assert!(action.delete_iid_to_terms(4).is_ok());
    }

    #[test]
    fn it_encodes_atom() {
        assert_eq!(StoreKVAction::encode_u32(0), [0, 0, 0, 0]);
        assert_eq!(StoreKVAction::encode_u32(1), [1, 0, 0, 0]);
        assert_eq!(StoreKVAction::encode_u32(45402), [90, 177, 0, 0]);
    }

    #[test]
    fn it_decodes_atom() {
        assert_eq!(StoreKVAction::decode_u32(&[0, 0, 0, 0]), Ok(0));
        assert_eq!(StoreKVAction::decode_u32(&[1, 0, 0, 0]), Ok(1));
        assert_eq!(StoreKVAction::decode_u32(&[90, 177, 0, 0]), Ok(45402));
    }

    #[test]
    fn it_round_trips_term_lists() {
        let terms = vec!["world".to_owned(), "hello".to_owned(), "hello".to_owned()];
        let encoded = StoreKVAction::encode_terms(&terms).unwrap();
        assert_eq!(
            StoreKVAction::decode_terms(&encoded),
            Ok(vec!["hello".to_owned(), "world".to_owned()])
        );
    }

    fn test_kv_store_config() -> Arc<crate::config::ConfigStoreKV> {
        let mut config = config::Config::builder()
            .add_source(config::File::from_str(
                crate::config::tests::defaults_toml(),
                config::FileFormat::Toml,
            ))
            .build()
            .unwrap()
            .get::<crate::config::ConfigStoreKV>("store.kv")
            .unwrap();
        config.path = std::env::temp_dir().join(format!(
            "sonic-kv-unit-{}-{}",
            std::process::id(),
            TEST_STORE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        Arc::new(config)
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
