// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

#[macro_export]
macro_rules! executor_kv_lock_read {
    ($store:ident) => {
        let kv_store_reference = $store.clone();

        let _kv_store_lock = kv_store_reference
            .as_ref()
            .map(|inner| inner.lock.read().unwrap());
    };
}

#[macro_export]
macro_rules! executor_kv_lock_write {
    ($store:ident) => {
        let kv_store_reference = $store.clone();

        let _kv_store_lock = kv_store_reference
            .as_ref()
            .map(|inner| inner.lock.write().unwrap());
    };
}

#[macro_export]
macro_rules! general_kv_access_lock_read {
    () => {
        use crate::store::kv::STORE_ACCESS_LOCK;

        let _kv_access = STORE_ACCESS_LOCK.read().unwrap();
    };
}

#[macro_export]
macro_rules! general_fst_access_lock_read {
    () => {
        use crate::store::fst::GRAPH_ACCESS_LOCK;

        let _fst_access = GRAPH_ACCESS_LOCK.read().unwrap();
    };
}
