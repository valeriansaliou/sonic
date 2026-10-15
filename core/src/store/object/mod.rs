// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::fmt;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use crate::store::generic::*;
pub use crate::store::rocksdb::pool::{GenericKvStorePool, KvStoreId};
use crate::store::rocksdb::{
    GenericRocksDbStore, GenericRocksDbStorePub, RocksDbWriteOptionsBuilder,
};
use crate::store::types::*;
use crate::util::impl_int_enum;

pub type ObjectStorePool = GenericKvStorePool<ObjectStore>;

// MARK: - Store

pub struct ObjectStore {
    database: rocksdb::DB,
    last_used: RwLock<Instant>,
    last_flushed: RwLock<Instant>,
    pub lock: RwLock<()>,
    write_options_builder: RocksDbWriteOptionsBuilder,
}

impl GenericStore for ObjectStore {
    fn kind() -> &'static str {
        "Object"
    }

    fn ref_last_used(&self) -> &RwLock<Instant> {
        &self.last_used
    }
}

impl GenericRocksDbStorePub for ObjectStore {
    type PoolStats = ();
}

impl GenericRocksDbStore for ObjectStore {
    fn open(
        store_id: &KvStoreId,
        config: &crate::config::KvStoreConfig,
        dyn_config: Option<&dyn crate::DynamicConfig>,
    ) -> Result<rocksdb::DB, rocksdb::Error> {
        tracing::debug!("Opening Object store {store_id}");

        // Configure database options.
        let mut db_options = rocksdb::Options::from(&config.database);

        db_options.set_merge_operator_associative("object_merge", merge::object_merge_operator);

        if let Some(dyn_config) = dyn_config {
            dyn_config.update_object_store_options(&mut db_options, &config.database);
        }

        // Open database connection.
        rocksdb::DB::open(&db_options, config.store_path(store_id))
    }

    fn new(
        db: rocksdb::DB,
        write_options_builder: RocksDbWriteOptionsBuilder,
        _pool_stats: Arc<Self::PoolStats>,
    ) -> Self {
        let now = Instant::now();

        Self {
            database: db,
            last_used: RwLock::new(now),
            last_flushed: RwLock::new(now),
            lock: RwLock::new(()),
            write_options_builder,
        }
    }

    fn database(&self) -> &rocksdb::DB {
        &self.database
    }

    fn write_options_builder(&self) -> &RocksDbWriteOptionsBuilder {
        &self.write_options_builder
    }

    fn last_flushed(&self) -> &RwLock<Instant> {
        &self.last_flushed
    }

    fn lock(&self) -> &RwLock<()> {
        &self.lock
    }

    fn bucket_key_ranges(bucket: &Bucket) -> Vec<(Option<&'static str>, std::ops::Range<Vec<u8>>)> {
        vec![(None, keys::prefix_range(bucket))]
    }
}

impl Drop for ObjectStore {
    fn drop(&mut self) {
        // Flush to disk, to ensure no data is ever lost.
        self.flush().unwrap()
    }
}

// MARK: Key

// WARN: Don’t change values here, it would break the index! Only add new cases.
// NOTE: There’s only one case for now, but it’s there to future-proof the
//   Object store index structure. It’s considered the source of truth in
//   Sonic v2, we must keep it backward compatible.
impl_int_enum!(pub(super) ObjectStoreKeyDiscriminator(u8):
    Object (OBJECT) = 0,
);

// MARK: - Repository

#[derive(Debug)]
pub struct ObjectRepository<'a> {
    bucket: Bucket<'a>,
    store: &'a ObjectStore,
}

impl ObjectStore {
    pub fn to_repository<'a>(&'a self, bucket: Bucket<'a>) -> ObjectRepository<'a> {
        ObjectRepository {
            bucket,
            store: self,
        }
    }
}

