// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use rocksdb::{DBCompactionStyle, DBCompressionType, Error as DBError, Options as DBOptions, DB};

use super::identifiers::*;
use crate::APP_CONF;

pub struct StoreKVBuilder;

pub struct StoreKV {
    database: DB,
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

impl StoreKV {
    /// Per-bucket mappings
    ///
    /// [IDX=0]  ((term))  ~>  [((iid))]
    /// [IDX=1]  ((oid))   ~>  ((iid))
    /// [IDX=2]  ((iid))   ~>  ((oid))
    /// [IDX=3]  ((iid))   ~>  [((term))]
    pub fn get_object_iid_to_oid(iid: &StoreObjectIID) -> Option<StoreObjectOID> {
        // TODO
        None
    }

    pub fn get_object_oid_to_iid(oid: StoreObjectOID) -> Option<StoreObjectIID> {
        // TODO
        None
    }

    pub fn set_object_id_association(iid: &StoreObjectIID, oid: StoreObjectOID) -> Result<(), ()> {
        // TODO
        Err(())
    }

    pub fn delete_object_id_association(iid: &StoreObjectIID) -> Result<(), ()> {
        // TODO
        Err(())
    }
}
