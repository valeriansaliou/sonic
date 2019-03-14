// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::store::item::StoreItem;
use crate::store::kv::{StoreKVPool, STORE_ACCESS_LOCK};

pub struct ExecutorCount;

impl ExecutorCount {
    pub fn execute<'a>(store: StoreItem<'a>) -> Result<u32, ()> {
        match store {
            StoreItem(collection, _bucket_value, _object_value) => {
                // Important: acquire database access read lock, and reference it in context. This \
                //   prevents the database from being erased while using it in this block.
                let _kv_access = STORE_ACCESS_LOCK.read().unwrap();

                if let Ok(_kv_store) = StoreKVPool::acquire(collection) {
                    // let kv_action = StoreKVActionBuilder::read(bucket, kv_store);

                    // TODO: if object, count terms in object (from kv directly)
                    // TODO: if bucket, count terms (from fst directly)
                    // TODO: if collection, count buckets (from fs directly)

                    // TODO
                    return Ok(0);
                }
            }
        }

        Err(())
    }
}
