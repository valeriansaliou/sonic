// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::store::item::StoreItem;
use crate::store::kv::StoreKVActionBuilder;

pub struct ExecutorFlushC;

impl ExecutorFlushC {
    pub fn execute<'a>(store: StoreItem<'a>) -> Result<u64, ()> {
        // Important: do not acquire the store from there, as otherwise it will remain open \
        //   even if dropped in the inner function, as this caller would still own a reference to \
        //   it.
        if let StoreItem(collection, None, None) = store {
            StoreKVActionBuilder::erase(collection)
        } else {
            Err(())
        }
    }
}