impl<'a> ObjectRepository<'a> {
    #[inline]
    pub fn write(&self, batch: rocksdb::WriteBatch) -> Result<(), rocksdb::Error> {
        self.store.write(batch, Default::default())
    }

    pub fn put_object_original(
        &self,
        oid: StoreObjectOid,
        text: impl AsRef<[u8]>,
        batch: &mut rocksdb::WriteBatch,
    ) {
        let store_key = keys::object_key(&self.bucket, oid);

        batch.put(&store_key, text);
    }

    /// Appends text to an object.
    ///
    /// WARN: Make sure you have spaces or newlines between chunks, otherwise
    ///   snippet retrieval won’t work properly (terms might get merged).
    pub fn push_object_original(
        &self,
        oid: StoreObjectOid,
        text: impl AsRef<[u8]>,
        batch: &mut rocksdb::WriteBatch,
    ) {
        let store_key = keys::object_key(&self.bucket, oid);

        batch.merge(&store_key, text);
    }

    pub fn get_object_original(
        &self,
        oid: StoreObjectOid,
    ) -> Result<Option<Vec<u8>>, rocksdb::Error> {
        let store_key = keys::object_key(&self.bucket, oid);

        self.store.database.get(&store_key)
    }
}

mod keys {
    use crate::store::generic::KEY_SEPARATOR;
    use crate::store::{Bucket, StoreObjectOid};

    use super::ObjectStoreKeyDiscriminator as D;

    pub(super) fn prefix_range(bucket: &Bucket) -> std::ops::Range<Vec<u8>> {
        let bucket_bytes = bucket.as_bytes();

        let mut start = Vec::with_capacity(bucket_bytes.len() + 1);
        start.extend_from_slice(bucket_bytes);
        start.push(KEY_SEPARATOR);

        let mut end = Vec::with_capacity(bucket_bytes.len() + 1);
        end.extend_from_slice(bucket_bytes);
        end.push(KEY_SEPARATOR + 1);

        start..end
    }

    pub(super) fn object_key(bucket: &Bucket, oid: StoreObjectOid) -> Vec<u8> {
        let mut key = Vec::with_capacity(bucket.len() + 3 + oid.len());

        key.extend_from_slice(bucket.as_bytes());
        key.push(KEY_SEPARATOR);

        key.push(D::Object as u8);

        key.extend_from_slice(oid.as_bytes());

        key
    }
}

mod merge {
    pub(super) fn object_merge_operator(
        _key: &[u8],
        existing_val: Option<&[u8]>,
        operands: &rocksdb::MergeOperands,
    ) -> Option<Vec<u8>> {
        concat_bytes(existing_val, operands)
    }

    fn concat_bytes(
        existing_val: Option<&[u8]>,
        operands: &rocksdb::MergeOperands,
    ) -> Option<Vec<u8>> {
        let current: &[u8] = existing_val.unwrap_or_default();

        let operands_total_len = operands.iter().fold(0, |acc, op| acc + op.len());

        let mut res: Vec<u8> = Vec::with_capacity(current.len() + operands_total_len);

        res.extend_from_slice(current);

        for op in operands {
            res.extend_from_slice(op);
        }

        assert!(
            !res.is_empty(),
            "{existing_val:?}, {operands:?}",
            operands = operands.iter().collect::<Vec<_>>()
        );

        Some(res)
    }
}

// MARK: - Boilerplate

impl fmt::Debug for ObjectStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use crate::util::fmt::AsPrettyRwLock;

        // NOTE: Deconstructing to future-proof this function.
        let Self {
            database,
            last_used,
            last_flushed,
            lock,
            write_options_builder,
        } = self;

        f.debug_struct("ObjectStore")
            .field("database", database)
            .field("last_used", &AsPrettyRwLock(last_used))
            .field("last_flushed", &AsPrettyRwLock(last_flushed))
            .field("lock", &AsPrettyRwLock(lock))
            .field("write_options_builder", write_options_builder)
            .finish_non_exhaustive()
    }
}
