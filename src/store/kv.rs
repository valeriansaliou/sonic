// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use byteorder::{ByteOrder, NativeEndian, ReadBytesExt};
use rocksdb::{
    DBCompactionStyle, DBCompressionType, DBVector, Error as DBError, Options as DBOptions, DB,
};
use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::mem;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::SystemTime;

use super::identifiers::*;
use super::item::StoreItemPart;
use super::keyer::StoreKeyerBuilder;
use crate::APP_CONF;

pub struct StoreKVPool;
pub struct StoreKVBuilder;

pub struct StoreKV {
    database: DB,
    collection: String,
    last_used: Arc<RwLock<SystemTime>>,
}

pub struct StoreKVActionBuilder;

pub struct StoreKVAction<'a> {
    store: StoreKVBox,
    bucket: StoreItemPart<'a>,
}

type StoreKVBox = Arc<StoreKV>;

lazy_static! {
    pub static ref STORE_ACCESS_LOCK: Arc<RwLock<bool>> = Arc::new(RwLock::new(false));
    static ref STORE_WRITE_LOCK: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    static ref STORE_POOL: Arc<RwLock<HashMap<String, StoreKVBox>>> =
        Arc::new(RwLock::new(HashMap::new()));
}

impl StoreKVPool {
    pub fn acquire<'a, T: Into<&'a str>>(collection: T) -> Result<StoreKVBox, DBError> {
        let collection_str = collection.into();

        // Acquire general lock, and reference it in context
        // Notice: this prevents database to be opened while also erased; or 2 databases on the \
        //   same collection to be opened at the same time.
        let _write = STORE_WRITE_LOCK.lock().unwrap();

        // Acquire a thread-safe store pool reference in read mode
        let store_pool_read = STORE_POOL.read().unwrap();

        if let Some(store_kv) = store_pool_read.get(collection_str) {
            debug!(
                "kv store acquired from pool for collection: {}",
                collection_str
            );

            // Bump store last used date (avoids early janitor eviction)
            let mut last_used_value = store_kv.last_used.write().unwrap();

            mem::replace(&mut *last_used_value, SystemTime::now());

            // Perform an early drop of the lock (frees up write lock early)
            drop(last_used_value);

            Ok(store_kv.clone())
        } else {
            info!(
                "kv store not in pool for collection: {}, opening it",
                collection_str
            );

            match StoreKVBuilder::new(collection_str) {
                Ok(store_kv) => {
                    // Important: we need to drop the read reference first, to avoid dead-locking \
                    //   when acquiring the RWLock in write mode in this block.
                    drop(store_pool_read);

                    // Acquire a thread-safe store pool reference in write mode
                    let mut store_pool_write = STORE_POOL.write().unwrap();
                    let store_kv_box = Arc::new(store_kv);

                    store_pool_write.insert(collection_str.to_string(), store_kv_box.clone());

                    debug!(
                        "opened and cached store in pool for collection: {}",
                        collection_str
                    );

                    Ok(store_kv_box)
                }
                Err(err) => {
                    error!(
                        "failed opening store for collection: {} because: {}",
                        collection_str, err
                    );

                    Err(err)
                }
            }
        }
    }

    pub fn janitor() {
        debug!("scanning for kv store pool items to janitor");

        let mut store_pool_write = STORE_POOL.write().unwrap();
        let mut removal_register: Vec<String> = Vec::new();

        for (collection, store_kv) in store_pool_write.iter() {
            let last_used = store_kv.last_used.read().unwrap();

            if last_used.elapsed().unwrap().as_secs() >= APP_CONF.store.kv.pool.inactive_after {
                debug!(
                    "found expired kv store pool item: {} with last used time: {:?}",
                    &collection, last_used
                );

                removal_register.push(collection.to_owned());
            }
        }

        for collection in &removal_register {
            store_pool_write.remove(collection.as_str());
        }

        info!(
            "done scanning for kv store pool items to janitor, expired {} items, now has {} items",
            removal_register.len(),
            store_pool_write.len()
        );
    }
}

impl StoreKVBuilder {
    pub fn new(collection: &str) -> Result<StoreKV, DBError> {
        Self::open(collection).map(|db| StoreKV {
            database: db,
            collection: collection.to_string(),
            last_used: Arc::new(RwLock::new(SystemTime::now())),
        })
    }

    fn open(collection: &str) -> Result<DB, DBError> {
        debug!("opening key-value database");

        // Configure database options
        let db_options = Self::configure();

        // Open database at path for collection
        DB::open(&db_options, Self::path(collection))
    }

    fn path(collection: &str) -> PathBuf {
        APP_CONF.store.kv.path.join(collection)
    }

