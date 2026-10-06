// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod keys;
mod merge;

use std::sync::{Arc, RwLock};
use std::time::Instant;
use std::{fmt, io};

use hashbrown::HashMap;
use rocksdb::{DB, WriteBatch};

use crate::store::generic::*;
use crate::store::rocksdb::GenericRocksDbStore;
pub use crate::store::rocksdb::pool::{GenericKvStorePool, KvStoreId};
use crate::store::types::*;

use super::encoding::*;

use self::keys::KvStoreKey;
pub use self::keys::StoreMetaKey;

pub type KvStorePool = GenericKvStorePool<KvStore>;

pub struct KvStore {
    database: DB,
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

pub struct KvRepositoryReadOnly<'a> {
    bucket: Bucket<'a>,
    store: &'a KvStore,
}

pub struct KvRepositoryReadWrite<'a> {
    bucket: Bucket<'a>,
    store: &'a KvStore,
}

impl KvStore {
    fn get_new_iid(
        &self,
        bucket: Bucket,
        batch: &mut WriteBatch,
    ) -> Result<StoreObjectIid, Box<dyn std::error::Error>> {
        let mut write_guard = self.iid_incr_per_bucket.write().unwrap();

        let cache_key = bucket.to_bytes();
        let iid = *write_guard
            .entry(cache_key)
            .and_modify(|iid| *iid = iid.saturating_add(1))
            .or_insert(StoreObjectIid::from(0));

        // Early release lock.
        drop(write_guard);

        let store_key = KvStoreKey::meta_to_value(&bucket, &StoreMetaKey::IIDIncr);
        batch.merge(store_key, encode_u64_counter(iid.into()));

        Ok(iid)
    }
}

