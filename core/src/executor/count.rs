// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::store::{StoreItemPart, StoreObjectOid};

impl super::Executor {
    /// Count terms in object (from KV store).
    pub fn counto(
        &self,
        collection: StoreItemPart,
        bucket: StoreItemPart,
        oid: StoreObjectOid,
    ) -> Result<u32, ()> {
        // Important: acquire database access read lock, and reference it in context. This \
        //   prevents the database from being erased while using it in this block.
        let _kv_read_guard = self.kv_pool.lock_read_access();

        if let Ok(kv_store) = self.kv_pool.acquire(false, collection, None, |_| {}) {
            let Some(kv_store) = kv_store else {
                tracing::debug!(
                    "collection store does not exist, consider {bucket:?} from {collection:?} empty"
                );
                return Ok(0);
            };

            // Important: acquire bucket store read lock
            executor_kv_lock_read!(kv_store);

            let kv_action = kv_store.access_read_only(bucket);

            // Try to resolve existing OID to IID
            kv_action
                .get_oid_to_iid(oid)
                .unwrap_or(None)
                .map(|iid| {
                    // List terms for IID
                    if let Some(terms) = kv_action.get_iid_to_terms(iid).unwrap_or(None) {
                        terms.len() as u32
                    } else {
                        0
                    }
                })
                .ok_or(())
                .or(Ok(0))
        } else {
            Err(())
        }
    }

    // FIXME: This is incorrect after a `FLUSHO`, see https://github.com/valeriansaliou/sonic/issues/392.
    /// Count objects in bucket (from KV store).
    pub fn countb(&self, collection: StoreItemPart, bucket: StoreItemPart) -> Result<u32, ()> {
        let kv_store = self.kv_pool.acquire(false, collection, None, |_| {})?;

        let Some(kv_store) = kv_store else {
            tracing::debug!(
                "collection store does not exist, consider {bucket:?} from {collection:?} empty"
            );
            return Ok(0);
        };

        let kv_action = kv_store.access_read_only(bucket);

        let iid_incr = kv_action
            .get_iid_incr()
            .map_err(|err| tracing::warn!("{err:?}"))?;

        let count = iid_incr.map_or(0, |last_iid| u32::from(last_iid) + 1);

        Ok(count)
    }

    /// Count buckets in collection (from FST filesystem).
    pub fn countc(&self, collection: StoreItemPart) -> Result<u32, ()> {
        self.fst_pool
            .count_collection_buckets(collection)
            .map(|count| count as u32)
    }
}

// MARK: - Deprecated

impl super::Executor {
    /// Count terms in (collection, bucket) from FST.
    pub fn legacy_countb(
        &self,
        collection: StoreItemPart,
        bucket: StoreItemPart,
    ) -> Result<u32, ()> {
        // Important: acquire graph access read lock, and reference it in context. This \
        //   prevents the graph from being erased while using it in this block.
        let _fst_read_guard = self.fst_pool.lock_read_access();

        if let Ok(fst_store) = self.fst_pool.acquire(collection, bucket) {
            Ok(fst_store.count_words() as u32)
        } else {
            Err(())
        }
    }
}
