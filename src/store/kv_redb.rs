// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use byteorder::{ByteOrder, LittleEndian, ReadBytesExt};
use hashbrown::HashMap;

use redb::{Builder, Database, ReadableTable, TableDefinition};

use std::fmt;
use std::fs;
use std::io::{self, Cursor};
use std::path::{Path, PathBuf};
use std::str;
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, SystemTime};
use std::vec::Drain;

use super::generic::{
    StoreGeneric, StoreGenericActionBuilder, StoreGenericBuilder, StoreGenericPool,
};
use super::identifiers::*;
use super::item::StoreItemPart;
use super::keyer::{StoreKeyerBuilder, StoreKeyerHasher, StoreKeyerKey, StoreKeyerPrefix};
use crate::APP_CONF;

pub struct StoreKVPool;
pub struct StoreKVBuilder;

pub struct StoreKV {
    database: Database,
    last_used: Arc<RwLock<SystemTime>>,
    last_flushed: Arc<RwLock<SystemTime>>,
    pub lock: RwLock<bool>,
}

pub struct StoreKVActionBuilder;

pub struct StoreKVAction<'a> {
    store: Option<StoreKVBox>,
    bucket: StoreItemPart<'a>,
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

// const ATOM_HASH_RADIX: usize = 16;

lazy_static! {
    pub static ref STORE_ACCESS_LOCK: Arc<RwLock<bool>> = Arc::new(RwLock::new(false));
    static ref STORE_ACQUIRE_LOCK: Arc<Mutex<()>> = Arc::new(Mutex::new(()));
    static ref STORE_FLUSH_LOCK: Arc<Mutex<()>> = Arc::new(Mutex::new(()));
    static ref STORE_POOL: Arc<RwLock<HashMap<StoreKVKey, StoreKVBox>>> =
        Arc::new(RwLock::new(HashMap::new()));
}

const TABLE: TableDefinition<&[u8], Vec<u8>> = TableDefinition::new("sonic");