    fn configure() -> DBOptions {
        debug!("configuring key-value database");

        // Make database options
        let mut db_options = DBOptions::default();

        db_options.create_if_missing(true);
        db_options.set_use_fsync(false);
        db_options.set_compaction_style(DBCompactionStyle::Level);

        db_options.set_compression_type(if APP_CONF.store.kv.database.compress == true {
            DBCompressionType::Lz4
        } else {
            DBCompressionType::None
        });

        db_options.increase_parallelism(APP_CONF.store.kv.database.parallelism as i32);
        db_options.set_max_open_files(APP_CONF.store.kv.database.max_files as i32);
        db_options
            .set_max_background_compactions(APP_CONF.store.kv.database.max_compactions as i32);
        db_options.set_max_background_flushes(APP_CONF.store.kv.database.max_flushes as i32);

        db_options
    }
}

impl StoreKV {
    pub fn get(&self, key: &str) -> Result<Option<DBVector>, DBError> {
        self.database.get(key.as_bytes())
    }

    pub fn put(&self, key: &str, data: &[u8]) -> Result<(), DBError> {
        self.database.put(key.as_bytes(), data)
    }

    pub fn delete(&self, key: &str) -> Result<(), DBError> {
        self.database.delete(key.as_bytes())
    }
}

impl StoreKVActionBuilder {
    pub fn read<'a>(bucket: StoreItemPart<'a>, store: StoreKVBox) -> StoreKVAction<'a> {
        let action = Self::build(bucket, store);

        debug!("begin action read block");

        // TODO: handle the rwlock things on (collection, bucket) tuple (unpack bucket store \
        //   and return it); read lock; return a lock guard to ensure it auto-unlocks when caller \
        //   goes out of scope.

        debug!("began action read block");

        action
    }

    pub fn write<'a>(bucket: StoreItemPart<'a>, store: StoreKVBox) -> StoreKVAction<'a> {
        let action = Self::build(bucket, store);

        debug!("begin action write block");

        // TODO: handle the rwlock things on (collection, bucket) tuple (unpack bucket store \
        //   and return it); write lock; return a lock guard to ensure it auto-unlocks when caller \
        //   goes out of scope.

        debug!("began action write block");

        action
    }

    pub fn erase<'a, T: Into<&'a str>>(collection: T) -> Result<u64, ()> {
        let collection_str = collection.into();

        info!("erase requested on collection: {}", collection_str);

        // Acquire write + access locks, and reference it in context
        // Notice: write lock prevents database to be acquired from any context; while access lock \
        //   lets the erasure process wait that any thread using the database is done with work.
        let (_access, _write) = (
            STORE_ACCESS_LOCK.write().unwrap(),
            STORE_WRITE_LOCK.lock().unwrap(),
        );

        // Check if database storage directory exists
        let collection_path = StoreKVBuilder::path(collection_str);

        if collection_path.exists() == true {
            debug!(
                "collection store exists, erasing collection: {} at path: {:?}",
                collection_str, &collection_path
            );

            // Force a RocksDB database close
            {
                STORE_POOL.write().unwrap().remove(collection_str);
            }

            // Remove database storage from filesystem
            let erase_result = fs::remove_dir_all(&collection_path);

            if erase_result.is_ok() == true {
                debug!("done with collection erasure");

                Ok(1)
            } else {
                Err(())
            }
        } else {
            debug!(
                "collection store does not exist, consider already erased: {} at path: {:?}",
                collection_str, &collection_path
            );

            Ok(0)
        }
    }

    fn build<'a>(bucket: StoreItemPart<'a>, store: StoreKVBox) -> StoreKVAction<'a> {
        StoreKVAction {
            store: store,
            bucket: bucket,
        }
    }
}

