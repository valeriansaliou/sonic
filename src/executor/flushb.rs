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
            general_kv_access_lock_read!();
            general_fst_access_lock_write!();

            if let Ok(kv_store) = self
                .kv_pool
                .acquire(StoreKVAcquireMode::OpenOnly, collection)
            {
                // Important: acquire bucket store write lock
                executor_kv_lock_write!(kv_store);

                if kv_store.is_some() {
                    // Store exists, proceed erasure.
                    tracing::debug!(
                        "collection store exists, erasing: {} from {}",
                        bucket.as_str(),
                        collection.as_str()
                    );

                    let kv_action = StoreKVActionBuilder::access(bucket, kv_store);

                    // Notice: we cannot use the provided KV bucket erasure helper there, as \
                    //   erasing a bucket requires a database lock, which would incur a dead-lock, \
                    //   thus we need to perform the erasure from there.
                    if let Ok(erase_count) = kv_action.batch_erase_bucket() {
                        if self.fst_pool.erase(collection, Some(bucket)).is_ok() {
                            tracing::debug!("done with bucket erasure");

                            return Ok(erase_count);
                        }
                    }
                } else {
                    // Store does not exist, consider as already erased.
                    tracing::debug!(
                        "collection store does not exist, consider {} from {} already erased",
                        bucket.as_str(),
                        collection.as_str()
                    );

                    return Ok(0);
                }
            }
        }

        Err(())
    }
}
