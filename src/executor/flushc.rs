// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::store::StoreItem;
use crate::store::fst::StoreFSTActionBuilder;
use crate::store::kv::StoreKVActionBuilder;

impl super::Executor {
    pub fn flushc(&self, item: StoreItem) -> Result<u32, ()> {
        // Important: do not acquire the store from there, as otherwise it will remain open \
        //   even if dropped in the inner function, as this caller would still own a reference to \
        //   it.
        if let StoreItem(collection, None, None) = item {
            // Acquire KV + FST locks in write mode, as we will erase them, we need to prevent any \
            //   other consumer to use them.
            general_kv_access_lock_write!();
            general_fst_access_lock_write!();

            let kv_action_builder = StoreKVActionBuilder {
                kv_pool: &self.kv_pool,
            };
            let fst_action_builder = StoreFSTActionBuilder {
                fst_store_config: &self.app_conf.store.fst,
            };

            match (
                kv_action_builder.erase(collection, None),
                fst_action_builder.erase(collection, None),
            ) {
                (Ok(erase_count), Ok(_)) => Ok(erase_count),
                _ => Err(()),
            }
        } else {
            Err(())
        }
    }
}
