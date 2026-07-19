// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2022, Troy Kohler <troy.kohler@zalando.de>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::query::{QuerySearchID, QuerySearchLimit, QuerySearchOffset};
use crate::store::StoreItem;
use crate::store::kv::{StoreKVAcquireMode, StoreKVActionBuilder};

impl super::Executor {
    pub fn list(
        &self,
        item: StoreItem,
        _event_id: QuerySearchID,
        limit: QuerySearchLimit,
        offset: QuerySearchOffset,
    ) -> Result<Vec<String>, ()> {
        if let StoreItem(collection, Some(bucket), None) = item {
            let _kv_read_guard = self.kv_pool.lock_read_access();
            let kv_store = self
                .kv_pool
                .acquire(StoreKVAcquireMode::OpenOnly, collection)?;
            executor_kv_lock_read!(kv_store);
            let kv_action = StoreKVActionBuilder::access(bucket, kv_store);
            if kv_action.bucket_id().is_none() {
                return Ok(Vec::new());
            }

            tracing::debug!("running list");
            return Ok(kv_action.list_terms(limit as usize, offset as usize));
        }

        Err(())
    }
}
