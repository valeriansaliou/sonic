// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::store::item::StoreItem;
use crate::store::kv::{StoreKVActionBuilder, StoreKVPool};

pub struct ExecutorFlushC;

impl ExecutorFlushC {
    pub fn execute<'a>(store: StoreItem<'a>) -> Result<u64, ()> {
        if let StoreItem(collection, None, None) = store {
            if let Ok(kv_store) = StoreKVPool::acquire(collection) {
                // let action = StoreKVActionBuilder::write(bucket, kv_store);

                // TODO: begin database lock (mutex on collection database acquire fn)
                // TODO: force a rocksdb database fd close
                // TODO: remove whole database from file system
                // TODO: end database lock (mutex on collection database acquire fn)

                // TODO
                return Ok(0);
            }
        }

        Err(())
    }
}
