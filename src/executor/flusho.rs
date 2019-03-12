// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::store::item::StoreItem;
use crate::store::kv::{StoreKVActionBuilder, StoreKVPool, STORE_ACCESS_LOCK};

pub struct ExecutorFlushO;

impl ExecutorFlushO {
    pub fn execute<'a>(store: StoreItem<'a>) -> Result<u32, ()> {
        if let StoreItem(collection, Some(bucket), Some(object)) = store {
            // Important: acquire database access read lock, and reference it in context. This \
            //   prevents the database from being erased while using it in this block.
            let _access = STORE_ACCESS_LOCK.read().unwrap();

            if let Ok(kv_store) = StoreKVPool::acquire(collection) {
                let action = StoreKVActionBuilder::write(bucket, kv_store);

                // Try to resolve existing OID to IID (if it does not exist, there is nothing to \
                //   be flushed)
                let oid = object.as_str().to_owned();

                if let Ok(iid_value) = action.get_oid_to_iid(&oid) {
                    let mut count_flushed = 0;

                    if let Some(iid) = iid_value {
                        // Resolve terms associated to IID
                        let iid_terms = action
                            .get_iid_to_terms(iid)
                            .ok()
                            .unwrap_or(None)
                            .unwrap_or(Vec::new());

                        // Flush bucket (batch operation, as it is shared w/ other executors)
                        if let Ok(batch_count) = action.batch_flush_bucket(iid, &oid, &iid_terms) {
                            count_flushed += batch_count;
                        }
                    }

                    return Ok(count_flushed);
                }
            }
        }

        Err(())
    }
}
