// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Sonic OSS License v1.0 (SOSSL v1.0)

use crate::store::fst::StoreFSTActionBuilder;
use crate::store::item::StoreItem;
use crate::store::kv::StoreKVActionBuilder;

pub struct ExecutorFlushC;

impl ExecutorFlushC {
    pub fn execute<'a>(store: StoreItem<'a>) -> Result<u32, ()> {
        // Important: do not acquire the store from there, as otherwise it will remain open \
        //   even if dropped in the inner function, as this caller would still own a reference to \
        //   it.
        if let StoreItem(collection, None, None) = store {
            match (
                StoreKVActionBuilder::erase(collection, None),
                StoreFSTActionBuilder::erase(collection, None),
            ) {
                (Ok(erase_count), Ok(_)) => Ok(erase_count),
                _ => Err(()),
            }
        } else {
            Err(())
        }
    }
}
