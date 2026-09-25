// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod backup;
mod keys;
mod pool;
mod util;

use std::sync::{Arc, RwLock};
use std::time::SystemTime;
use std::{fmt, io};

use hashbrown::HashMap;
use rocksdb::{DB, WriteBatch};

use crate::store::generic::*;
use crate::store::*;
use crate::util::hash::NoopU32HasherBuilder;

use self::keys::KvStoreKey;
pub use self::keys::StoreMetaKey;
pub use self::pool::{KvStoreId, KvStorePool};
use self::util::*;

pub struct KvStore {
    database: DB,
    last_used: RwLock<SystemTime>,
    last_flushed: RwLock<SystemTime>,
    pub lock: RwLock<()>,
    kv_store_config: Arc<crate::config::KvStoreConfig>,

    /// Cache of `IIDIncr` per bucket, removing the need for countless reads
    /// while ingesting new data.
    ///
    /// This cache is particularly effective with large memtables, which often
    /// have bad read performance.
    ///
    /// In benchmarks, we saw a `~23%` throughput increase after this change.
    // PERF: We use a no-op hasher since u32 keys come from xxhash and are
    //   already well distributed. No need to perform another hash computation.
    iid_incr_per_bucket: RwLock<HashMap<u32, StoreObjectIid, NoopU32HasherBuilder>>,
}

pub struct KvStoreActionReadOnly<'a> {
    bucket: StoreItemPart<'a>,
    store: &'a KvStore,
}

pub struct KvStoreActionReadWrite<'a> {
    bucket: StoreItemPart<'a>,
    store: &'a KvStore,
}

type KvStoreAtom = u32;

impl KvStore {
    fn flush(&self) -> Result<(), rocksdb::Error> {
        // Generate flush options
        let mut flush_options = rocksdb::FlushOptions::default();

        flush_options.set_wait(true);

        // Perform flush (in blocking mode)
        self.database.flush_opt(&flush_options)
    }

    fn do_write(&self, batch: WriteBatch) -> Result<(), rocksdb::Error> {
        // Configure this write
        let mut write_options = rocksdb::WriteOptions::default();

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

    /// Reads `IIDIncr` from the cache, fetching from the database if necessary
    /// (beware of slow reads).
    fn get_iid_incr(
        &self,
        bucket: &StoreItemPart,
        iid_incr_per_bucket: &HashMap<u32, StoreObjectIid, NoopU32HasherBuilder>,
    ) -> Result<Option<StoreObjectIid>, Box<dyn std::error::Error>> {
        iid_incr_per_bucket.get(&bucket.into_compact()).map_or_else(
            || {
                tracing::debug!(?bucket, "IIDIncr not found in cache, reading database…");
                self.fetch_iid_incr(bucket)
            },
            |&iid_incr| {
                tracing::debug!(?bucket, ?iid_incr, "Read IIDIncr from cache");
                Ok(Some(iid_incr))
            },
        )
    }

    /// Reads `IIDIncr` directly from the database.
    fn fetch_iid_incr<'a>(
        &self,
        bucket: &StoreItemPart<'a>,
    ) -> Result<Option<StoreObjectIid>, Box<dyn std::error::Error>> {
        let store_key = KvStoreKey::meta_to_value(&bucket, &StoreMetaKey::IIDIncr);
        let value = self.database.get(store_key)?;

        match value {
            Some(bytes) => match decode_u32_mapped(&bytes) {
                Ok(iid_incr) => {
                    tracing::debug!(?bucket, ?iid_incr, "Read IIDIncr from database");
                    Ok(Some(iid_incr))
                }
                Err(()) => {
                    tracing::error!(?bucket, "Invalid IIDIncr in database");
                    Err(Box::new(io::Error::other(
                        "Invalid IIDIncr value in bucket {bucket:?}",
                    )))
                }
            },
            None => {
                tracing::debug!(?bucket, "IIDIncr not found in database");
                Ok(None)
            }
        }
    }

