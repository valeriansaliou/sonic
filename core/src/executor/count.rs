// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::store::StoreItem;
use crate::store::kv::StoreKVAcquireMode;
use crate::store::kv::StoreKVActionBuilder;

impl super::Executor {
    pub fn count(&self, item: StoreItem) -> Result<u32, ()> {
        match item {
            // Count terms in (collection, bucket, object) from KV
            StoreItem(collection, Some(bucket), Some(object)) => {
                // Important: acquire database access read lock, and reference it in context. This \
                //   prevents the database from being erased while using it in this block.
                let _kv_read_guard = self.kv_pool.lock_read_access();

                if let Ok(kv_store) = self
                    .kv_pool
                    .acquire(StoreKVAcquireMode::OpenOnly, collection)
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
            // Count terms in (collection, bucket) from KV
            StoreItem(collection, Some(bucket), None) => {
                let _kv_read_guard = self.kv_pool.lock_read_access();
                let kv_store = self
                    .kv_pool
                    .acquire(StoreKVAcquireMode::OpenOnly, collection)?;
                executor_kv_lock_read!(kv_store);
                let kv_action = StoreKVActionBuilder::access(bucket, kv_store);
                if kv_action.bucket_id().is_none() {
                    return Ok(0);
                }
                Ok(kv_action.count_terms() as u32)
            }
            // Count buckets in (collection) from KV
            StoreItem(collection, None, None) => {
                let _kv_read_guard = self.kv_pool.lock_read_access();
                let kv_store = self
                    .kv_pool
                    .acquire(StoreKVAcquireMode::OpenOnly, collection)?;

                Ok(kv_store
                    .as_ref()
                    .map(|store| store.count_buckets() as u32)
                    .unwrap_or(0))
            }
            _ => Err(()),
        }
    }
}
