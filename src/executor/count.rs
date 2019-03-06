// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::store::item::StoreItem;
use crate::store::kv::{StoreKVActionBuilder, StoreKVPool};

pub struct ExecutorCount;

impl ExecutorCount {
    pub fn execute<'a>(store: StoreItem<'a>) -> Result<u64, ()> {
        if let StoreItem(collection, bucket_value, object_value) = store {
            if let Ok(kv_store) = StoreKVPool::acquire(collection) {
                // let action = StoreKVActionBuilder::new(bucket, kv_store);

                // TODO
                return Ok(0);
            }
        }

        Err(())
    }
}