    fn get_new_iid(
        &self,
        bucket: StoreItemPart,
        batch: &mut WriteBatch,
    ) -> Result<StoreObjectIid, Box<dyn std::error::Error>> {
        let mut write_guard = self.iid_incr_per_bucket.write().unwrap();

        let cache_key = bucket.into_compact();
        let iid = match write_guard.get_mut(&cache_key) {
            Some(iid) => {
                let new_iid = iid.saturating_add(1);
                *iid = new_iid;
                new_iid
            }
            None => {
                let new_iid = match self.get_iid_incr(&bucket, &write_guard)? {
                    Some(iid_incr) => {
                        // COMPAT: Backfill `ObjectCount` from `IIDIncr` for
                        //   users migrating from an older version.
                        // TODO(major): Remove compat backfill.
                        {
                            let object_count_key =
                                KvStoreKey::meta_to_value(&bucket, &StoreMetaKey::ObjectCount);

                            if (self.database)
                                .get_pinned(object_count_key)
                                .is_ok_and(|opt| opt.is_none())
                            {
                                self.database
                                    .put(
                                        object_count_key,
                                        i32::try_from(u32::from(iid_incr))
                                            .unwrap_or(i32::MAX)
                                            .to_le_bytes(),
                                    )
                                    .unwrap_or_else(|error| {
                                        tracing::error!(
                                            "Could not backfill ObjectCount from IIDIncr: {error:?}"
                                        )
                                    });
                            }
                        }

                        iid_incr.saturating_add(1)
                    }
                    None => StoreObjectIid::from(0),
                };

                write_guard.insert(cache_key, new_iid);

                new_iid
            }
        };

        // Early release lock.
        drop(write_guard);

        let store_key = KvStoreKey::meta_to_value(&bucket, &StoreMetaKey::IIDIncr);
        batch.merge(store_key, iid.into_bytes());

        Ok(iid)
    }
}

impl<'a> KvStoreActionReadWrite<'a> {
    pub fn write(&self, batch: WriteBatch) -> Result<(), rocksdb::Error> {
        self.store.do_write(batch)
    }
}

impl StoreGeneric for KvStore {
    fn ref_last_used(&self) -> &RwLock<SystemTime> {
        &self.last_used
    }
}

impl KvStore {
    pub fn access_read_only<'a>(&'a self, bucket: StoreItemPart<'a>) -> KvStoreActionReadOnly<'a> {
        KvStoreActionReadOnly {
            bucket,
            store: self,
        }
    }

    pub fn access_read_write<'a>(
        &'a self,
        bucket: StoreItemPart<'a>,
    ) -> KvStoreActionReadWrite<'a> {
        KvStoreActionReadWrite {
            bucket,
            store: self,
        }
    }
}