#[derive(Debug, thiserror::Error)]
pub enum RedbStoreKVError {
    #[error("{0}")]
    DatabaseError(#[from] redb::DatabaseError),

    #[error("{0}")]
    StorageError(#[from] redb::StorageError),

    #[error("{0}")]
    TransactionError(#[from] redb::TransactionError),

    #[error("{0}")]
    CommitError(#[from] redb::CommitError),

    #[error("{0}")]
    TableError(#[from] redb::TableError),
}

impl StoreKVPool {
    pub fn count() -> usize {
        STORE_POOL.read().unwrap().len()
    }

    pub fn acquire<'a, T: Into<&'a str>>(
        mode: StoreKVAcquireMode,
        collection: T,
    ) -> Result<Option<StoreKVBox>, ()> {
        let collection_str = collection.into();
        let pool_key = StoreKVKey::from_str(collection_str);

        // Freeze acquire lock, and reference it in context
        // Notice: this prevents two databases on the same collection to be opened at the same time.
        let _acquire = STORE_ACQUIRE_LOCK.lock().unwrap();

        // Acquire a thread-safe store pool reference in read mode
        let store_pool_read = STORE_POOL.read().unwrap();

        if let Some(store_kv) = store_pool_read.get(&pool_key) {
            Self::proceed_acquire_cache("kv", collection_str, pool_key, store_kv).map(Some)
        } else {
            info!(
                "kv store not in pool for collection: {} {}, opening it",
                collection_str, pool_key
            );

            // Important: we need to drop the read reference first, to avoid \
            //   dead-locking when acquiring the RWLock in write mode in this block.
            drop(store_pool_read);

            // Check if can open database?
            let can_open_db = if mode == StoreKVAcquireMode::OpenOnly {
                StoreKVBuilder::path(pool_key.collection_hash).exists()
            } else {
                true
            };

            // Open KV database? (ie. we do not need to create a new KV database file tree if \
            //   the database does not exist yet on disk and we are just looking to read data from \
            //   it)
            if can_open_db {
                Self::proceed_acquire_open("kv", collection_str, pool_key, &*STORE_POOL).map(Some)
            } else {
                Ok(None)
            }
        }
    }

    pub fn janitor() {
        Self::proceed_janitor(
            "kv",
            &*STORE_POOL,
            APP_CONF.store.kv.pool.inactive_after,
            &*STORE_ACCESS_LOCK,
        )
    }

    pub fn backup(path: &Path) -> Result<(), io::Error> {
        debug!("backing up all kv stores to path: {:?}", path);

        // Create backup directory (full path)
        fs::create_dir_all(path)?;

        // Proceed dump action (backup)
        Self::dump_action("backup", &*APP_CONF.store.kv.path, path, &Self::backup_item)
    }

    pub fn restore(path: &Path) -> Result<(), io::Error> {
        debug!("restoring all kv stores from path: {:?}", path);

        // Proceed dump action (restore)
        Self::dump_action(
            "restore",
            path,
            &*APP_CONF.store.kv.path,
            &Self::restore_item,
        )
    }

    pub fn flush(force: bool) {
        debug!("scanning for kv store pool items to flush to disk");

        // Acquire flush lock, and reference it in context
        // Notice: this prevents two flush operations to be executed at the same time.
        let _flush = STORE_FLUSH_LOCK.lock().unwrap();

        // Step 1: List keys to be flushed
        let mut keys_flush: Vec<StoreKVKey> = Vec::new();

        {
            // Acquire access lock (in blocking write mode), and reference it in context
            // Notice: this prevents store to be acquired from any context
            let _access = STORE_ACCESS_LOCK.write().unwrap();

            let store_pool_read = STORE_POOL.read().unwrap();

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
                        error!(
                            "kv key: {} last flush duration clock issue, zeroing: {}",
                            key, err
                        );

                        // Assuming a zero seconds fallback duration
                        Duration::from_secs(0)
                    })
                    .as_secs();

                if force || not_flushed_for >= APP_CONF.store.kv.database.flush_after {
                    info!(
                        "kv key: {} not flushed for: {} seconds, may flush",
                        key, not_flushed_for
                    );

                    keys_flush.push(*key);
                } else {
                    debug!(
                        "kv key: {} not flushed for: {} seconds, no flush",
                        key, not_flushed_for
                    );
                }
            }
        }

        // Exit trap: Nothing to flush yet? Abort there.
        if keys_flush.is_empty() {
            info!("no kv store pool items need to be flushed at the moment");

            return;
        }

        // Step 2: Flush KVs, one-by-one (sequential locking; this avoids global locks)
        let mut count_flushed = 0;

        {
            for key in &keys_flush {
                {
                    // Acquire access lock (in blocking write mode), and reference it in context
                    // Notice: this prevents store to be acquired from any context
                    let _access = STORE_ACCESS_LOCK.write().unwrap();

                    if let Some(store) = STORE_POOL.read().unwrap().get(key) {
                        debug!("kv key: {} flush started", key);

                        if let Err(err) = store.flush() {
                            error!("kv key: {} flush failed: {}", key, err);
                        } else {
                            count_flushed += 1;

                            debug!("kv key: {} flush complete", key);
                        }

                        // Bump 'last flushed' time
                        *store.last_flushed.write().unwrap() = SystemTime::now();
                    }
                }

                // Give a bit of time to other threads before continuing
                thread::yield_now();
            }
        }

        info!(
            "done scanning for kv store pool items to flush to disk (flushed: {})",
            count_flushed
        );
    }

    fn dump_action(
        action: &str,
        read_path: &Path,
        write_path: &Path,
        fn_item: &dyn Fn(&Path, &Path, &str) -> Result<(), io::Error>,
    ) -> Result<(), io::Error> {
        // Iterate on KV collections
        for collection in fs::read_dir(read_path)? {
            let collection = collection?;

            // Actual collection found?
            if let (Ok(collection_file_type), Some(collection_name)) =
                (collection.file_type(), collection.file_name().to_str())
            {
                if collection_file_type.is_dir() {
                    debug!("kv collection ongoing {}: {}", action, collection_name);

                    fn_item(write_path, &collection.path(), collection_name)?;
                }
            }
        }

        Ok(())
    }

    fn backup_item(
        _backup_path: &Path,
        _origin_path: &Path,
        _collection_name: &str,
    ) -> Result<(), io::Error> {
        Ok(())
    }

    fn restore_item(
        _backup_path: &Path,
        _origin_path: &Path,
        _collection_name: &str,
    ) -> Result<(), io::Error> {
        Ok(())
    }
}

impl StoreGenericPool<StoreKVKey, StoreKV, StoreKVBuilder> for StoreKVPool {}

impl StoreKVBuilder {
    fn open(collection_hash: StoreKVAtom) -> Result<Database, RedbStoreKVError> {
        debug!(
            "opening key-value database for collection: <{:x?}>",
            collection_hash
        );

        // Configure database options
        let builder = Self::configure();

        // Open database at path for collection
        let db = builder.create(Self::path(collection_hash))?;
        let write_txn = db.begin_write()?;
        {
            let mut _table = write_txn.open_table(TABLE)?;
        }
        write_txn.commit()?;

        Ok(db)
    }

    fn close(collection_hash: StoreKVAtom) {
        debug!(
            "closing key-value database for collection: <{:x?}>",
            collection_hash
        );

        let mut store_pool_write = STORE_POOL.write().unwrap();

        let collection_target = StoreKVKey::from_atom(collection_hash);

        store_pool_write.remove(&collection_target);
    }

    fn path(collection_hash: StoreKVAtom) -> PathBuf {
        APP_CONF
            .store
            .kv
            .path
            .join(format!("{:x?}.redb", collection_hash))
    }

    fn configure() -> Builder {
        debug!("configuring key-value database");

        // Make database options
        let mut db_options = Builder::new();

        db_options.set_cache_size(APP_CONF.store.kv.database.write_buffer * 1024);

        db_options
    }
}

impl StoreGenericBuilder<StoreKVKey, StoreKV> for StoreKVBuilder {
    fn build(pool_key: StoreKVKey) -> Result<StoreKV, ()> {
        Self::open(pool_key.collection_hash)
            .map(|db| {
                let now = SystemTime::now();

                StoreKV {
                    database: db,
                    last_used: Arc::new(RwLock::new(now)),
                    last_flushed: Arc::new(RwLock::new(now)),
                    lock: RwLock::new(false),
                }
            })
            .map_err(|err| {
                error!("failed opening kv: {}", err);
            })
    }
}

impl StoreKV {
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, RedbStoreKVError> {
        let read_txn = self.database.begin_read()?;
        let table = read_txn.open_table(TABLE)?;
        let v = table.get(key)?.map(|v| v.value());
        Ok(v)
    }

    pub fn put(&self, key: &[u8], data: &[u8]) -> Result<(), RedbStoreKVError> {
        let write_txn = self.database.begin_write()?;
        {
            let mut table = write_txn.open_table(TABLE)?;
            let _v = table.insert(key, data.to_vec())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn delete(&self, key: &[u8]) -> Result<(), RedbStoreKVError> {
        let write_txn = self.database.begin_write()?;
        {
            let mut table = write_txn.open_table(TABLE)?;
            let _v = table.remove(key)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn delete_range(
        &self,
        key_start: StoreKeyerKey,
        key_end: StoreKeyerKey,
    ) -> Result<(), RedbStoreKVError> {
        let write_txn = self.database.begin_write()?;
        {
            let mut table = write_txn.open_table(TABLE)?;
            let start: &[u8] = &key_start;
            let end: &[u8] = &key_end;
            let _v = table.drain::<&[u8]>((std::ops::Bound::Included(start), std::ops::Bound::Included(end)))?;
        }
        write_txn.commit()?;

        Ok(())
    }

    fn flush(&self) -> Result<(), RedbStoreKVError> {
        Ok(())
    }
}

impl StoreGeneric for StoreKV {
    fn ref_last_used(&self) -> &RwLock<SystemTime> {
        &self.last_used
    }
}

impl StoreKVActionBuilder {
    pub fn access(bucket: StoreItemPart, store: Option<StoreKVBox>) -> StoreKVAction {
        Self::build(bucket, store)
    }

    pub fn erase<'a, T: Into<&'a str>>(collection: T, bucket: Option<T>) -> Result<u32, ()> {
        Self::dispatch_erase("kv", collection, bucket)
    }

    fn build(bucket: StoreItemPart, store: Option<StoreKVBox>) -> StoreKVAction {
        StoreKVAction { store, bucket }
    }
}

impl StoreGenericActionBuilder for StoreKVActionBuilder {
    fn proceed_erase_collection(collection_str: &str) -> Result<u32, ()> {
        let collection_atom = StoreKeyerHasher::to_compact(collection_str);
        let collection_path = StoreKVBuilder::path(collection_atom);

        // Force a KV store close
        StoreKVBuilder::close(collection_atom);

        if collection_path.exists() {
            debug!(
                "kv collection store exists, erasing: {}/* at path: {:?}",
                collection_str, &collection_path
            );

            // Remove KV store storage from filesystem
            let erase_result = fs::remove_dir_all(&collection_path);

            if erase_result.is_ok() {
                debug!("done with kv collection erasure");

                Ok(1)
            } else {
                Err(())
            }
        } else {
            debug!(
                "kv collection store does not exist, consider already erased: {}/* at path: {:?}",
                collection_str, &collection_path
            );

            Ok(0)
        }
    }

    fn proceed_erase_bucket(_collection: &str, _bucket: &str) -> Result<u32, ()> {
        // This one is not implemented, as we need to acquire the collection; which would cause \
        //   a party-killer dead-lock.
        Err(())
    }
}

impl<'a> StoreKVAction<'a> {
    /// Meta-to-Value mapper
    ///
    /// [IDX=0] ((meta)) ~> ((value))
    pub fn get_meta_to_value(&self, meta: StoreMetaKey) -> Result<Option<StoreMetaValue>, ()> {
        if let Some(ref store) = self.store {
            let store_key = StoreKeyerBuilder::meta_to_value(self.bucket.as_str(), &meta);

            debug!("store get meta-to-value: {}", store_key);

            match store.get(&store_key.as_bytes()) {
                Ok(Some(value)) => {
                    debug!("got meta-to-value: {}", store_key);

                    Ok(if let Ok(value) = str::from_utf8(&value) {
                        match meta {
                            StoreMetaKey::IIDIncr => value
                                .parse::<StoreObjectIID>()
                                .ok()
                                .map(StoreMetaValue::IIDIncr)
                                .or(None),
                        }
                    } else {
                        None
                    })
                }
                Ok(None) => {
                    debug!("no meta-to-value found: {}", store_key);

                    Ok(None)
                }
                Err(err) => {
                    error!(
                        "error getting meta-to-value: {} with trace: {}",
                        store_key, err
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
            let store_key = StoreKeyerBuilder::meta_to_value(self.bucket.as_str(), &meta);

            debug!("store set meta-to-value: {}", store_key);

            let value_string = match value {
                StoreMetaValue::IIDIncr(iid_incr) => iid_incr.to_string(),
            };

            store
                .put(&store_key.as_bytes(), value_string.as_bytes())
                .or(Err(()))
        } else {
            Err(())
        }
    }

    /// Term-to-IIDs mapper
    ///
    /// [IDX=1] ((term)) ~> [((iid))]
    pub fn get_term_to_iids(
        &self,
        term_hashed: StoreTermHashed,
    ) -> Result<Option<Vec<StoreObjectIID>>, ()> {
        if let Some(ref store) = self.store {
            let store_key = StoreKeyerBuilder::term_to_iids(self.bucket.as_str(), term_hashed);

            debug!("store get term-to-iids: {}", store_key);

            match store.get(&store_key.as_bytes()) {
                Ok(Some(value)) => {
                    debug!(
                        "got term-to-iids: {} with encoded value: {:?}",
                        store_key, &*value
                    );

                    Self::decode_u32_list(&*value)
                        .or(Err(()))
                        .map(|value_decoded| {
                            debug!(
                                "got term-to-iids: {} with decoded value: {:?}",
                                store_key, &value_decoded
                            );

                            Some(value_decoded)
                        })
                }
                Ok(None) => {
                    debug!("no term-to-iids found: {}", store_key);

                    Ok(None)
                }
                Err(err) => {
                    error!(
                        "error getting term-to-iids: {} with trace: {}",
                        store_key, err
                    );

                    Err(())
                }
            }
        } else {
            Ok(None)
        }
    }

    pub fn set_term_to_iids(
        &self,
        term_hashed: StoreTermHashed,
        iids: &[StoreObjectIID],
    ) -> Result<(), ()> {
        if let Some(ref store) = self.store {
            let store_key = StoreKeyerBuilder::term_to_iids(self.bucket.as_str(), term_hashed);

            debug!("store set term-to-iids: {}", store_key);

            // Encode IID list into storage serialized format
            let iids_encoded = Self::encode_u32_list(iids);

            debug!(
                "store set term-to-iids: {} with encoded value: {:?}",
                store_key, iids_encoded
            );

            store.put(&store_key.as_bytes(), &iids_encoded).or(Err(()))
        } else {
            Err(())
        }
    }

    pub fn delete_term_to_iids(&self, term_hashed: StoreTermHashed) -> Result<(), ()> {
        if let Some(ref store) = self.store {
            let store_key = StoreKeyerBuilder::term_to_iids(self.bucket.as_str(), term_hashed);

            debug!("store delete term-to-iids: {}", store_key);

            store.delete(&store_key.as_bytes()).or(Err(()))
        } else {
            Err(())
        }
    }

    /// OID-to-IID mapper
    ///
    /// [IDX=2] ((oid)) ~> ((iid))
    pub fn get_oid_to_iid(&self, oid: StoreObjectOID<'a>) -> Result<Option<StoreObjectIID>, ()> {
        if let Some(ref store) = self.store {
            let store_key = StoreKeyerBuilder::oid_to_iid(self.bucket.as_str(), oid);

            debug!("store get oid-to-iid: {}", store_key);

            match store.get(&store_key.as_bytes()) {
                Ok(Some(value)) => {
                    debug!(
                        "got oid-to-iid: {} with encoded value: {:?}",
                        store_key, &*value
                    );

                    Self::decode_u32(&*value).or(Err(())).map(|value_decoded| {
                        debug!(
                            "got oid-to-iid: {} with decoded value: {:?}",
                            store_key, &value_decoded
                        );

                        Some(value_decoded)
                    })
                }
                Ok(None) => {
                    debug!("no oid-to-iid found: {}", store_key);

                    Ok(None)
                }
                Err(err) => {
                    error!(
                        "error getting oid-to-iid: {} with trace: {}",
                        store_key, err
                    );

                    Err(())
                }
            }
        } else {
            Ok(None)
        }
    }

    pub fn set_oid_to_iid(&self, oid: StoreObjectOID<'a>, iid: StoreObjectIID) -> Result<(), ()> {
        if let Some(ref store) = self.store {
            let store_key = StoreKeyerBuilder::oid_to_iid(self.bucket.as_str(), oid);

            debug!("store set oid-to-iid: {}", store_key);

            // Encode IID
            let iid_encoded = Self::encode_u32(iid);

            debug!(
                "store set oid-to-iid: {} with encoded value: {:?}",
                store_key, iid_encoded
            );

            store.put(&store_key.as_bytes(), &iid_encoded).or(Err(()))
        } else {
            Err(())
        }
    }

    pub fn delete_oid_to_iid(&self, oid: StoreObjectOID<'a>) -> Result<(), ()> {
        if let Some(ref store) = self.store {
            let store_key = StoreKeyerBuilder::oid_to_iid(self.bucket.as_str(), oid);

            debug!("store delete oid-to-iid: {}", store_key);

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
            let store_key = StoreKeyerBuilder::iid_to_oid(self.bucket.as_str(), iid);

            debug!("store get iid-to-oid: {}", store_key);

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
            let store_key = StoreKeyerBuilder::iid_to_oid(self.bucket.as_str(), iid);

            debug!("store set iid-to-oid: {}", store_key);

            store.put(&store_key.as_bytes(), oid.as_bytes()).or(Err(()))
        } else {
            Err(())
        }
    }

    pub fn delete_iid_to_oid(&self, iid: StoreObjectIID) -> Result<(), ()> {
        if let Some(ref store) = self.store {
            let store_key = StoreKeyerBuilder::iid_to_oid(self.bucket.as_str(), iid);

            debug!("store delete iid-to-oid: {}", store_key);

            store.delete(&store_key.as_bytes()).or(Err(()))
        } else {
            Err(())
        }
    }

    /// IID-to-Terms mapper
    ///
    /// [IDX=4] ((iid)) ~> [((term))]
    pub fn get_iid_to_terms(
        &self,
        iid: StoreObjectIID,
    ) -> Result<Option<Vec<StoreTermHashed>>, ()> {
        if let Some(ref store) = self.store {
            let store_key = StoreKeyerBuilder::iid_to_terms(self.bucket.as_str(), iid);

            debug!("store get iid-to-terms: {}", store_key);

            match store.get(&store_key.as_bytes()) {
                Ok(Some(value)) => {
                    debug!(
                        "got iid-to-terms: {} with encoded value: {:?}",
                        store_key, &*value
                    );

                    Self::decode_u32_list(&*value)
                        .or(Err(()))
                        .map(|value_decoded| {
                            debug!(
                                "got iid-to-terms: {} with decoded value: {:?}",
                                store_key, &value_decoded
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

    pub fn set_iid_to_terms(
        &self,
        iid: StoreObjectIID,
        terms_hashed: &[StoreTermHashed],
    ) -> Result<(), ()> {
        if let Some(ref store) = self.store {
            let store_key = StoreKeyerBuilder::iid_to_terms(self.bucket.as_str(), iid);

            debug!("store set iid-to-terms: {}", store_key);

            // Encode term list into storage serialized format
            let terms_hashed_encoded = Self::encode_u32_list(terms_hashed);

            debug!(
                "store set iid-to-terms: {} with encoded value: {:?}",
                store_key, terms_hashed_encoded
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
            let store_key = StoreKeyerBuilder::iid_to_terms(self.bucket.as_str(), iid);

            debug!("store delete iid-to-terms: {}", store_key);

            store.delete(&store_key.as_bytes()).or(Err(()))
        } else {
            Err(())
        }
    }

    pub fn batch_flush_bucket(
        &self,
        iid: StoreObjectIID,
        oid: StoreObjectOID<'a>,
        iid_terms_hashed: &[StoreTermHashed],
    ) -> Result<u32, ()> {
        let mut count = 0;

        debug!(
            "store batch flush bucket: {} with hashed terms: {:?}",
            iid, iid_terms_hashed
        );

        // Delete OID <> IID association
        match (
            self.delete_oid_to_iid(oid),
            self.delete_iid_to_oid(iid),
            self.delete_iid_to_terms(iid),
        ) {
            (Ok(_), Ok(_), Ok(_)) => {
                // Delete IID from each associated term
                for iid_term in iid_terms_hashed {
                    if let Ok(Some(mut iid_term_iids)) = self.get_term_to_iids(*iid_term) {
                        if iid_term_iids.contains(&iid) {
                            count += 1;

                            // Remove IID from list of IIDs
                            iid_term_iids.retain(|cur_iid| cur_iid != &iid);
                        }

                        let is_ok = if iid_term_iids.is_empty() {
                            self.delete_term_to_iids(*iid_term).is_ok()
                        } else {
                            self.set_term_to_iids(*iid_term, &iid_term_iids).is_ok()
                        };

                        if !is_ok {
                            return Err(());
                        }
                    }
                }

                Ok(count)
            }
            _ => Err(()),
        }
    }

    pub fn batch_truncate_object(
        &self,
        term_hashed: StoreTermHashed,
        term_iids_drain: Drain<StoreObjectIID>,
    ) -> Result<u32, ()> {
        let mut count = 0;

        for term_iid_drain in term_iids_drain {
            debug!("store batch truncate object iid: {}", term_iid_drain);

            // Nuke term in IID to Terms list
            if let Ok(Some(mut term_iid_drain_terms)) = self.get_iid_to_terms(term_iid_drain) {
                count += 1;

                term_iid_drain_terms.retain(|cur_term| cur_term != &term_hashed);

                // IID to Terms list is empty? Flush whole object.
                if term_iid_drain_terms.is_empty() {
                    // Acquire OID for this drained IID
                    if let Ok(Some(term_iid_drain_oid)) = self.get_iid_to_oid(term_iid_drain) {
                        if self
                            .batch_flush_bucket(term_iid_drain, &term_iid_drain_oid, &Vec::new())
                            .is_err()
                        {
                            error!(
                                "failed executing store batch truncate object batch-flush-bucket"
                            );
                        }
                    } else {
                        error!("failed getting store batch truncate object iid-to-oid");
                    }
                } else {
                    // Update IID to Terms list
                    if self
                        .set_iid_to_terms(term_iid_drain, &term_iid_drain_terms)
                        .is_err()
                    {
                        error!("failed setting store batch truncate object iid-to-terms");
                    }
                }
            }
        }

        Ok(count)
    }

    pub fn batch_erase_bucket(&self) -> Result<u32, ()> {
        if let Some(ref store) = self.store {
            // Generate all key prefix values (with dummy post-prefix values; we dont care)
            let (k_meta_to_value, k_term_to_iids, k_oid_to_iid, k_iid_to_oid, k_iid_to_terms) = (
                StoreKeyerBuilder::meta_to_value(self.bucket.as_str(), &StoreMetaKey::IIDIncr),
                StoreKeyerBuilder::term_to_iids(self.bucket.as_str(), 0),
                StoreKeyerBuilder::oid_to_iid(self.bucket.as_str(), &String::new()),
                StoreKeyerBuilder::iid_to_oid(self.bucket.as_str(), 0),
                StoreKeyerBuilder::iid_to_terms(self.bucket.as_str(), 0),
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
                debug!(
                    "store batch erase bucket: {} for prefix: {:?}",
                    self.bucket.as_str(),
                    key_prefix
                );

                // Generate start and end prefix for batch delete (in other words, the minimum \
                //   key value possible, and the highest key value possible)
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

                // Batch-delete keys matching range
                let _ = store.delete_range(key_prefix_start, key_prefix_end);
            }

            info!(
                "done processing store batch erase bucket: {}",
                self.bucket.as_str()
            );

            Ok(1)
        } else {
            Err(())
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

    fn encode_u32_list(decoded: &[u32]) -> Vec<u8> {
        // Pre-reserve required capacity as to avoid heap resizes (50% performance gain relative \
        //   to initializing this with a zero-capacity)
        let mut encoded = Vec::with_capacity(decoded.len() * 4);

        for decoded_item in decoded {
            encoded.extend(&Self::encode_u32(*decoded_item))
        }

        encoded
    }

    fn decode_u32_list(encoded: &[u8]) -> Result<Vec<u32>, ()> {
        // Pre-reserve required capacity as to avoid heap resizes (50% performance gain relative \
        //   to initializing this with a zero-capacity)
        let mut decoded = Vec::with_capacity(encoded.len() / 4);

        for encoded_chunk in encoded.chunks(4) {
            if let Ok(decoded_chunk) = Self::decode_u32(encoded_chunk) {
                decoded.push(decoded_chunk);
            } else {
                return Err(());
            }
        }

        Ok(decoded)
    }
}

impl StoreKVKey {
    pub fn from_atom(collection_hash: StoreKVAtom) -> StoreKVKey {
        StoreKVKey { collection_hash }
    }

    pub fn from_str(collection_str: &str) -> StoreKVKey {
        StoreKVKey {
            collection_hash: StoreKeyerHasher::to_compact(collection_str),
        }
    }
}

impl fmt::Display for StoreKVKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<{:x?}>", self.collection_hash)
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Deref;

    use crate::{AppArgs, APP_ARGS};

    use super::*;

    #[test_log::test]
    fn redb() {
        // sonic_init("./config.cfg");

        // let db = StoreKVPool::acquire(StoreKVAcquireMode::Any, "c:test:3")
        //     .unwrap()
        //     .unwrap()
        //     .database;
        // let write_txn = db.begin_write().unwrap();
        // {
        //     let mut table = write_txn.open_table(TABLE).unwrap();
        //     let key: StoreKeyerKey = [0, 0, 0, 0, 0, 0, 0, 0, 0];
        //     let k1: &[u8] = &key;
        //     table.insert(k1, key.to_vec()).unwrap();
        // }
    }

    fn sonic_init(config_path: &str) {
        // Ensure all statics are valid (a `deref` is enough to lazily initialize them)
        let app_args: &AppArgs = APP_ARGS.deref();
        let p = app_args as *const AppArgs as *mut AppArgs;
        unsafe {
            (*p).config = config_path.to_string();
        }

        let _ = APP_CONF.deref();
    }

    #[test_log::test]
    fn it_proceeds_actions() {
        // init
        sonic_init("./config.cfg");

        let store = StoreKVPool::acquire(StoreKVAcquireMode::Any, "c:test:3").unwrap();
        let action =
            StoreKVActionBuilder::access(StoreItemPart::from_str("b:test:3").unwrap(), store);

        assert!(action.get_meta_to_value(StoreMetaKey::IIDIncr).is_ok());
        assert!(action
            .set_meta_to_value(StoreMetaKey::IIDIncr, StoreMetaValue::IIDIncr(1))
            .is_ok());

        assert!(action.get_term_to_iids(1).is_ok());
        assert!(action.set_term_to_iids(1, &[0, 1, 2]).is_ok());
        assert!(action.delete_term_to_iids(1).is_ok());

        assert!(action.get_oid_to_iid(&"s".to_string()).is_ok());
        assert!(action.set_oid_to_iid(&"s".to_string(), 4).is_ok());
        assert!(action.delete_oid_to_iid(&"s".to_string()).is_ok());

        assert!(action.get_iid_to_oid(4).is_ok());
        assert!(action.set_iid_to_oid(4, &"s".to_string()).is_ok());
        assert!(action.delete_iid_to_oid(4).is_ok());

        assert!(action.get_iid_to_terms(4).is_ok());
        assert!(action.set_iid_to_terms(4, &[45402]).is_ok());
        assert!(action.delete_iid_to_terms(4).is_ok());
    }
}
