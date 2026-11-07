// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

pub(crate) mod encoding;
pub mod keys;
mod merge;

pub mod errors {
    pub use super::encoding::{BadLength, SizeError};
}

use std::num::NonZeroUsize;
use std::ops::Deref;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;
use std::{fmt, io};

use hashbrown::HashMap;
use lru::LruCache;
use rocksdb::{DB, WriteBatch};
use zerocopy::IntoBytes;

use crate::store::generic::*;
use crate::store::kv::merge::TF_TOMBSTONE;
pub use crate::store::rocksdb::pool::{GenericKvStorePool, KvStoreId};
use crate::store::rocksdb::{
    CF_CREATED_ON_OPEN, DEFAULT_CF_NAME, GenericRocksDbStore, GenericRocksDbStorePub,
    RocksDbWriteOptionsBuilder,
};
use crate::store::types::*;
use crate::util::{impl_transparent_wrapper_utils, usize_to_u64};

use self::encoding::*;
use self::keys::TypedKvStoreKey;
pub use self::merge::{AVERAGE_MERGE_RATIO, IidToTermsEntry, TermToIidsEntry};

/// A column family for data which need fast access during Ingest too, not just
/// Search. During a bulk ingestion, this column family is still flushed and
/// compacted normally, for faster access.
pub const READ_ON_PUSH_CF_NAME: &str = "read_on_push";

/// A column family whose data is never compressed. It’s used to avoid wasting
/// compute trying to compress IID-To-Terms for example, whose values consist
/// exclusively of hashed data.
///
/// See [Issue #435 “perf: Store `IID-To-Terms` in a separate column family with no compression”](https://github.com/valeriansaliou/sonic/issues/435) for context.
pub const INCOMPRESSIBLE_CF_NAME: &str = "incompressible";

// TODO: Find a more appropriate value, this one was chosen for no particular reason.
const IID_INCR_PER_OBJECT_CACHE_SIZE: NonZeroUsize = NonZeroUsize::new(1_000).unwrap();

const CF_NAMES: &[&str] = &[
    DEFAULT_CF_NAME,
    READ_ON_PUSH_CF_NAME,
    INCOMPRESSIBLE_CF_NAME,
];

pub type KvStorePool = GenericKvStorePool<KvStore>;

type ColumnFamilyRef<'a> = &'a rocksdb::ColumnFamily;

#[derive(Debug, Default)]
pub struct KvStorePoolStats {}

pub struct KvStore {
    database: DB,
    last_used: RwLock<Instant>,
    last_flushed: RwLock<Instant>,
    pub lock: RwLock<()>,
    write_options_builder: RocksDbWriteOptionsBuilder,

    /// Cache of `IidIncr` per bucket.
    ///
    /// Because atomic operations in RocksDB would be very expensive, it’s
    /// better for us to cache values as much as possible.
    ///
    /// In benchmarks, we saw a `~23%` throughput increase after this change.
    iid_incr_per_bucket: RwLock<HashMap<Vec<u8>, IidIncr>>,

    /// Cache of `IidIncr` per object.
    ///
    /// Because atomic operations in RocksDB would be very expensive, it’s
    /// better for us to cache values as much as possible.
    iid_incr_per_object: RwLock<LruCache<(Vec<u8>, GenericIid), IidIncr>>,

    /// A synchronization lock used to prevent race conditions when assigning
    /// new IIDs.
    ///
    /// This fixes the situation where two Sonic channels push to the same new
    /// OID at the same time. Both threads would read `None` via
    /// `get_oid_to_iid`, and assign two different IIDs to the same OID (the
    /// last one would take precedence, but everything tied to the first in the
    /// index would become dead data).
    ///
    /// This lock is used for both object OIDs and fragment OIDs out of
    /// simplicity. We might split it in two if we notice performace issues
    /// (unlikely).
    ///
    /// Note that we must use a `Mutex` as read guards cannot be upgraded and
    /// any non-atomic upgrade would either be very expensive (double checking)
    /// or still leave room for race conditions.
    oid_sync: Mutex<()>,

    pool_stats: Arc<KvStorePoolStats>,
}

#[derive(Clone, Copy)]
pub struct KvRepositoryRead<'a> {
    store: &'a KvStore,
    bucket: Bucket<'a>,
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct KvRepositoryReadOnly<'a>(KvRepositoryRead<'a>);

impl_transparent_wrapper_utils!(Deref for KvRepositoryReadOnly<'a>(KvRepositoryRead<'a>));

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct KvRepositoryReadWrite<'a>(KvRepositoryRead<'a>);

impl_transparent_wrapper_utils!(Deref for KvRepositoryReadWrite<'a>(KvRepositoryRead<'a>));

impl<'a> KvRepositoryReadWrite<'a> {
    #[inline]
    pub fn write_opt(
        &self,
        batch: WriteBatch,
        options: crate::store::rocksdb::WriteOptions,
    ) -> Result<(), rocksdb::Error> {
        self.store.write(batch, options)
    }