impl<'a> KvRepositoryReadWrite<'a> {
    #[inline]
    pub fn write(&self, batch: WriteBatch) -> Result<(), rocksdb::Error> {
        self.store.write(batch)
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

impl GenericRocksDbStore for KvStore {
    fn new(db: DB, config: Arc<crate::config::KvStoreConfig>) -> Self {
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
        db_options.set_merge_operator_associative("kv_merge", merge::kv_merge_operator);
    }

    fn database(&self) -> &DB {
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
        KvStoreKey::prefix_range(bucket)
    }

    fn on_batch_erase_bucket(&self, bucket: &Bucket) {
        (self.iid_incr_per_bucket.write().unwrap()).remove(&bucket.to_bytes());
    }
}

impl KvStore {
    pub fn to_repository_read_only<'a>(&'a self, bucket: Bucket<'a>) -> KvRepositoryReadOnly<'a> {
        KvRepositoryReadOnly {
            bucket,
            store: self,
        }
    }

    pub fn to_repository_read_write<'a>(&'a self, bucket: Bucket<'a>) -> KvRepositoryReadWrite<'a> {
        KvRepositoryReadWrite {
            bucket,
            store: self,
        }
    }
}

impl<'a> KvRepositoryReadOnly<'a> {
    /// Meta-to-Value mapper
    ///
    /// [IDX=0] ((meta)) ~> ((value))
    pub fn get_meta_to_value<T: std::str::FromStr>(
        &self,
        meta: StoreMetaKey,
    ) -> Result<Option<T>, ()> {
        let store_key = KvStoreKey::meta_to_value(&self.bucket, &meta);

        tracing::debug!("store get meta-to-value: {store_key}");

        match self.store.database.get(&store_key) {
            Ok(Some(value)) => {
                tracing::debug!("got meta-to-value: {store_key}");

                Ok(str::from_utf8(&value).map_or(None, |value| value.parse::<T>().ok()))
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

    /// Note that because of the underlying use of `i64`, the max value is
    /// `i64::MAX` (hence `u64::MAX / 2`).
    pub fn get_object_count(&self) -> Result<u64, Box<dyn std::error::Error>> {
        let bucket = self.bucket;

        let store_key = KvStoreKey::meta_to_value(&bucket, &StoreMetaKey::ObjectCount);
        let value = self.store.database.get(&store_key)?;

        match value {
            Some(bytes) => match bytes.split_first_chunk::<8>() {
                Some((chunk, [])) => {
                    let count = decode_i64_counter(*chunk);

                    tracing::debug!(?bucket, ?count, "Read ObjectCount from database");

                    match u64::try_from(count) {
                        Ok(count) => Ok(count),
                        Err(error) => Err(Box::new(io::Error::other(format!(
                            "Invalid ObjectCount value in bucket {bucket:?}: {error:?}",
                        )))),
                    }
                }
                Some(_) => Err(Box::new(io::Error::other(format!(
                    "Invalid ObjectCount value in bucket {bucket:?}: too many bytes",
                )))),
                None => Err(Box::new(io::Error::other(format!(
                    "Invalid ObjectCount value in bucket {bucket:?}: missing bytes",
                )))),
            },
            None => {
                tracing::debug!(
                    ?bucket,
                    "ObjectCount not found in database, considering bucket empty"
                );
                Ok(0)
            }
        }
    }

    /// Term-to-IIDs mapper
    ///
    /// [IDX=1] ((term)) ~> [((iid))]
    pub fn get_term_to_iids(
        &self,
        term_hash: &StoreTermHash,
    ) -> Result<Option<Vec<StoreObjectIid>>, ()> {
        let store_key = KvStoreKey::term_to_iids(&self.bucket, term_hash);

        tracing::debug!("store get term-to-iids: {store_key}");

        match self.store.database.get(&store_key) {
            Ok(Some(value)) => {
                tracing::debug!("got term-to-iids: {store_key} with encoded value: {value:?}");

                decode_iids_list(&value).map(|value_decoded| {
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
    pub fn get_oid_to_iid(&self, oid: StoreObjectOid) -> Result<Option<StoreObjectIid>, ()> {
        let store_key = KvStoreKey::oid_to_iid(&self.bucket, oid);

        tracing::debug!("store get oid-to-iid: {store_key}");

        match self.store.database.get(&store_key) {
            Ok(Some(value)) => {
                tracing::debug!("got oid-to-iid: {store_key} with encoded value: {value:?}");

                try_decode_iid(&value).map(|value_decoded| {
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
    pub fn get_iid_to_oid(&self, iid: StoreObjectIid) -> Result<Option<String>, ()> {
        let store_key = KvStoreKey::iid_to_oid(&self.bucket, iid);

        tracing::debug!("store get iid-to-oid: {store_key}");

        match self.store.database.get(&store_key) {
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
    pub fn get_iid_to_terms(&self, iid: StoreObjectIid) -> Result<Option<Vec<StoreTermHash>>, ()> {
        let store_key = KvStoreKey::iid_to_terms(&self.bucket, iid);

        tracing::debug!("store get iid-to-terms: {store_key}");

        match self.store.database.get(&store_key) {
            Ok(Some(value)) => {
                tracing::debug!("got iid-to-terms: {store_key} with encoded value: {value:?}");

                decode_terms_list(&value).map(|value_decoded| {
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

impl<'a> KvRepositoryReadWrite<'a> {
    /// This is `O(1)`, nothing meaningful happens.
    fn as_read_only<'b>(&'b self) -> KvRepositoryReadOnly<'b> {
        KvRepositoryReadOnly {
            bucket: self.bucket,
            store: self.store,
        }
    }

    /// Meta-to-Value mapper
    ///
    /// [IDX=0] ((meta)) ~> ((value))
    pub fn get_meta_to_value<T: std::str::FromStr>(
        &self,
        meta: StoreMetaKey,
    ) -> Result<Option<T>, ()> {
        self.as_read_only().get_meta_to_value(meta)
    }

    pub fn set_meta_to_value(
        &self,
        batch: &mut WriteBatch,
        meta: StoreMetaKey,
        value: impl ToString,
    ) {
        let store_key = KvStoreKey::meta_to_value(&self.bucket, &meta);

        tracing::debug!("store set meta-to-value: {store_key}");

        batch.put(store_key, value.to_string().as_bytes())
    }

    #[inline]
    pub fn get_object_count(&self) -> Result<u64, Box<dyn std::error::Error>> {
        self.as_read_only().get_object_count()
    }

    #[inline]
    fn add_object_count(&self, batch: &mut WriteBatch, diff: i64) {
        let store_key = KvStoreKey::meta_to_value(&self.bucket, &StoreMetaKey::ObjectCount);

        tracing::trace!(
            ?store_key,
            "increasing {} object count by {diff}",
            &self.bucket
        );

        batch.merge(store_key, encode_i64_counter(diff));
    }

    pub fn get_new_iid(
        &self,
        batch: &mut WriteBatch,
    ) -> Result<StoreObjectIid, Box<dyn std::error::Error>> {
        // Increment object count
        self.add_object_count(batch, 1);

        self.store.get_new_iid(self.bucket, batch)
    }

    /// Term-to-IIDs mapper
    ///
    /// [IDX=1] ((term)) ~> [((iid))]
    #[inline]
    pub fn get_term_to_iids(
        &self,
        term_hash: &StoreTermHash,
    ) -> Result<Option<Vec<StoreObjectIid>>, ()> {
        self.as_read_only().get_term_to_iids(term_hash)
    }

    // TODO(pref): Update merge operator to support deletion and get rid of this.
    pub fn set_term_to_iids(
        &self,
        batch: &mut WriteBatch,
        term_hash: &StoreTermHash,
        iids: impl ExactSizeIterator<Item = StoreObjectIid>,
    ) {
        let store_key = KvStoreKey::term_to_iids(&self.bucket, term_hash);

        tracing::debug!("store set term-to-iids: {store_key}");

        // Encode IID list into storage serialized format
        let iids_encoded = encode_iids_list(iids);

        tracing::debug!("store set term-to-iids: {store_key} with encoded value: {iids_encoded:?}");

        batch.put(store_key, &iids_encoded)
    }

    pub fn add_term_to_iid(
        &self,
        batch: &mut WriteBatch,
        term_hash: &StoreTermHash,
        iid: StoreObjectIid,
    ) {
        let store_key = KvStoreKey::term_to_iids(&self.bucket, term_hash);

        tracing::debug!("store add term-to-iids: {store_key}");

        batch.merge(&store_key, encode_iid(iid));
    }

    pub fn delete_term_to_iids(&self, batch: &mut WriteBatch, term_hash: &StoreTermHash) {
        let store_key = KvStoreKey::term_to_iids(&self.bucket, term_hash);

        tracing::debug!("store delete term-to-iids: {store_key}");

        batch.delete(store_key)
    }

    /// OID-to-IID mapper
    ///
    /// [IDX=2] ((oid)) ~> ((iid))
    pub fn get_oid_to_iid(&self, oid: StoreObjectOid) -> Result<Option<StoreObjectIid>, ()> {
        self.as_read_only().get_oid_to_iid(oid)
    }

    pub fn set_oid_to_iid(&self, batch: &mut WriteBatch, oid: StoreObjectOid, iid: StoreObjectIid) {
        let store_key = KvStoreKey::oid_to_iid(&self.bucket, oid);

        tracing::debug!("store set oid-to-iid: {store_key}");

        // Encode IID
        let iid_encoded = encode_iid(iid);

        tracing::debug!("store set oid-to-iid: {store_key} with encoded value: {iid_encoded:?}");

        batch.put(store_key, &iid_encoded)
    }

    pub fn delete_oid_to_iid(&self, batch: &mut WriteBatch, oid: StoreObjectOid) {
        let store_key = KvStoreKey::oid_to_iid(&self.bucket, oid);

        tracing::debug!("store delete oid-to-iid: {store_key}");

        batch.delete(store_key)
    }

    /// IID-to-OID mapper
    ///
    /// [IDX=3] ((iid)) ~> ((oid))
    pub fn get_iid_to_oid(&self, iid: StoreObjectIid) -> Result<Option<String>, ()> {
        self.as_read_only().get_iid_to_oid(iid)
    }

    pub fn set_iid_to_oid(&self, batch: &mut WriteBatch, iid: StoreObjectIid, oid: StoreObjectOid) {
        let store_key = KvStoreKey::iid_to_oid(&self.bucket, iid);

        tracing::debug!("store set iid-to-oid: {store_key}");

        batch.put(store_key, oid.as_bytes())
    }

    pub fn delete_iid_to_oid(&self, batch: &mut WriteBatch, iid: StoreObjectIid) {
        let store_key = KvStoreKey::iid_to_oid(&self.bucket, iid);

        tracing::debug!("store delete iid-to-oid: {store_key}");

        batch.delete(store_key)
    }

    /// IID-to-Terms mapper
    ///
    /// [IDX=4] ((iid)) ~> [((term))]
    pub fn get_iid_to_terms(&self, iid: StoreObjectIid) -> Result<Option<Vec<StoreTermHash>>, ()> {
        self.as_read_only().get_iid_to_terms(iid)
    }

    pub fn set_iid_to_terms<'t>(
        &self,
        batch: &mut WriteBatch,
        iid: StoreObjectIid,
        terms_hashes: impl ExactSizeIterator<Item = &'t StoreTermHash>,
    ) {
        let store_key = KvStoreKey::iid_to_terms(&self.bucket, iid);

        tracing::debug!("store set iid-to-terms: {store_key}");

        // Encode term list into storage serialized format
        let terms_hashes_encoded = encode_terms_list(terms_hashes);

        tracing::debug!(
            "store set iid-to-terms: {store_key} with encoded value: {terms_hashes_encoded:?}"
        );

        batch.put(store_key, &terms_hashes_encoded)
    }

    pub fn add_iid_to_terms<'t>(
        &self,
        batch: &mut WriteBatch,
        iid: StoreObjectIid,
        terms_hashes: impl Iterator<Item = &'t StoreTermHash>,
    ) {
        let store_key = KvStoreKey::iid_to_terms(&self.bucket, iid);

        tracing::debug!("store add iid-to-terms: {store_key}");

        for term_hash in terms_hashes {
            batch.merge(&store_key, encode_term_hash_value(term_hash));
        }
    }

    pub fn delete_iid_to_terms(&self, batch: &mut WriteBatch, iid: StoreObjectIid) {
        let store_key = KvStoreKey::iid_to_terms(&self.bucket, iid);

        tracing::debug!("store delete iid-to-terms: {store_key}");

        batch.delete(store_key)
    }

    pub fn batch_flush_bucket(
        &self,
        batch: &mut WriteBatch,
        iid: StoreObjectIid,
        oid: StoreObjectOid,
        iid_terms_hashes: &[StoreTermHash],
    ) -> u32 {
        let mut count = 0;

        tracing::debug!(
            "store batch flush bucket: {iid:?} with hashed terms: {iid_terms_hashes:?}"
        );

        // Delete OID <> IID association
        self.delete_oid_to_iid(batch, oid);
        self.delete_iid_to_oid(batch, iid);
        self.delete_iid_to_terms(batch, iid);

        // Delete IID from each associated term
        for term_hash in iid_terms_hashes {
            let Ok(Some(mut term_iids)) = self.get_term_to_iids(term_hash) else {
                continue;
            };

            if term_iids.contains(&iid) {
                count += 1;

                // Remove IID from list of IIDs
                term_iids.retain(|&cur_iid| cur_iid != iid);
            }

            if term_iids.is_empty() {
                self.delete_term_to_iids(batch, term_hash)
            } else {
                self.set_term_to_iids(batch, term_hash, term_iids.into_iter())
            };
        }

        // Decrement object count
        self.add_object_count(batch, -1);

        count
    }
}

// MARK: - Tests

#[cfg(test)]
mod tests {
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

    #[test]
    fn it_proceeds_actions() {
        let kv_store_config = test_kv_store_config();
        let kv_pool = KvStorePool::new(kv_store_config);

        let store = kv_pool
            .acquire(true, "c:test:3".into(), None, |_| {})
            .unwrap()
            .unwrap();
        let action = store.to_repository_read_write("b:test:3".into());

        assert!(
            action
                .get_meta_to_value::<StoreObjectIid>(StoreMetaKey::IIDIncr)
                .is_ok()
        );
        assert!({
            let mut batch = WriteBatch::default();
            action.set_meta_to_value(&mut batch, StoreMetaKey::IIDIncr, 1);
            action.write(batch).is_ok()
        });

        assert!(action.get_term_to_iids(&1.into()).is_ok());
        assert!({
            let mut batch = WriteBatch::default();
            action.set_term_to_iids(
                &mut batch,
                &1.into(),
                [0, 1, 2].into_iter().map(StoreObjectIid::from),
            );
            action.write(batch).is_ok()
        });
        assert!({
            let mut batch = WriteBatch::default();
            action.delete_term_to_iids(&mut batch, &1.into());
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
            let terms = ([45402].into_iter())
                .map(StoreTermHash::from)
                .collect::<Vec<_>>();
            action.set_iid_to_terms(&mut batch, 4.into(), terms.iter());
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

        f.debug_struct("KvStore")
            .field("database", database)
            .field("last_used", &AsPrettyRwLock(last_used))
            .field("last_flushed", &AsPrettyRwLock(last_flushed))
            .field("lock", &AsPrettyRwLock(lock))
            .field("iid_incr_per_bucket", &AsPrettyRwLock(iid_incr_per_bucket))
            .finish_non_exhaustive()
    }
}
