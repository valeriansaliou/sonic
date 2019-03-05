// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use byteorder::{ByteOrder, NativeEndian, ReadBytesExt};
use rocksdb::{DBCompactionStyle, DBCompressionType, Error as DBError, Options as DBOptions, DB};
use std::io::Cursor;

use super::identifiers::*;
use super::item::StoreItemPart;
use super::keyer::StoreKeyerBuilder;
use crate::APP_CONF;

pub struct StoreKVPool;
pub struct StoreKVBuilder;

pub struct StoreKV {
    database: DB,
}

pub struct StoreKVActionBuilder;

pub struct StoreKVAction<'a> {
    store: StoreKV,
    bucket: StoreItemPart<'a>,
}

impl StoreKVPool {
    pub fn acquire(_target: &str) -> Result<StoreKV, DBError> {
        // TODO: pool and auto-close or auto-open if needed
        // TODO: keep it in a LAZY_STATIC global object
        StoreKVBuilder::new()
    }
}

impl StoreKVBuilder {
    pub fn new() -> Result<StoreKV, DBError> {
        Self::open().map(|db| StoreKV { database: db })
    }

    fn open() -> Result<DB, DBError> {
        debug!("opening key-value database");

        // Configure database options
        let db_options = Self::configure();

        // Acquire path to database
        // TODO: 1 database per collection
        // TODO: auto-close file descriptor if not used in a while, and re-open whenever needed
        let db_path = APP_CONF.store.kv.path.join("./collection");

        DB::open(&db_options, db_path)
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

impl StoreKVActionBuilder {
    pub fn new<'a>(bucket: StoreItemPart<'a>, store: StoreKV) -> StoreKVAction<'a> {
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

        match self.store.database.get(store_key.as_bytes()) {
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
            .database
            .put(store_key.as_bytes(), value_string.as_bytes())
            .or(Err(()))
    }

    /// Term-to-IIDs mapper
    ///
    /// [IDX=1] ((term)) ~> [((iid))]
    pub fn get_term_to_iids(&self, term: &str) -> Result<Option<Vec<StoreObjectIID>>, ()> {
        let store_key = StoreKeyerBuilder::term_to_iids(self.bucket.as_str(), term).to_string();

        debug!("store get term-to-iids: {}", store_key);

        match self.store.database.get(store_key.as_bytes()) {
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

    pub fn set_term_to_iids(&self, term: &str, iids: &[StoreObjectIID]) -> Result<(), ()> {
        let store_key = StoreKeyerBuilder::term_to_iids(self.bucket.as_str(), term).to_string();

        debug!("store set term-to-iids: {}", store_key);

        // Encode IID list into storage serialized format
        let iids_encoded = Self::encode_u64_list(iids);

        debug!(
            "store set term-to-iids: {} with encoded value: {:?}",
            store_key, iids_encoded
        );

        self.store
            .database
            .put(store_key.as_bytes(), &iids_encoded)
            .or(Err(()))
    }

    /// OID-to-IID mapper
    ///
    /// [IDX=2] ((oid)) ~> ((iid))
    pub fn get_oid_to_iid(&self, oid: &StoreObjectOID) -> Result<Option<StoreObjectIID>, ()> {
        let store_key = StoreKeyerBuilder::oid_to_iid(self.bucket.as_str(), oid).to_string();

        debug!("store get oid-to-iid: {}", store_key);

        match self.store.database.get(store_key.as_bytes()) {
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

        self.store
            .database
            .put(store_key.as_bytes(), &iid_encoded)
            .or(Err(()))
    }

    /// IID-to-OID mapper
    ///
    /// [IDX=3] ((iid)) ~> ((oid))
    pub fn get_iid_to_oid(&self, iid: StoreObjectIID) -> Result<Option<StoreObjectOID>, ()> {
        let store_key = StoreKeyerBuilder::iid_to_oid(self.bucket.as_str(), iid).to_string();

        debug!("store get iid-to-oid: {}", store_key);

        match self.store.database.get(store_key.as_bytes()) {
            Ok(Some(value)) => Ok(value.to_utf8().map(|value| value.to_string())),
            Ok(None) => Ok(None),
            Err(_) => Err(()),
        }
    }

    pub fn set_iid_to_oid(&self, iid: StoreObjectIID, oid: &StoreObjectOID) -> Result<(), ()> {
        let store_key = StoreKeyerBuilder::iid_to_oid(self.bucket.as_str(), iid).to_string();

        debug!("store set iid-to-oid: {}", store_key);

        self.store
            .database
            .put(store_key.as_bytes(), oid.as_bytes())
            .or(Err(()))
    }

    /// IID-to-Terms mapper
    ///
    /// [IDX=4] ((iid)) ~> [((term))]
    pub fn get_iid_to_terms(&self, iid: StoreObjectIID) -> Result<Option<Vec<String>>, ()> {
        let store_key = StoreKeyerBuilder::iid_to_terms(self.bucket.as_str(), iid).to_string();

        debug!("store get iid-to-terms: {}", store_key);

        match self.store.database.get(store_key.as_bytes()) {
            Ok(Some(value)) => Ok(if let Some(encoded) = value.to_utf8() {
                debug!(
                    "got iid-to-terms: {} with encoded value: {:?}",
                    store_key, encoded
                );

                let decoded: Vec<String> =
                    encoded.split(" ").into_iter().map(String::from).collect();

                debug!(
                    "got iid-to-terms: {} with decoded value: {:?}",
                    store_key, decoded
                );

                if decoded.is_empty() == false {
                    Some(decoded)
                } else {
                    None
                }
            } else {
                None
            }),
            Ok(None) => Ok(None),
            Err(_) => Err(()),
        }
    }

    pub fn set_iid_to_terms(&self, iid: StoreObjectIID, terms: &[String]) -> Result<(), ()> {
        let store_key = StoreKeyerBuilder::iid_to_terms(self.bucket.as_str(), iid).to_string();

        debug!("store set iid-to-terms: {}", store_key);

        // Encode terms (as 'space' is a special character, its safe to join them based on a space)
        let encoded = terms.join(" ");

        self.store
            .database
            .put(store_key.as_bytes(), encoded.as_bytes())
            .or(Err(()))
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
}
