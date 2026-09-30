// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::fmt;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use hashbrown::HashMap;

use crate::store::generic::*;
use crate::store::rocksdb::GenericRocksDbStore;
pub use crate::store::rocksdb::pool::{GenericKvStorePool, KvStoreId};
use crate::store::types::*;

pub type ObjectStorePool = GenericKvStorePool<ObjectStore>;

// MARK: - Store

pub struct ObjectStore {
    database: rocksdb::DB,
    last_used: RwLock<Instant>,
    last_flushed: RwLock<Instant>,
    pub lock: RwLock<()>,
    kv_store_config: Arc<crate::config::KvStoreConfig>,

    /// Cache of `IIDIncr` per bucket, removing the need for countless reads
    /// while ingesting new data.
    ///
    /// This cache is particularly effective with large memtables, which often
    /// have bad read performance.
    ///
    /// In benchmarks, we saw a `~23%` throughput increase after this change.
    iid_incr_per_bucket: RwLock<HashMap<Vec<u8>, StoreObjectIid>>,
}

impl GenericStore for ObjectStore {
    fn kind() -> &'static str {
        "Object"
    }

    fn ref_last_used(&self) -> &RwLock<Instant> {
        &self.last_used
    }
}

impl GenericRocksDbStore for ObjectStore {
    fn new(db: rocksdb::DB, config: Arc<crate::config::KvStoreConfig>) -> Self {
        let now = Instant::now();

        Self {
            database: db,
            last_used: RwLock::new(now),
            last_flushed: RwLock::new(now),
            lock: RwLock::new(()),
            kv_store_config: config,
            iid_incr_per_bucket: RwLock::new(HashMap::new()),
        }
    }

    fn configure(db_options: &mut rocksdb::Options) {
        db_options.set_merge_operator_associative("object_merge", merge::object_merge_operator);
    }

    fn database(&self) -> &rocksdb::DB {
        &self.database
    }

    fn config(&self) -> &crate::config::KvStoreConfig {
        &self.kv_store_config
    }

    fn last_flushed(&self) -> &RwLock<Instant> {
        &self.last_flushed
    }

    fn lock(&self) -> &RwLock<()> {
        &self.lock
    }

    fn bucket_key_range(bucket: &Bucket) -> std::ops::Range<Vec<u8>> {
        keys::prefix_range(bucket)
    }
}

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
        self.store.write(batch)
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
        let mut key = Vec::with_capacity(bucket.len() + 1 + oid.len());

        key.extend_from_slice(bucket.as_bytes());
        key.push(KEY_SEPARATOR);

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
            // NOTE: We don’t care about the configuration,
            //   we can see it elsewhere if needed.
            kv_store_config: _kv_store_config,
            iid_incr_per_bucket,
        } = self;

        f.debug_struct("ObjectStore")
            .field("database", database)
            .field("last_used", &AsPrettyRwLock(last_used))
            .field("last_flushed", &AsPrettyRwLock(last_flushed))
            .field("lock", &AsPrettyRwLock(lock))
            .field("iid_incr_per_bucket", &AsPrettyRwLock(iid_incr_per_bucket))
            .finish_non_exhaustive()
    }
}
