// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::store::fst::StoreFSTActionBuilder;
use crate::store::item::StoreItem;
use crate::store::kv::{StoreKVActionBuilder, StoreKVPool, STORE_ACCESS_LOCK};

pub struct ExecutorFlushB;

impl ExecutorFlushB {
    pub fn execute<'a>(store: StoreItem<'a>) -> Result<u32, ()> {
        if let StoreItem(collection, Some(bucket), None) = store {
            // Important: acquire database access read lock, and reference it in context. This \
            //   prevents the database from being erased while using it in this block.
            let _kv_access = STORE_ACCESS_LOCK.read().unwrap();

            if let Ok(kv_store) = StoreKVPool::acquire(collection) {
                let kv_action = StoreKVActionBuilder::write(bucket, kv_store);

                if let Ok(erase_count) = kv_action.batch_erase_bucket() {
                    if StoreFSTActionBuilder::erase(collection, Some(bucket)).is_ok() == true {
                        return Ok(erase_count);
                    }
                }
            }
        }

        Err(())
    }
}
