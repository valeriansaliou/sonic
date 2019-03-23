// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::store::item::StoreItem;
use crate::store::kv::{StoreKVAcquireMode, StoreKVActionBuilder, StoreKVPool};

pub struct ExecutorFlushO;

impl ExecutorFlushO {
    pub fn execute<'a>(store: StoreItem<'a>) -> Result<u32, ()> {
        if let StoreItem(collection, Some(bucket), Some(object)) = store {
            // Important: acquire database access read lock, and reference it in context. This \
            //   prevents the database from being erased while using it in this block.
            general_kv_access_lock_read!();

            if let Ok(kv_store) = StoreKVPool::acquire(StoreKVAcquireMode::OpenOnly, collection) {
                // Important: acquire bucket store write lock
                executor_kv_lock_write!(kv_store);

                let kv_action = StoreKVActionBuilder::access(bucket, kv_store);

                // Try to resolve existing OID to IID (if it does not exist, there is nothing to \
                //   be flushed)
                let oid = object.as_str();

                if let Ok(iid_value) = kv_action.get_oid_to_iid(oid) {
                    let mut count_flushed = 0;

                    if let Some(iid) = iid_value {
                        // Resolve terms associated to IID
                        let iid_terms = {
                            if let Ok(iid_terms_value) = kv_action.get_iid_to_terms(iid) {
                                iid_terms_value.unwrap_or(Vec::new())
                            } else {
                                error!("failed getting flusho executor iid-to-terms");

                                Vec::new()
                            }
                        };

                        // Flush bucket (batch operation, as it is shared w/ other executors)
                        if let Ok(batch_count) = kv_action.batch_flush_bucket(iid, oid, &iid_terms)
                        {
                            count_flushed += batch_count;
                        } else {
                            error!("failed executing batch-flush-bucket in flusho executor");
                        }
                    }

                    return Ok(count_flushed);
                } else {
                    error!("failed getting flusho executor oid-to-iid");
                }
            }
        }

        Err(())
    }
}
