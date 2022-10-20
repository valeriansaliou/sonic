// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2022, Troy Kohler <troy.kohler@zalando.de>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::query::types::{QuerySearchID, QuerySearchLimit, QuerySearchOffset};
use crate::store::fst::StoreFSTActionBuilder;
use crate::store::fst::StoreFSTPool;
use crate::store::item::StoreItem;

pub struct ExecutorList;

impl ExecutorList {
    pub fn execute(
        store: StoreItem,
        _event_id: QuerySearchID,
        limit: QuerySearchLimit,
        offset: QuerySearchOffset,
    ) -> Result<Vec<String>, ()> {
        if let StoreItem(collection, Some(bucket), None) = store {
            // Important: acquire graph access read lock, and reference it in context. This \
            //   prevents the graph from being erased while using it in this block.
            general_fst_access_lock_read!();

            if let Ok(fst_store) = StoreFSTPool::acquire(collection, bucket) {
                let fst_action = StoreFSTActionBuilder::access(fst_store);

                debug!("running list");

                return fst_action.list_words(limit as usize, offset as usize);
            }
        }

        Err(())
    }
}
