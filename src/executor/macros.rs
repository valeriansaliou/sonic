// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

#[macro_export]
macro_rules! executor_kv_lock_read {
    ($store:ident) => {
        let kv_store_reference = $store.clone();

        let _kv_store_lock = kv_store_reference.as_ref().map(|inner| inner.lock.read().unwrap());
    };
}

#[macro_export]
macro_rules! executor_kv_lock_write {
    ($store:ident) => {
        let kv_store_reference = $store.clone();

        let _kv_store_lock = kv_store_reference.as_ref().map(|inner| inner.lock.write().unwrap());
    };
}