impl<'a> StoreKVAction<'a> {
    /// Meta-to-Value mapper
    ///
    /// [IDX=0] ((meta)) ~> ((value))
    pub fn get_meta_to_value(&self, meta: StoreMetaKey) -> Result<Option<StoreMetaValue>, ()> {
        let store_key = StoreKeyerBuilder::meta_to_value(self.bucket.as_str(), &meta).to_string();

        debug!("store get meta-to-value: {}", store_key);

        match self.store.get(&store_key) {
            Ok(Some(value)) => {
                debug!("got meta-to-value: {}", store_key);

                Ok(if let Some(value) = value.to_utf8() {
                    match meta {
                        StoreMetaKey::IIDIncr => value
                            .parse::<StoreObjectIID>()
                            .ok()
                            .map(|value| StoreMetaValue::IIDIncr(value))
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
    }

    pub fn set_meta_to_value(&self, meta: StoreMetaKey, value: StoreMetaValue) -> Result<(), ()> {
        let store_key = StoreKeyerBuilder::meta_to_value(self.bucket.as_str(), &meta).to_string();

        debug!("store set meta-to-value: {}", store_key);

        let value_string = match value {
            StoreMetaValue::IIDIncr(iid_incr) => iid_incr.to_string(),
        };

        self.store
            .put(&store_key, value_string.as_bytes())
            .or(Err(()))
    }

    /// Term-to-IIDs mapper
    ///
    /// [IDX=1] ((term)) ~> [((iid))]
    pub fn get_term_to_iids(
        &self,
        term_hashed: StoreTermHashed,
    ) -> Result<Option<Vec<StoreObjectIID>>, ()> {
        let store_key =
            StoreKeyerBuilder::term_to_iids(self.bucket.as_str(), term_hashed).to_string();

        debug!("store get term-to-iids: {}", store_key);

        match self.store.get(&store_key) {
            Ok(Some(value)) => {
                debug!(
                    "got term-to-iids: {} with encoded value: {:?}",
                    store_key, &*value
                );

                Self::decode_u64_list(&*value)
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
    }

    pub fn set_term_to_iids(
        &self,
        term_hashed: StoreTermHashed,
        iids: &[StoreObjectIID],
    ) -> Result<(), ()> {
        let store_key =
            StoreKeyerBuilder::term_to_iids(self.bucket.as_str(), term_hashed).to_string();

        debug!("store set term-to-iids: {}", store_key);

        // Encode IID list into storage serialized format
        let iids_encoded = Self::encode_u64_list(iids);

        debug!(
            "store set term-to-iids: {} with encoded value: {:?}",
            store_key, iids_encoded
        );

        self.store.put(&store_key, &iids_encoded).or(Err(()))
    }

    pub fn delete_term_to_iids(&self, term_hashed: StoreTermHashed) -> Result<(), ()> {
        let store_key =
            StoreKeyerBuilder::term_to_iids(self.bucket.as_str(), term_hashed).to_string();

        debug!("store delete term-to-iids: {}", store_key);

        self.store.delete(&store_key).or(Err(()))
    }

    /// OID-to-IID mapper
    ///
    /// [IDX=2] ((oid)) ~> ((iid))
    pub fn get_oid_to_iid(&self, oid: &StoreObjectOID) -> Result<Option<StoreObjectIID>, ()> {
        let store_key = StoreKeyerBuilder::oid_to_iid(self.bucket.as_str(), oid).to_string();

        debug!("store get oid-to-iid: {}", store_key);

        match self.store.get(&store_key) {
            Ok(Some(value)) => {
                debug!(
                    "got oid-to-iid: {} with encoded value: {:?}",
                    store_key, &*value
                );

                Self::decode_u64(&*value).or(Err(())).map(|value_decoded| {
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
    }

    pub fn set_oid_to_iid(&self, oid: &StoreObjectOID, iid: StoreObjectIID) -> Result<(), ()> {
        let store_key = StoreKeyerBuilder::oid_to_iid(self.bucket.as_str(), oid).to_string();

        debug!("store set oid-to-iid: {}", store_key);

        // Encode IID
        let iid_encoded = Self::encode_u64(iid);

        debug!(
            "store set oid-to-iid: {} with encoded value: {:?}",
            store_key, iid_encoded
        );

        self.store.put(&store_key, &iid_encoded).or(Err(()))
    }

    pub fn delete_oid_to_iid(&self, oid: &StoreObjectOID) -> Result<(), ()> {
        let store_key = StoreKeyerBuilder::oid_to_iid(self.bucket.as_str(), oid).to_string();

        debug!("store delete oid-to-iid: {}", store_key);

        self.store.delete(&store_key).or(Err(()))
    }

    /// IID-to-OID mapper
    ///
    /// [IDX=3] ((iid)) ~> ((oid))
    pub fn get_iid_to_oid(&self, iid: StoreObjectIID) -> Result<Option<StoreObjectOID>, ()> {
        let store_key = StoreKeyerBuilder::iid_to_oid(self.bucket.as_str(), iid).to_string();

        debug!("store get iid-to-oid: {}", store_key);

        match self.store.get(&store_key) {
            Ok(Some(value)) => Ok(value.to_utf8().map(|value| value.to_string())),
            Ok(None) => Ok(None),
            Err(_) => Err(()),
        }
    }

    pub fn set_iid_to_oid(&self, iid: StoreObjectIID, oid: &StoreObjectOID) -> Result<(), ()> {
        let store_key = StoreKeyerBuilder::iid_to_oid(self.bucket.as_str(), iid).to_string();

        debug!("store set iid-to-oid: {}", store_key);

        self.store.put(&store_key, oid.as_bytes()).or(Err(()))
    }

    pub fn delete_iid_to_oid(&self, iid: StoreObjectIID) -> Result<(), ()> {
        let store_key = StoreKeyerBuilder::iid_to_oid(self.bucket.as_str(), iid).to_string();

        debug!("store delete iid-to-oid: {}", store_key);

        self.store.delete(&store_key).or(Err(()))
    }

    /// IID-to-Terms mapper
    ///
    /// [IDX=4] ((iid)) ~> [((term))]
    pub fn get_iid_to_terms(
        &self,
        iid: StoreObjectIID,
    ) -> Result<Option<Vec<StoreTermHashed>>, ()> {
        let store_key = StoreKeyerBuilder::iid_to_terms(self.bucket.as_str(), iid).to_string();

        debug!("store get iid-to-terms: {}", store_key);

        match self.store.get(&store_key) {
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

                        if value_decoded.is_empty() == false {
                            Some(value_decoded)
                        } else {
                            None
                        }
                    })
            }
            Ok(None) => Ok(None),
            Err(_) => Err(()),
        }
    }

    pub fn set_iid_to_terms(
        &self,
        iid: StoreObjectIID,
        terms_hashed: &[StoreTermHashed],
    ) -> Result<(), ()> {
        let store_key = StoreKeyerBuilder::iid_to_terms(self.bucket.as_str(), iid).to_string();

        debug!("store set iid-to-terms: {}", store_key);

        // Encode term list into storage serialized format
        let terms_hashed_encoded = Self::encode_u32_list(terms_hashed);

        debug!(
            "store set iid-to-terms: {} with encoded value: {:?}",
            store_key, terms_hashed_encoded
        );

        self.store
            .put(&store_key, &terms_hashed_encoded)
            .or(Err(()))
    }

    pub fn delete_iid_to_terms(&self, iid: StoreObjectIID) -> Result<(), ()> {
        let store_key = StoreKeyerBuilder::iid_to_terms(self.bucket.as_str(), iid).to_string();

        debug!("store delete iid-to-terms: {}", store_key);

        self.store.delete(&store_key).or(Err(()))
    }

    pub fn batch_flush_bucket(
        &self,
        iid: StoreObjectIID,
        oid: &StoreObjectOID,
        iid_terms_hashed: &Vec<StoreTermHashed>,
    ) -> Result<u64, ()> {
        let mut count = 0;

        debug!(
            "store batch flush bucket: {} with hashed terms: {:?}",
            iid, iid_terms_hashed
        );

        // Delete OID <> IID association
        match (
            self.delete_oid_to_iid(&oid),
            self.delete_iid_to_oid(iid),
            self.delete_iid_to_terms(iid),
        ) {
            (Ok(_), Ok(_), Ok(_)) => {
                // Delete IID from each associated term
                for iid_term in iid_terms_hashed {
                    if let Ok(Some(mut iid_term_iids)) = self.get_term_to_iids(*iid_term) {
                        if iid_term_iids.contains(&iid) == true {
                            count += 1;

                            iid_term_iids.remove_item(&iid);
                        }

                        if iid_term_iids.is_empty() == true {
                            self.delete_term_to_iids(*iid_term).ok();
                        } else {
                            self.set_term_to_iids(*iid_term, &iid_term_iids).ok();
                        }
                    }
                }

                Ok(count)
            }
            _ => Err(()),
        }
    }

    fn encode_u32(decoded: u32) -> [u8; 4] {
        let mut encoded = [0; 4];

        NativeEndian::write_u32(&mut encoded, decoded);

        encoded
    }

    fn decode_u32(encoded: &[u8]) -> Result<u32, ()> {
        Cursor::new(encoded).read_u32::<NativeEndian>().or(Err(()))
    }

    fn encode_u64(decoded: u64) -> [u8; 8] {
        let mut encoded = [0; 8];

        NativeEndian::write_u64(&mut encoded, decoded);

        encoded
    }

    fn decode_u64(encoded: &[u8]) -> Result<u64, ()> {
        Cursor::new(encoded).read_u64::<NativeEndian>().or(Err(()))
    }

    fn encode_u64_list(decoded: &[u64]) -> Vec<u8> {
        let mut encoded = Vec::new();

        for decoded_item in decoded {
            encoded.extend(&Self::encode_u64(*decoded_item))
        }

        encoded
    }

    fn decode_u64_list(encoded: &[u8]) -> Result<Vec<u64>, ()> {
        let mut decoded = Vec::new();

        for encoded_chunk in encoded.chunks(8) {
            if let Ok(decoded_chunk) = Self::decode_u64(encoded_chunk) {
                decoded.push(decoded_chunk);
            } else {
                return Err(());
            }
        }

        Ok(decoded)
    }

    fn encode_u32_list(decoded: &[u32]) -> Vec<u8> {
        let mut encoded = Vec::new();

        for decoded_item in decoded {
            encoded.extend(&Self::encode_u32(*decoded_item))
        }

        encoded
    }

    fn decode_u32_list(encoded: &[u8]) -> Result<Vec<u32>, ()> {
        let mut decoded = Vec::new();

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
