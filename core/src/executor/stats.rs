// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Crisp IM SAS
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::store::kv::StoreKVAcquireMode;
use crate::store::stats::StoreCollectionStats;

impl super::Executor {
    pub fn stats(&self, collection: &str, deep: bool) -> Result<StoreCollectionStats, ()> {
        let _guard = self.kv_pool.lock_read_access();
        let store = self
            .kv_pool
            .acquire(StoreKVAcquireMode::OpenOnly, collection)?;
        executor_kv_lock_read!(store);
        store
            .as_ref()
            .map_or_else(|| Err(()), |store| store.stats(collection, deep))
    }
}
