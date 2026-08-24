// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::store::StoreItem;
use crate::store::kv::{StoreKVAcquireMode, StoreKVActionBuilder};

impl super::Executor {
    pub fn flushb(&self, item: StoreItem) -> Result<u32, ()> {
        if let StoreItem(collection, Some(bucket), None) = item {
            // Important: acquire database access read lock, and reference it in context. This \
            //   prevents the database from being erased while using it in this block.
            // Notice: acquire FST lock in write mode, as we will erase it.
            let _kv_read_guard = self.kv_pool.lock_read_access();
            let _fst_write_guard = self.fst_pool.lock_write_access();

            if let Ok(kv_store) = self
                .kv_pool
                .acquire(StoreKVAcquireMode::OpenOnly, collection)
            {
                let Some(kv_store) = kv_store else {
                    tracing::debug!(
                        "collection store does not exist, consider {bucket:?} from {collection:?} already erased"
                    );
                    return Ok(0);
                };

                // Important: acquire bucket store write lock
                executor_kv_lock_write!(kv_store);

                // Store exists, proceed erasure.
                tracing::debug!(
                    "collection store exists, erasing: {} from {}",
                    bucket.as_str(),
                    collection.as_str()
                );

                let kv_action = StoreKVActionBuilder::access_read_write(bucket, kv_store);

                // Notice: we cannot use the provided KV bucket erasure helper there, as \
                //   erasing a bucket requires a database lock, which would incur a dead-lock, \
                //   thus we need to perform the erasure from there.
                if let Ok(erase_count) = kv_action.batch_erase_bucket() {
                    if self.fst_pool.erase(collection, Some(bucket)).is_ok() {
                        tracing::debug!("done with bucket erasure");

                        return Ok(erase_count);
                    }
                }
            }
        }

        Err(())
    }
}
