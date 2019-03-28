// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::store::fst::StoreFSTActionBuilder;
use crate::store::item::StoreItem;
use crate::store::kv::{StoreKVAcquireMode, StoreKVActionBuilder, StoreKVPool};

pub struct ExecutorFlushB;

impl ExecutorFlushB {
    pub fn execute<'a>(store: StoreItem<'a>) -> Result<u32, ()> {
        if let StoreItem(collection, Some(bucket), None) = store {
            // Important: acquire database access read lock, and reference it in context. This \
            //   prevents the database from being erased while using it in this block.
            general_kv_access_lock_read!();

            if let Ok(kv_store) = StoreKVPool::acquire(StoreKVAcquireMode::OpenOnly, collection) {
                // Important: acquire bucket store write lock
                executor_kv_lock_write!(kv_store);

                if kv_store.is_some() == true {
                    // Store exists, proceed erasure.
                    debug!(
                        "collection store exists, erasing: {} from {}",
                        bucket.as_str(),
                        collection.as_str()
                    );

                    let kv_action = StoreKVActionBuilder::access(bucket, kv_store);

                    // Notice: we cannot use the provided KV bucket erasure helper there, as \
                    //   erasing a bucket requires a database lock, which would incur a dead-lock, \
                    //   thus we need to perform the erasure from there.
                    if let Ok(erase_count) = kv_action.batch_erase_bucket() {
                        if StoreFSTActionBuilder::erase(collection, Some(bucket)).is_ok() == true {
                            debug!("done with bucket erasure");

                            return Ok(erase_count);
                        }
                    }
                } else {
                    // Store does not exist, consider as already erased.
                    debug!(
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
