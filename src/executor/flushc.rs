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
                return StoreKVActionBuilder::erase(kv_store);
            }
        }

        Err(())
    }
}