impl<'a> KvStoreActionReadOnly<'a> {
    /// Meta-to-Value mapper
    ///
    /// [IDX=0] ((meta)) ~> ((value))
    pub fn get_meta_to_value<T: std::str::FromStr>(
        &self,
        meta: StoreMetaKey,
    ) -> Result<Option<T>, ()> {
        let store_key = KvStoreKey::meta_to_value(&self.bucket, &meta);

        tracing::debug!("store get meta-to-value: {store_key}");

        match self.store.database.get(store_key) {
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

    /// Note that because of the underlying use of `i32`, the max value is
    /// `i32::MAX` (hence `u32::MAX / 2`).
    pub fn get_object_count(&self) -> Result<u32, Box<dyn std::error::Error>> {
        let bucket = self.bucket;

        let store_key = KvStoreKey::meta_to_value(&bucket, &StoreMetaKey::ObjectCount);
        let value = self.store.database.get(&store_key)?;

        let get_iid_incr_fallback = || {
            self.store
                .get_iid_incr(&bucket, &self.store.iid_incr_per_bucket.read().unwrap())
                .map(|opt| opt.map_or(0, |n| u32::from(n).saturating_add(1)))
        };

        match value {
            Some(bytes) if bytes.len() == 4 => {
                // SAFETY: `bytes` is guaranteed to be 4 bytes long.
                let count = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);

                tracing::debug!(?bucket, ?count, "Read ObjectCount from database");

                // FIX: In Sonic v1.10.0, singed counters used to be stored
                //   unsigned. In itself it wasn’t a problem, but because of
                //   how RocksDB merges operations, huge batches would cause
                //   partial counters to be mis-interpreted, yielding a final
                //   sum that’s far off. We can fix it by checking whether
                //   `ObjectCount` is negative or bigger than `IIDIncr + 1`.
                let iid_incr_fallback = get_iid_incr_fallback()?;
                if (count < 0) || (count as u32 > iid_incr_fallback) {
                    // Fix stored `ObjectCount` once (issue won’t reappear).
                    self.store.database.put(
                        &store_key,
                        i32::try_from(iid_incr_fallback)
                            .unwrap_or(i32::MAX)
                            .to_le_bytes(),
                    )?;
                    tracing::debug!(
                        ?bucket,
                        "Fixed ObjectCount in database, falling back to IIDIncr (only this time)"
                    );
                    return Ok(iid_incr_fallback);
                }

                match u32::try_from(count) {
                    Ok(count) => Ok(count),
                    Err(error) => Err(Box::new(io::Error::other(format!(
                        "Invalid ObjectCount value in bucket {bucket:?}: {error:?}",
                    )))),
                }
            }
            Some(_bytes) => {
                tracing::error!(?bucket, "Invalid ObjectCount in database");
                Err(Box::new(io::Error::other(
                    "Invalid ObjectCount value in bucket {bucket:?}",
                )))
            }
            None => {
                tracing::debug!(
                    ?bucket,
                    "ObjectCount not found in database, falling back to IIDIncr"
                );

                // COMPAT: Fallback to `IIDIncr` for users migrating from an older version.
                // TODO(major): Remove compat fallback.
                get_iid_incr_fallback()
            }
        }
    }

    /// Term-to-IIDs mapper
    ///
    /// [IDX=1] ((term)) ~> [((iid))]
    pub fn get_term_to_iids(
        &self,
        term_hash: StoreTermHash,
    ) -> Result<Option<Vec<StoreObjectIid>>, ()> {
        let store_key = KvStoreKey::term_to_iids(&self.bucket, term_hash);

        tracing::debug!("store get term-to-iids: {store_key}");

        match self.store.database.get(store_key) {
            Ok(Some(value)) => {
                tracing::debug!("got term-to-iids: {store_key} with encoded value: {value:?}");

                decode_u32_list_mapped(&value).map(|value_decoded| {
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

        match self.store.database.get(store_key) {
            Ok(Some(value)) => {
                tracing::debug!("got oid-to-iid: {store_key} with encoded value: {value:?}");

                decode_u32_mapped(&value).map(|value_decoded| {
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

        match self.store.database.get(store_key) {
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

        match self.store.database.get(store_key) {
            Ok(Some(value)) => {
                tracing::debug!("got iid-to-terms: {store_key} with encoded value: {value:?}");

                decode_u32_list_mapped(&value).map(|value_decoded| {
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

impl<'a> KvStoreActionReadWrite<'a> {
    /// This is `O(1)`, nothing meaningful happens.
    fn as_read_only<'b>(&'b self) -> KvStoreActionReadOnly<'b> {
        KvStoreActionReadOnly {
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
    pub fn get_object_count(&self) -> Result<u32, Box<dyn std::error::Error>> {
        self.as_read_only().get_object_count()
    }

    #[inline]
    fn add_object_count(&self, batch: &mut WriteBatch, diff: i32) {
        let store_key = KvStoreKey::meta_to_value(&self.bucket, &StoreMetaKey::ObjectCount);

        tracing::trace!(
            ?store_key,
            "increasing {} object count by {diff}",
            &self.bucket
        );

        batch.merge(store_key, diff.to_le_bytes());
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
        term_hash: StoreTermHash,
    ) -> Result<Option<Vec<StoreObjectIid>>, ()> {
        self.as_read_only().get_term_to_iids(term_hash)
    }

    // TODO(pref): Update merge operator to support deletion and get rid of this.
    pub fn set_term_to_iids(
        &self,
        batch: &mut WriteBatch,
        term_hash: StoreTermHash,
        iids: impl ExactSizeIterator<Item = StoreObjectIid>,
    ) {
        let store_key = KvStoreKey::term_to_iids(&self.bucket, term_hash);

        tracing::debug!("store set term-to-iids: {store_key}");

        // Encode IID list into storage serialized format
        let iids_encoded = encode_u32_list_mapped(iids);

        tracing::debug!("store set term-to-iids: {store_key} with encoded value: {iids_encoded:?}");

        batch.put(store_key, &iids_encoded)
    }

    pub fn add_term_to_iids(
        &self,
        batch: &mut WriteBatch,
        term_hash: StoreTermHash,
        iids: impl Iterator<Item = StoreObjectIid>,
    ) {
        let store_key = KvStoreKey::term_to_iids(&self.bucket, term_hash);

        tracing::debug!("store add term-to-iids: {store_key}");

        for iid in iids {
            batch.merge(store_key, iid.into_bytes());
        }
    }

    pub fn delete_term_to_iids(&self, batch: &mut WriteBatch, term_hash: StoreTermHash) {
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
        let iid_encoded = iid.into_bytes();

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

    pub fn set_iid_to_terms(
        &self,
        batch: &mut WriteBatch,
        iid: StoreObjectIid,
        terms_hashes: impl ExactSizeIterator<Item = StoreTermHash>,
    ) {
        let store_key = KvStoreKey::iid_to_terms(&self.bucket, iid);

        tracing::debug!("store set iid-to-terms: {store_key}");

        // Encode term list into storage serialized format
        let terms_hashes_encoded = encode_u32_list_mapped(terms_hashes);

        tracing::debug!(
            "store set iid-to-terms: {store_key} with encoded value: {terms_hashes_encoded:?}"
        );

        batch.put(store_key, &terms_hashes_encoded)
    }

    pub fn add_iid_to_terms(
        &self,
        batch: &mut WriteBatch,
        iid: StoreObjectIid,
        terms_hashes: impl Iterator<Item = StoreTermHash>,
    ) {
        let store_key = KvStoreKey::iid_to_terms(&self.bucket, iid);

        tracing::debug!("store add iid-to-terms: {store_key}");

        for term_hash in terms_hashes {
            batch.merge(store_key, term_hash.into_bytes());
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
            let Ok(Some(mut term_iids)) = self.get_term_to_iids(*term_hash) else {
                continue;
            };

            if term_iids.contains(&iid) {
                count += 1;

                // Remove IID from list of IIDs
                term_iids.retain(|&cur_iid| cur_iid != iid);
            }

            if term_iids.is_empty() {
                self.delete_term_to_iids(batch, *term_hash)
            } else {
                self.set_term_to_iids(batch, *term_hash, term_iids.into_iter())
            };
        }

        // Decrement object count
        self.add_object_count(batch, -1);

        count
    }

    pub fn batch_erase_bucket(&self) -> Result<u32, ()> {
        let bucket = self.bucket;

        // Generate all key prefix values (with dummy post-prefix values; we dont care)
        let (k_meta_to_value, k_term_to_iids, k_oid_to_iid, k_iid_to_oid, k_iid_to_terms) = (
            KvStoreKey::meta_to_value(&bucket, &StoreMetaKey::IIDIncr),
            KvStoreKey::term_to_iids(&bucket, 0.into()),
            KvStoreKey::oid_to_iid(&bucket, StoreObjectOid(StoreItemPart(""))),
            KvStoreKey::iid_to_oid(&bucket, 0.into()),
            KvStoreKey::iid_to_terms(&bucket, 0.into()),
        );

        let key_prefixes = [
            k_meta_to_value.into_prefix(),
            k_term_to_iids.into_prefix(),
            k_oid_to_iid.into_prefix(),
            k_iid_to_oid.into_prefix(),
            k_iid_to_terms.into_prefix(),
        ];

        // Scan all keys per-prefix and nuke them right away
        for key_prefix in &key_prefixes {
            tracing::debug!("store batch erase bucket: {bucket} for prefix: {key_prefix:?}");

            // Generate start and end prefix for batch delete (in other words,
            // the minimum key value possible, and the highest key value possible)
            let key_prefix_start = KvStoreKey::from([
                key_prefix[0],
                key_prefix[1],
                key_prefix[2],
                key_prefix[3],
                key_prefix[4],
                0,
                0,
                0,
                0,
            ]);
            let key_prefix_end = KvStoreKey::from([
                key_prefix[0],
                key_prefix[1],
                key_prefix[2],
                key_prefix[3],
                key_prefix[4],
                255,
                255,
                255,
                255,
            ]);

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

        (self.store.iid_incr_per_bucket.write().unwrap()).remove(&bucket.into_compact());

        tracing::info!("done processing store batch erase bucket: {bucket}");

        Ok(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_proceeds_actions() {
        let kv_store_config = test_kv_store_config();
        let kv_pool = KvStorePool::new(kv_store_config);

        let store = kv_pool
            .acquire(true, "c:test:3".into(), None, |_| {})
            .unwrap()
            .unwrap();
        let action = store.access_read_write("b:test:3".into());

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

        assert!(action.get_term_to_iids(1.into()).is_ok());
        assert!({
            let mut batch = WriteBatch::default();
            action.set_term_to_iids(
                &mut batch,
                1.into(),
                [0, 1, 2].into_iter().map(StoreObjectIid::from),
            );
            action.write(batch).is_ok()
        });
        assert!({
            let mut batch = WriteBatch::default();
            action.delete_term_to_iids(&mut batch, 1.into());
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
            action.set_iid_to_terms(
                &mut batch,
                4.into(),
                [45402].into_iter().map(StoreTermHash::from),
            );
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