    #[inline]
    pub fn write(&self, batch: WriteBatch) -> Result<(), rocksdb::Error> {
        self.write_opt(batch, Default::default())
    }
}

impl GenericStore for KvStore {
    fn kind() -> &'static str {
        "KV"
    }

    fn ref_last_used(&self) -> &RwLock<Instant> {
        &self.last_used
    }
}

impl GenericRocksDbStorePub for KvStore {
    type PoolStats = KvStorePoolStats;
}

impl GenericRocksDbStore for KvStore {
    #[tracing::instrument(
        level = "debug",
        skip_all, fields(%store_id),
    )]
    fn open(
        store_id: &KvStoreId,
        config: &crate::config::KvStoreConfig,
        dyn_config: Option<&dyn crate::DynamicConfig>,
    ) -> Result<DB, rocksdb::Error> {
        tracing::debug!("Opening KV store…");

        let base_options = rocksdb::Options::from(&config.database);

        // Configure Column Families.
        let cf_descriptors = {
            let mut cf_descriptors = Vec::with_capacity(1);

            for &cf_name in CF_NAMES {
                let mut cf_options = base_options.clone();

                if let Some(dyn_config) = dyn_config {
                    dyn_config.update_kv_store_cf_options(
                        cf_name,
                        &mut cf_options,
                        &config.database,
                    );
                }

                match cf_name {
                    DEFAULT_CF_NAME => {
                        cf_options
                            .set_merge_operator_associative("kv_merge", merge::kv_merge_operator);
                    }
                    READ_ON_PUSH_CF_NAME => {
                        cf_options
                            .set_merge_operator_associative("kv_merge", merge::kv_merge_operator);

                        // Never disable auto-compactions for this CF.
                        cf_options.set_disable_auto_compactions(false);

                        // Never delay flushes.
                        cf_options.set_write_buffer_size(8 * 1024 * 1024);
                        cf_options.set_min_write_buffer_number_to_merge(1);
                        cf_options.set_max_write_buffer_number(8);
                    }
                    INCOMPRESSIBLE_CF_NAME => {
                        cf_options
                            .set_merge_operator_associative("kv_merge", merge::kv_merge_operator);

                        // Never compress this CF.
                        cf_options.set_compression_type(rocksdb::DBCompressionType::None);
                        cf_options.set_wal_compression_type(rocksdb::DBCompressionType::None);
                        cf_options.set_blob_compression_type(rocksdb::DBCompressionType::None);
                    }
                    _ => {}
                }

                cf_descriptors.push(rocksdb::ColumnFamilyDescriptor::new(cf_name, cf_options));
            }

            cf_descriptors
        };

        // Configure database options.
        let mut db_options = base_options;

        if let Some(dyn_config) = dyn_config {
            dyn_config.update_kv_store_options(&mut db_options, &config.database);
        }

        // Open database connection.
        DB::open_cf_descriptors(&db_options, config.store_path(&store_id), cf_descriptors)
    }

    fn new(
        db: DB,
        write_options_builder: RocksDbWriteOptionsBuilder,
        pool_stats: Arc<Self::PoolStats>,
    ) -> Self {
        let now = Instant::now();

        Self {
            database: db,
            last_used: RwLock::new(now),
            last_flushed: RwLock::new(now),
            lock: RwLock::new(()),
            write_options_builder,
            iid_incr_per_bucket: RwLock::new(HashMap::new()),
            iid_incr_per_object: RwLock::new(LruCache::sparse(IID_INCR_PER_OBJECT_CACHE_SIZE)),
            oid_sync: Mutex::new(()),
            pool_stats,
        }
    }

    fn database(&self) -> &DB {
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
        let prefix_range = keys::prefix_range(bucket);

        (CF_NAMES.iter())
            .map(|&cf_name| (Some(cf_name), prefix_range.clone()))
            .collect::<Vec<_>>()
    }

    fn on_batch_erase_bucket(&self, bucket: &Bucket) {
        (self.iid_incr_per_bucket.write().unwrap()).remove(&bucket.to_kv_key_part());

        // PERF: Let’s not care about `iid_incr_per_object`. It’s a LRU cache,
        //   values will get evicted at some point anyway. Walking the entire
        //   cache and removing keys would likely be more expensive than just
        //   leaving it untouched.
    }
}

impl Drop for KvStore {
    fn drop(&mut self) {
        // Flush to disk, to ensure no data is ever lost.
        self.flush().unwrap()
    }
}

impl KvStore {
    pub fn to_repository_read_only<'a>(&'a self, bucket: Bucket<'a>) -> KvRepositoryReadOnly<'a> {
        KvRepositoryReadOnly(KvRepositoryRead {
            bucket,
            store: self,
        })
    }

    pub fn to_repository_read_write<'a>(&'a self, bucket: Bucket<'a>) -> KvRepositoryReadWrite<'a> {
        KvRepositoryReadWrite(KvRepositoryRead {
            bucket,
            store: self,
        })
    }
}

