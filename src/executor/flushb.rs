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
                let _kv_action = StoreKVActionBuilder::write(bucket, kv_store);

                // TODO: begin database lock (mutex on collection database acquire fn)
                // TODO: force a rocksdb database fd close
                // TODO: remove whole database from file system
                // TODO: end database lock (mutex on collection database acquire fn)

                if StoreFSTActionBuilder::erase(collection, Some(bucket)).is_ok() == true {
                    // TODO: erase on key-value store also
                    return Ok(0);
                }
            }
        }

        Err(())
    }
}
