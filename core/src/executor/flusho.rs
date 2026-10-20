// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use rocksdb::WriteBatch;

use crate::store::{Bucket, StoreItemPart, StoreObjectOid};

impl super::Executor {
    pub fn flusho(
        &self,
        collection: StoreItemPart,
        bucket: Bucket,
        oid: StoreObjectOid,
    ) -> Result<u32, ()> {
        // Important: acquire database access read lock, and reference it in context. This \
        //   prevents the database from being erased while using it in this block.
        let _kv_read_guard = self.kv_pool.lock_read_access();

        let Ok(kv_store) = self.kv_pool.acquire(false, collection, None) else {
            return Err(());
        };

        let Some(kv_store) = kv_store else {
            tracing::debug!(
                "collection store does not exist, consider {bucket:?} from {collection:?} empty"
            );
            return Ok(0);
        };

        // Important: acquire bucket store write lock
        executor_kv_lock_write!(kv_store);

        let kv_repo = kv_store.to_repository_read_write(bucket);

        // Try to resolve existing OID to IID (if it does not exist, there is nothing to \
        //   be flushed)
        let Ok(iid_value) = kv_repo.get_oid_to_iid(oid) else {
            tracing::error!("failed getting flusho executor oid-to-iid");
            return Err(());
        };

        let Some(iid) = iid_value else {
            return Ok(0);
        };

        // Resolve terms associated to IID
        let object_terms = (kv_repo.get_iid_to_terms(iid))
            .map_err(|()| tracing::error!("failed getting flusho executor iid-to-terms"))?
            .into_iter()
            .flatten()
            .map(|entry| entry.term_hash());

        let mut batch = WriteBatch::default();

        // Flush bucket (batch operation, as it is shared w/ other executors)
        let batch_count = kv_repo.delete_object(&mut batch, iid, oid, object_terms);

        kv_repo.write(batch).map_err(|error| {
            tracing::error!("failed executing batch-flush-bucket in flusho executor: {error}")
        })?;

        Ok(batch_count)
    }
}