impl<'db> KvRepositoryRead<'db> {
    #[tracing::instrument(
        level = "trace",
        skip_all, fields(%key),
    )]
    fn get_pinned<'a, K: TypedKvStoreKey>(
        &'a self,
        cf: Option<ColumnFamilyRef<'_>>,
        key: &K,
    ) -> Result<Option<K::Value<'db>>, ()>
    where
        K::Value<'db>: TryFromKvValue<rocksdb::DBPinnableSlice<'a>>,
    {
        let value_res = if let Some(cf) = cf {
            self.store.database.get_pinned_cf(cf, key.to_bytes())
        } else {
            self.store.database.get_pinned(key.to_bytes())
        };

        match value_res {
            Ok(Some(value)) => {
                tracing::debug!(%key, "Got encoded value: {:?}", value.deref());

                match K::Value::try_from_kv_value(value) {
                    Ok(value_decoded) => {
                        tracing::debug!(%key, "Got decoded value: {value_decoded:?}");

                        Ok(Some(value_decoded))
                    }
                    Err(error) => {
                        tracing::error!(%key, "Invalid value: {error}");

                        Err(())
                    }
                }
            }
            Ok(None) => {
                tracing::debug!(%key, "Found no value");

                Ok(None)
            }
            Err(error) => {
                tracing::error!(%key, "Error getting value: {error}");

                Err(())
            }
        }
    }
}

impl<'db> KvRepositoryRead<'db> {
    /// Note that because of the underlying use of `i64`, the max value is
    /// `i64::MAX` (hence `u64::MAX / 2`).
    pub fn get_object_count(&self) -> Result<u64, ()> {
        let store_key = keys::BucketMetaToValue {
            bucket: self.bucket,
            meta: keys::bucket_meta::ObjectCount,
        };

        self.get_pinned(None, &store_key).map(|opt| {
            opt.map_or_else(
                || {
                    tracing::debug!(
                        bucket = %self.bucket,
                        "ObjectCount not found in database, considering bucket empty"
                    );
                    0
                },
                |count| match u64::try_from(count.0.get()) {
                    Ok(n) => n,
                    Err(error) => {
                        tracing::debug!(
                            bucket = %self.bucket,
                            %error,
                            "Invalid ObjectCount in database, considering bucket empty"
                        );
                        0
                    }
                },
            )
        })
    }

