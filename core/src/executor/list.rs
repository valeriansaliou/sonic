// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2022, Troy Kohler <troy.kohler@zalando.de>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use super::types::{QuerySearchLimit, QuerySearchOffset};
use crate::store::{Bucket, StoreItemPart};

impl super::Executor {
    pub fn list(
        &self,
        collection: StoreItemPart,
        bucket: Bucket,
        limit: QuerySearchLimit,
        offset: QuerySearchOffset,
    ) -> Result<Vec<String>, ()> {
        // Important: acquire graph access read lock, and reference it in context. This \
        //   prevents the graph from being erased while using it in this block.
        let _fst_read_guard = self.fst_pool.lock_read_access();

        if let Ok(fst_store) = self.fst_pool.acquire(collection, bucket) {
            tracing::debug!("running list");

            let fst_repo = fst_store.to_repository();

            return fst_repo.list_words(limit as usize, offset as usize);
        }

        Err(())
    }
}
