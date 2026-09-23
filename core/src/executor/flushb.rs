// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::store::{Bucket, StoreItemPart};

impl super::Executor {
    pub fn flushb(&self, collection: StoreItemPart, bucket: Bucket) -> Result<u32, ()> {
        // Important: acquire database access read lock, and reference it in context. This \
        //   prevents the database from being erased while using it in this block.
        // Notice: acquire FST lock in write mode, as we will erase it.
        let _kv_read_guard = self.kv_pool.lock_read_access();
        let _fst_write_guard = self.fst_pool.lock_write_access();

        match (
            self.kv_pool.erase(collection, Some(bucket)),
            self.fst_pool.erase(collection, Some(bucket)),
        ) {
            (Ok(_), Ok(erase_count)) => Ok(erase_count),
            _ => Err(()),
        }
    }
}