    #[inline]
    pub fn get_term_to_iids<'a>(
        &'a self,
        term_hash: &StoreTermHash,
    ) -> Result<Option<impl ExactSizeIterator<Item = TermToIidsEntry> + 'a>, ()> {
        let store_key = keys::TermToIids {
            bucket: self.bucket,
            term_hash,
        };

        self.get_pinned(None, &store_key)
            .map(|opt| opt.map(|iter| iter.0))
    }

    pub fn get_document_frequency(&self, term_hash: &StoreTermHash) -> u64 {
        let store_key = keys::TermToIids {
            bucket: self.bucket,
            term_hash,
        };

        match self.store.database.get_pinned(store_key.to_bytes()) {
            Ok(Some(value)) => {
                usize_to_u64(value.len()) / usize_to_u64(size_of::<TermToIidsEntry>())
            }
            Ok(None) => {
                tracing::debug!(%store_key, ?term_hash, "term-to-iids not found");

                0
            }
            Err(error) => {
                tracing::error!(
                    %store_key,
                    ?term_hash,
                    "error getting term-to-iids: {error}"
                );

                0
            }
        }
    }

    #[inline]
    pub fn get_oid_to_iid(&self, oid: GenericOid) -> Result<Option<GenericIid>, ()> {
        let store_key = keys::OidToIid {
            bucket: self.bucket,
            oid,
        };

        let cf_handle = (self.store.database)
            .cf_handle(READ_ON_PUSH_CF_NAME)
            .expect(CF_CREATED_ON_OPEN);

        self.get_pinned(Some(cf_handle), &store_key)
    }

    #[inline]
    pub fn get_iid_to_oid(&self, iid: GenericIid) -> Result<Option<GenericOidOwned>, ()> {
        let store_key = keys::IidToOid {
            bucket: self.bucket,
            iid,
        };

        self.get_pinned(None, &store_key)
    }

    #[inline]
    pub fn get_iid_to_terms<'a>(
        &'a self,
        iid: GenericIid,
    ) -> Result<Option<impl ExactSizeIterator<Item = IidToTermsEntry> + 'a>, ()> {
        let store_key = keys::IidToTerms {
            bucket: self.bucket,
            iid,
        };

        let cf_handle = (self.store.database)
            .cf_handle(INCOMPRESSIBLE_CF_NAME)
            .expect(CF_CREATED_ON_OPEN);

        self.get_pinned(Some(cf_handle), &store_key)
            .map(|opt| opt.map(|iter| iter.0))
    }

    /// Note that because of the underlying use of `i64`, the max value is
    /// `i64::MAX` (hence `u64::MAX / 2`).
    pub fn get_fragment_count(&self, object_iid: GenericIid) -> Result<u64, ()> {
        let store_key = keys::ObjectMetaToValue {
            bucket: self.bucket,
            object_iid,
            meta: keys::object_meta::FragmentCount,
        };

        self.get_pinned(None, &store_key).map(|opt| {
            opt.map_or_else(
                || {
                    tracing::debug!(
                        %store_key,
                        "FragmentCount not found in database, considering bucket empty"
                    );
                    0
                },
                |count| match u64::try_from(count.0.get()) {
                    Ok(n) => n,
                    Err(error) => {
                        tracing::debug!(
                            %store_key,
                            %error,
                            "Invalid FragmentCount in database, considering bucket empty"
                        );
                        0
                    }
                },
            )
        })
    }

    #[inline]
    pub fn get_term_to_fragment_iids<'a>(
        &'a self,
        object_iid: GenericIid,
        term_hash: &StoreTermHash,
        ordering: Ordering,
    ) -> Result<Option<impl ExactSizeIterator<Item = TermToIidsEntry> + 'a>, ()> {
        let store_key = keys::TermToFragmentIids {
            bucket: self.bucket,
            object_iid,
            term_hash,
        };

        self.get_pinned(None, &store_key).map(|opt| {
            opt.map(|iter| {
                let mut iter = iter.0;
                iter.dir = match ordering {
                    Ordering::ReverseIngestionOrder => super::rocksdb::Direction::Forward,
                    Ordering::IngestionOrder => super::rocksdb::Direction::Reverse,
                };
                iter
            })
        })
    }

    #[inline]
    pub fn get_fragment_oid_to_iid(
        &self,
        object_oid: GenericOid,
        fragment_oid: GenericOid,
    ) -> Result<Option<GenericIid>, ()> {
        let store_key = keys::FragmentOidToIid {
            bucket: self.bucket,
            object_oid,
            fragment_oid,
        };

        let cf_handle = (self.store.database)
            .cf_handle(READ_ON_PUSH_CF_NAME)
            .expect(CF_CREATED_ON_OPEN);

        self.get_pinned(Some(cf_handle), &store_key)
    }

    #[inline]
    pub fn get_fragment_iid_to_oid(
        &self,
        object_iid: GenericIid,
        fragment_iid: GenericIid,
    ) -> Result<Option<String>, ()> {
        let store_key = keys::FragmentIidToOid {
            bucket: self.bucket,
            object_iid,
            fragment_iid,
        };

        self.get_pinned(None, &store_key)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Ordering {
    ReverseIngestionOrder,
    IngestionOrder,
}

impl<'a> KvRepositoryReadWrite<'a> {
    fn merge<K, V>(
        &self,
        cf: Option<ColumnFamilyRef<'_>>,
        key: &K,
        value: &V,
        batch: &mut WriteBatch,
    ) where
        K: TypedKvStoreKey,
        V: IntoBytes + zerocopy::Immutable + ?Sized,
    {
        let value = value.as_bytes();

        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(store_key = %key, ?value, "Updating value");
        } else {
            tracing::debug!(store_key = %key, "Updating value");
        }

        if let Some(cf) = cf {
            batch.merge_cf(cf, key.to_bytes(), value)
        } else {
            batch.merge(key.to_bytes(), value)
        }
    }

    fn put<K, V>(&self, cf: Option<ColumnFamilyRef<'_>>, key: &K, value: &V, batch: &mut WriteBatch)
    where
        K: TypedKvStoreKey,
        V: IntoBytes + zerocopy::Immutable + ?Sized,
    {
        let value = value.as_bytes();

        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(store_key = %key, ?value, "Setting value");
        } else {
            tracing::debug!(store_key = %key, "Setting value");
        }

        if let Some(cf) = cf {
            batch.put_cf(cf, key.to_bytes(), value)
        } else {
            batch.put(key.to_bytes(), value)
        }
    }

    fn delete<K>(&self, cf: Option<ColumnFamilyRef<'_>>, key: &K, batch: &mut WriteBatch)
    where
        K: TypedKvStoreKey,
    {
        tracing::debug!(store_key = %key, "Deleting value");

        if let Some(cf) = cf {
            batch.delete_cf(cf, key.to_bytes())
        } else {
            batch.delete(key.to_bytes())
        }
    }
}

impl<'db> KvRepositoryReadWrite<'db> {
    #[inline]
    fn update_object_count(&self, batch: &mut WriteBatch, diff: i64) {
        let store_key = keys::BucketMetaToValue {
            bucket: self.bucket,
            meta: keys::bucket_meta::ObjectCount,
        };

        self.merge(None, &store_key, &ObjectCount(diff.into()), batch);
    }

    fn get_new_iid(
        &self,
        batch: &mut WriteBatch,
    ) -> Result<GenericIid, Box<dyn std::error::Error>> {
        let store_key = keys::BucketMetaToValue {
            bucket: self.bucket,
            meta: keys::bucket_meta::IidIncr,
        };

        let cf_handle = (self.store.database)
            .cf_handle(READ_ON_PUSH_CF_NAME)
            .expect(CF_CREATED_ON_OPEN);

        let mut cache_guard = self.0.store.iid_incr_per_bucket.write().unwrap();

        let cache_key = self.bucket.to_kv_key_part();
        let iid_incr = match cache_guard.entry(cache_key) {
            hashbrown::hash_map::Entry::Occupied(mut entry) => {
                let iid_incr = entry.get_mut();

                iid_incr.increment();

                *iid_incr
            }
            hashbrown::hash_map::Entry::Vacant(entry) => {
                // Read from database (`cache_guard` ensures no race condition).
                let iid_incr = match self
                    .get_pinned(Some(cf_handle), &store_key)
                    .map_err(|()| io::Error::other("Error getting IidIncr"))?
                {
                    Some(mut iid_incr) => {
                        iid_incr.increment();
                        iid_incr
                    }
                    None => Default::default(),
                };

                // Populate the cache.
                *entry.insert(iid_incr)
            }
        };

        // Early release lock.
        drop(cache_guard);

        // Increment object count
        self.update_object_count(batch, 1);

        // Schedule a write to the database.
        // NOTE: If the `WriteBatch` doesn’t get written, we’ll have wasted one IID.
        //   We can’t really do otherwise without blocking locks for a long time.
        //   `Iid::MAX` should never get reached so it’s an acceptable trade-off.
        self.merge(Some(cf_handle), &store_key, iid_incr.as_bytes(), batch);

        Ok(iid_incr.0)
    }

    fn assign_new_iid(
        &self,
        oid: GenericOid<'_>,
    ) -> Result<GenericIid, Box<dyn std::error::Error>> {
        tracing::trace!("Must initialize push executor oid-to-iid and iid-to-oid");

        let mut batch = WriteBatch::new();

        // Bump last stored increment
        let iid = self.get_new_iid(&mut batch)?;

        // Associate OID <> IID (bidirectional)
        self.set_oid_to_iid(&mut batch, oid, iid);
        self.set_iid_to_oid(&mut batch, iid, oid);

        self.write(batch)?;

        Ok(iid)
    }

    /// Gets the IID for an object OID, creating a new one if necessary.
    ///
    /// Returns a boolean indicating if the IID is new.
    #[inline]
    pub fn get_or_assign_iid(
        &self,
        oid: GenericOid<'_>,
    ) -> Result<(GenericIid, bool), Box<dyn std::error::Error>> {
        let _oid_sync_guard = self.0.store.oid_sync.lock().unwrap();

        match (self.get_oid_to_iid(oid))
            .map_err(|()| io::Error::other("Error getting OID-To-IID"))?
        {
            Some(iid) => Ok((iid, false)),
            None => self.assign_new_iid(oid).map(|iid| (iid, true)),
        }
    }

    pub fn update_term_to_iid(
        &self,
        batch: &mut WriteBatch,
        term_hash: &StoreTermHash,
        iid: GenericIid,
        tf_delta: Option<u8>,
    ) {
        debug_assert!(
            !matches!(tf_delta, Some(TF_TOMBSTONE)),
            "Don’t update Term-To-IIDs with a tf of 0"
        );

        let store_key = keys::TermToIids {
            bucket: self.bucket,
            term_hash,
        };

        self.merge(
            None,
            &store_key,
            &TermToIidsEntry::new(iid, tf_delta.unwrap_or(TF_TOMBSTONE)),
            batch,
        );
    }

    pub fn set_oid_to_iid(&self, batch: &mut WriteBatch, oid: GenericOid, iid: GenericIid) {
        let store_key = keys::OidToIid {
            bucket: self.bucket,
            oid,
        };

        let cf_handle = (self.store.database)
            .cf_handle(READ_ON_PUSH_CF_NAME)
            .expect(CF_CREATED_ON_OPEN);

        self.put(Some(cf_handle), &store_key, &iid, batch)
    }

    pub fn delete_oid_to_iid(&self, batch: &mut WriteBatch, oid: GenericOid) {
        let store_key = keys::OidToIid {
            bucket: self.bucket,
            oid,
        };

        let cf_handle = (self.store.database)
            .cf_handle(READ_ON_PUSH_CF_NAME)
            .expect(CF_CREATED_ON_OPEN);

        self.delete(Some(cf_handle), &store_key, batch)
    }

    pub fn set_iid_to_oid(&self, batch: &mut WriteBatch, iid: GenericIid, oid: GenericOid) {
        let store_key = keys::IidToOid {
            bucket: self.bucket,
            iid,
        };

        self.put(None, &store_key, oid.as_bytes(), batch)
    }

    pub fn delete_iid_to_oid(&self, batch: &mut WriteBatch, iid: GenericIid) {
        let store_key = keys::IidToOid {
            bucket: self.bucket,
            iid,
        };

        self.delete(None, &store_key, batch)
    }

    pub fn set_iid_to_terms(
        &self,
        batch: &mut WriteBatch,
        iid: GenericIid,
        terms: impl ExactSizeIterator<Item = StoreTermHash>,
    ) {
        let store_key = keys::IidToTerms {
            bucket: self.bucket,
            iid,
        };

        // Encode iterator into storage serialized format.
        let terms = terms.map(IidToTermsEntry::new).collect::<Vec<_>>();

        let cf_handle = (self.store.database)
            .cf_handle(INCOMPRESSIBLE_CF_NAME)
            .expect(CF_CREATED_ON_OPEN);

        self.put(Some(cf_handle), &store_key, terms.as_slice(), batch)
    }

    #[inline]
    pub fn update_iid_to_terms(
        &self,
        batch: &mut WriteBatch,
        iid: GenericIid,
        new_terms: impl ExactSizeIterator<Item = StoreTermHash>,
    ) {
        let store_key = keys::IidToTerms {
            bucket: self.bucket,
            iid,
        };

        // Encode iterator into storage serialized format.
        // PERF: Collect here instead of running multiple `merge_cf` so we
        //   RocksDB has less calls to process in parallel (order) and so it
        //   allocates only once (we know the length of the iterator, RocksDB
        //   has no such info).
        let new_terms: Vec<IidToTermsEntry> =
            new_terms.into_iter().map(IidToTermsEntry::new).collect();

        let cf_handle = (self.store.database)
            .cf_handle(INCOMPRESSIBLE_CF_NAME)
            .expect(CF_CREATED_ON_OPEN);

        self.merge(Some(cf_handle), &store_key, new_terms.as_slice(), batch);
    }

    pub fn delete_iid_to_terms(&self, batch: &mut WriteBatch, iid: GenericIid) {
        let store_key = keys::IidToTerms {
            bucket: self.bucket,
            iid,
        };

        let cf_handle = (self.store.database)
            .cf_handle(INCOMPRESSIBLE_CF_NAME)
            .expect(CF_CREATED_ON_OPEN);

        self.delete(Some(cf_handle), &store_key, batch)
    }

    #[inline]
    fn update_fragment_count(&self, batch: &mut WriteBatch, object_iid: GenericIid, diff: i64) {
        let store_key = keys::ObjectMetaToValue {
            bucket: self.bucket,
            object_iid,
            meta: keys::object_meta::FragmentCount,
        };

        self.merge(None, &store_key, &FragmentCount(diff.into()), batch);
    }

    fn get_new_fragment_iid(
        &self,
        batch: &mut WriteBatch,
        object_iid: GenericIid,
    ) -> Result<GenericIid, Box<dyn std::error::Error>> {
        // Increment fragment count
        self.update_fragment_count(batch, object_iid, 1);

        let store_key = keys::ObjectMetaToValue {
            bucket: self.bucket,
            object_iid,
            meta: keys::object_meta::IidIncr,
        };

        let cf_handle = (self.store.database)
            .cf_handle(READ_ON_PUSH_CF_NAME)
            .expect(CF_CREATED_ON_OPEN);

        let mut cache_guard = self.0.store.iid_incr_per_object.write().unwrap();

        let cache_key = (self.bucket.to_kv_key_part(), object_iid);
        let iid_incr = match cache_guard.get_mut(&cache_key) {
            Some(iid_incr) => {
                iid_incr.increment();

                *iid_incr
            }
            None => {
                // Read from database (`cache_guard` ensures no race condition).
                let iid_incr = match self
                    .get_pinned(Some(cf_handle), &store_key)
                    .map_err(|()| io::Error::other("Error getting object IidIncr"))?
                {
                    Some(mut iid_incr) => {
                        iid_incr.increment();
                        iid_incr
                    }
                    None => Default::default(),
                };

                // Populate the cache.
                let previous = cache_guard.put(cache_key, iid_incr);
                debug_assert_eq!(previous, None);

                iid_incr
            }
        };

        // Early release lock.
        drop(cache_guard);

        // Schedule a write to the database.
        // NOTE: If the `WriteBatch` doesn’t get written, we’ll have wasted one IID.
        //   We can’t really do otherwise without blocking locks for a long time.
        //   `Iid::MAX` should never get reached so it’s an acceptable trade-off.
        self.merge(Some(cf_handle), &store_key, iid_incr.as_bytes(), batch);

        Ok(iid_incr.0)
    }

    fn assign_new_fragment_iid(
        &self,
        object_oid: GenericOid<'_>,
        object_iid: GenericIid,
        fragment_oid: GenericOid<'_>,
    ) -> Result<GenericIid, Box<dyn std::error::Error>> {
        let mut batch = WriteBatch::new();

        // Bump last stored increment
        let fragment_iid = self.get_new_fragment_iid(&mut batch, object_iid)?;

        // Associate OID <> IID (bidirectional)
        self.set_fragment_oid_to_iid(&mut batch, object_oid, fragment_oid, fragment_iid);
        self.set_fragment_iid_to_oid(&mut batch, object_iid, fragment_iid, fragment_oid);

        self.write(batch)?;

        Ok(fragment_iid)
    }

    /// Gets the IID for a fragment OID, creating a new one if necessary.
    ///
    /// Returns a boolean indicating if the IID is new.
    #[inline]
    pub fn get_fragment_iid(
        &self,
        object_oid: GenericOid<'_>,
        object_iid: GenericIid,
        fragment_oid: GenericOid<'_>,
    ) -> Result<(GenericIid, bool), Box<dyn std::error::Error>> {
        match (self.get_fragment_oid_to_iid(object_oid, fragment_oid))
            .map_err(|()| io::Error::other("Error getting OID-To-IID"))?
        {
            Some(iid) => Ok((iid, false)),
            None => self
                .assign_new_fragment_iid(object_oid, object_iid, fragment_oid)
                .map(|iid| (iid, true)),
        }
    }

    #[inline]
    pub fn update_term_to_fragment_iid(
        &self,
        batch: &mut WriteBatch,
        object_iid: GenericIid,
        term_hash: &StoreTermHash,
        iid: GenericIid,
        tf_delta: Option<u8>,
    ) {
        debug_assert!(
            !matches!(tf_delta, Some(TF_TOMBSTONE)),
            "Don’t update Term-To-IIDs with a tf of 0"
        );

        let store_key = keys::TermToFragmentIids {
            bucket: self.bucket,
            object_iid,
            term_hash,
        };

        self.merge(
            None,
            &store_key,
            &TermToIidsEntry::new(iid, tf_delta.unwrap_or(TF_TOMBSTONE)),
            batch,
        );
    }

    #[inline]
    pub fn set_fragment_oid_to_iid(
        &self,
        batch: &mut WriteBatch,
        object_oid: GenericOid,
        fragment_oid: GenericOid,
        fragment_iid: GenericIid,
    ) {
        let store_key = keys::FragmentOidToIid {
            bucket: self.bucket,
            object_oid,
            fragment_oid,
        };

        let cf_handle = (self.store.database)
            .cf_handle(READ_ON_PUSH_CF_NAME)
            .expect(CF_CREATED_ON_OPEN);

        self.put(Some(cf_handle), &store_key, &fragment_iid, batch)
    }

    #[inline]
    pub fn delete_fragment_oid_to_iid(
        &self,
        batch: &mut WriteBatch,
        object_oid: GenericOid,
        fragment_oid: GenericOid,
    ) {
        let store_key = keys::FragmentOidToIid {
            bucket: self.bucket,
            object_oid,
            fragment_oid,
        };

        let cf_handle = (self.store.database)
            .cf_handle(READ_ON_PUSH_CF_NAME)
            .expect(CF_CREATED_ON_OPEN);

        self.delete(Some(cf_handle), &store_key, batch)
    }

    #[inline]
    pub fn set_fragment_iid_to_oid(
        &self,
        batch: &mut WriteBatch,
        object_iid: GenericIid,
        fragment_iid: GenericIid,
        fragment_oid: GenericOid,
    ) {
        let store_key = keys::FragmentIidToOid {
            bucket: self.bucket,
            object_iid,
            fragment_iid,
        };

        self.put(None, &store_key, fragment_oid.as_bytes(), batch)
    }

    #[inline]
    pub fn delete_fragment_iid_to_oid(
        &self,
        batch: &mut WriteBatch,
        object_iid: GenericIid,
        fragment_iid: GenericIid,
    ) {
        let store_key = keys::FragmentIidToOid {
            bucket: self.bucket,
            object_iid,
            fragment_iid,
        };

        self.delete(None, &store_key, batch)
    }

    pub fn delete_object(
        &self,
        batch: &mut WriteBatch,
        iid: GenericIid,
        oid: GenericOid,
        object_terms: impl IntoIterator<Item = StoreTermHash>,
    ) -> u32 {
        let mut count: u32 = 0;

        tracing::debug!("store batch flush bucket: {iid:?}");

        // Delete OID <> IID association
        self.delete_oid_to_iid(batch, oid);
        self.delete_iid_to_oid(batch, iid);
        self.delete_iid_to_terms(batch, iid);

        // Delete IID from each associated term
        for term_hash in object_terms {
            self.update_term_to_iid(batch, &term_hash, iid, None);

            count = count.checked_add(1).unwrap();
        }

        // Decrement object count
        self.update_object_count(batch, -1);

        count
    }

    pub fn delete_fragment(
        &self,
        batch: &mut WriteBatch,
        object_iid: GenericIid,
        fragment_iid: GenericIid,
        object_oid: GenericOid,
        fragment_oid: GenericOid,
        object_terms: impl IntoIterator<Item = StoreTermHash>,
    ) -> u32 {
        let mut deleted_terms_count: u32 = 0;

        tracing::debug!("store batch flush fragment: {object_iid}:{fragment_iid}");

        // Delete OID <> IID association
        self.delete_fragment_oid_to_iid(batch, object_oid, fragment_oid);
        self.delete_fragment_iid_to_oid(batch, object_iid, fragment_iid);

        // Delete IID from each associated term
        // NOTE: To reduce the disk footprint of the Sonic index, we don’t store
        //   a list of terms per fragment. However, since we have a list of terms
        //   per object we can use that to efficiently remove the fragment from
        //   Term-To-Fragment-IID entries. If the fragment didn’t contain a
        //   term, the merge operator will just ignore it so we’re good.
        for term_hash in object_terms {
            self.update_term_to_fragment_iid(batch, object_iid, &term_hash, fragment_iid, None);

            deleted_terms_count = deleted_terms_count.checked_add(1).unwrap();
        }

        // Decrement fragment count
        self.update_fragment_count(batch, object_iid, -1);

        deleted_terms_count
    }
}

// MARK: - Tests

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn it_acquires_database() {
        let kv_store_config = test_kv_store_config();
        let kv_pool = KvStorePool::new(kv_store_config);

        assert!(kv_pool.acquire(true, "c:test:1".into(), None).is_ok());
    }

    #[test]
    fn it_janitors_database() {
        let kv_store_config = test_kv_store_config();
        let kv_pool = KvStorePool::new(kv_store_config);

        kv_pool.janitor(|_| true);
    }

    #[test]
    fn it_proceeds_actions() {
        let kv_store_config = test_kv_store_config();
        let kv_pool = KvStorePool::new(kv_store_config);

        let store = kv_pool
            .acquire(true, "c:test:3".into(), None)
            .unwrap()
            .unwrap();
        let action = store.to_repository_read_write("b:test:3".into());

        assert_eq!(action.get_term_to_iids(&1.into()).err(), None);
        assert!({
            let mut batch = WriteBatch::default();
            action.update_term_to_iid(&mut batch, &1.into(), 2.into(), Some(3));
            action.write(batch).is_ok()
        });

        assert!(action.get_oid_to_iid("s".into()).is_ok());
        assert!({
            let mut batch = WriteBatch::default();
            action.set_oid_to_iid(&mut batch, "s".into(), 4.into());
            action.write(batch).is_ok()
        });
        assert!({
            let mut batch = WriteBatch::default();
            action.delete_oid_to_iid(&mut batch, "s".into());
            action.write(batch).is_ok()
        });

        assert!(action.get_iid_to_oid(4.into()).is_ok());
        assert!({
            let mut batch = WriteBatch::default();
            action.set_iid_to_oid(&mut batch, 4.into(), "s".into());
            action.write(batch).is_ok()
        });
        assert!({
            let mut batch = WriteBatch::default();
            action.delete_iid_to_oid(&mut batch, 4.into());
            action.write(batch).is_ok()
        });

        assert!(action.get_iid_to_terms(4.into()).is_ok());
        assert!({
            let mut batch = WriteBatch::default();
            let terms = [StoreTermHash::from(45402)];
            action.set_iid_to_terms(&mut batch, 4.into(), terms.into_iter());
            action.write(batch).is_ok()
        });
        assert!({
            let mut batch = WriteBatch::default();
            action.delete_iid_to_terms(&mut batch, 4.into());
            action.write(batch).is_ok()
        });
    }

    // MARK: Helpers

    pub(in crate::store::kv) fn test_kv_store_config() -> Arc<crate::config::KvStoreConfig> {
        Arc::new(
            config::Config::builder()
                .add_source(config::File::from_str(
                    crate::config::tests::defaults_toml(),
                    config::FileFormat::Toml,
                ))
                .build()
                .unwrap()
                .get::<crate::config::KvStoreConfig>("store.kv")
                .unwrap(),
        )
    }
}

// MARK: - Boilerplate

impl fmt::Debug for KvStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use crate::util::fmt::{AsPrettyMutex, AsPrettyRwLock};

        // NOTE: Deconstructing to future-proof this function.
        let Self {
            database,
            last_used,
            last_flushed,
            lock,
            write_options_builder,
            iid_incr_per_bucket,
            iid_incr_per_object,
            oid_sync,
            pool_stats,
        } = self;

        f.debug_struct("KvStore")
            .field("database", database)
            .field("last_used", &AsPrettyRwLock(last_used))
            .field("last_flushed", &AsPrettyRwLock(last_flushed))
            .field("lock", &AsPrettyRwLock(lock))
            .field("write_options_builder", write_options_builder)
            .field("iid_incr_per_bucket", &AsPrettyRwLock(iid_incr_per_bucket))
            .field("iid_incr_per_object", &AsPrettyRwLock(iid_incr_per_object))
            .field("oid_sync", &AsPrettyMutex(oid_sync))
            .field("pool_stats", &pool_stats)
            .finish()
    }
}
