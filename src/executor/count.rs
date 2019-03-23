// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::store::fst::StoreFSTPool;
use crate::store::fst::{StoreFSTActionBuilder, StoreFSTMisc};
use crate::store::item::StoreItem;
use crate::store::kv::StoreKVActionBuilder;
use crate::store::kv::{StoreKVAcquireMode, StoreKVPool};

pub struct ExecutorCount;

impl ExecutorCount {
    pub fn execute<'a>(store: StoreItem<'a>) -> Result<u32, ()> {
        match store {
            // Count terms in (collection, bucket, object) from KV
            StoreItem(collection, Some(bucket), Some(object)) => {
                // Important: acquire database access read lock, and reference it in context. This \
                //   prevents the database from being erased while using it in this block.
                general_kv_access_lock_read!();

                if let Ok(kv_store) = StoreKVPool::acquire(StoreKVAcquireMode::OpenOnly, collection)
                {
                    // Important: acquire bucket store read lock
                    executor_kv_lock_read!(kv_store);

                    let kv_action = StoreKVActionBuilder::access(bucket, kv_store);

                    // Try to resolve existing OID to IID
                    let oid = object.as_str();

                    kv_action
                        .get_oid_to_iid(oid)
                        .unwrap_or(None)
                        .map(|iid| {
                            // List terms for IID
                            if let Some(terms) = kv_action.get_iid_to_terms(iid).unwrap_or(None) {
                                terms.len() as u32
                            } else {
                                0
                            }
                        })
                        .ok_or(())
                        .or(Ok(0))
                } else {
                    Err(())
                }
            }
            // Count terms in (collection, bucket) from FST
            StoreItem(collection, Some(bucket), None) => {
                // Important: acquire graph access read lock, and reference it in context. This \
                //   prevents the graph from being erased while using it in this block.
                general_fst_access_lock_read!();

                if let Ok(fst_store) = StoreFSTPool::acquire(collection, bucket) {
                    let fst_action = StoreFSTActionBuilder::access(fst_store);

                    Ok(fst_action.count_words() as u32)
                } else {
                    Err(())
                }
            }
            // Count buckets in (collection) from FS
            StoreItem(collection, None, None) => {
                StoreFSTMisc::count_collection_buckets(collection).map(|count| count as u32)
            }
            _ => Err(()),
        }
    }
}
