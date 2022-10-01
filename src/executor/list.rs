use crate::query::types::ControlListAllLimit;
use crate::query::types::ControlListAllOffset;
use crate::store::fst::StoreFSTActionBuilder;
use crate::store::fst::StoreFSTPool;
use crate::store::item::StoreItem;

pub struct ExecutorList;

impl ExecutorList {
    pub fn execute(
        store: StoreItem,
        limit: ControlListAllLimit,
        offset: ControlListAllOffset,
    ) -> Result<Vec<String>, ()> {
        if let StoreItem(collection, Some(bucket), None) = store {

            general_fst_access_lock_read!();

            if let Ok(fst_store) = StoreFSTPool::acquire(collection, bucket) {
                let fst_action = StoreFSTActionBuilder::access(fst_store);

                debug!("running list, read lock is acquired");
                let (limit_usize, offset_usize) = (limit as usize, offset as usize);

                return fst_action.list_words(limit_usize, offset_usize);
            }
        }

        Err(())
    }
}
