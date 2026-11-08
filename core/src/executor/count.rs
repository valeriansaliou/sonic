// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::store::{Bucket, GenericOid, StoreItemPart};

impl super::Executor {
    /// Count terms in object (from KV store).
    #[tracing::instrument(
        level = "info",
        skip_all, fields(?collection, ?bucket, object = ?oid),
    )]
    pub fn counto(
        &self,
        collection: StoreItemPart,
        bucket: Bucket,
        oid: GenericOid,
    ) -> Result<u32, crate::Error> {
        // Important: acquire database access read lock, and reference it in context. This \
        //   prevents the database from being erased while using it in this block.
        let _kv_read_guard = self.kv_pool.lock_read_access();

        let Some(kv_store) = self.kv_pool.acquire(false, collection, None)? else {
            tracing::debug!("collection store does not exist, consider bucket empty.");
            return Ok(0);
        };

        // Important: acquire bucket store read lock
        executor_kv_lock_read!(kv_store);

        let kv_repo = kv_store.to_repository_read_only(bucket);

        // Try to resolve existing OID to IID
        let Some(iid) = kv_repo.get_oid_to_iid(oid)? else {
            return Ok(0);
        };

        // List terms for IID
        let Some(terms) = kv_repo.get_iid_to_terms(iid)? else {
            return Ok(0);
        };

        Ok(terms.len() as u32)
    }

    /// Count objects in bucket (from KV store).
    #[tracing::instrument(
        level = "info",
        skip_all, fields(?collection, ?bucket),
    )]
    pub fn countb(&self, collection: StoreItemPart, bucket: Bucket) -> Result<u64, crate::Error> {
        let Some(kv_store) = self.kv_pool.acquire(false, collection, None)? else {
            tracing::debug!("Collection store does not exist, consider bucket empty.");
            return Ok(0);
        };

        let kv_repo = kv_store.to_repository_read_only(bucket);

        let count = kv_repo.get_object_count()?;

        Ok(count)
    }

    /// Count buckets in collection (from FST filesystem).
    #[tracing::instrument(
        level = "info",
        skip_all, fields(?collection),
    )]
    pub fn countc(&self, collection: StoreItemPart) -> Result<u32, crate::Error> {
        self.fst_pool
            .count_collection_buckets(collection)
            .map(|count| count as u32)
    }
}

// MARK: - Deprecated

impl super::Executor {
    /// Count terms in (collection, bucket) from FST.
    #[tracing::instrument(
        level = "info",
        skip_all, fields(?collection, ?bucket),
    )]
    pub fn legacy_countb(
        &self,
        collection: StoreItemPart,
        bucket: Bucket,
    ) -> Result<u32, crate::Error> {
        // Important: acquire graph access read lock, and reference it in context. This \
        //   prevents the graph from being erased while using it in this block.
        let _fst_read_guard = self.fst_pool.lock_read_access();

        let fst_store = self.fst_pool.acquire(collection, bucket)?;

        let fst_repo = fst_store.to_repository();

        Ok(fst_repo.count_words() as u32)
    }
}
