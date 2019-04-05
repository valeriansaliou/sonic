// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use core::cmp::Eq;
use core::hash::Hash;
use hashbrown::HashMap;
use std::fmt::Display;
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

pub trait StoreGenericKey {}

pub trait StoreGeneric {
    fn ref_last_used<'a>(&'a self) -> &'a RwLock<SystemTime>;
}

pub trait StoreGenericPool<
    K: Hash + Eq + Copy + Display,
    S: StoreGeneric,
    B: StoreGenericBuilder<K, S>,
>
{
    fn proceed_acquire_cache(
        kind: &str,
        collection_str: &str,
        pool_key: K,
        store: &Arc<S>,
    ) -> Result<Arc<S>, ()> {
        debug!(
            "{} store acquired from pool for collection: {} (pool key: {})",
            kind, collection_str, pool_key
        );

        // Bump store last used date (avoids early janitor eviction)
        let mut last_used_value = store.ref_last_used().write().unwrap();

        *last_used_value = SystemTime::now();

        // Perform an early drop of the lock (frees up write lock early)
        drop(last_used_value);

        Ok(store.clone())
    }

    fn proceed_acquire_open(
        kind: &str,
        collection_str: &str,
        pool_key: K,
        pool: &Arc<RwLock<HashMap<K, Arc<S>>>>,
    ) -> Result<Arc<S>, ()> {
        match B::new(pool_key) {
            Ok(store) => {
                // Acquire a thread-safe store pool reference in write mode
                let mut store_pool_write = pool.write().unwrap();
                let store_box = Arc::new(store);

                store_pool_write.insert(pool_key, store_box.clone());

                debug!(
                    "opened and cached {} store in pool for collection: {} (pool key: {})",
                    kind, collection_str, pool_key
                );

                Ok(store_box)
            }
            Err(err) => {
                error!(
                    "failed opening {} store for collection: {} (pool key: {})",
                    kind, collection_str, pool_key
                );

                Err(err)
            }
        }
    }

    fn proceed_janitor(
        kind: &str,
        pool: &Arc<RwLock<HashMap<K, Arc<S>>>>,
        inactive_after: u64,
        access_lock: &Arc<RwLock<bool>>,
    ) {
        debug!("scanning for {} store pool items to janitor", kind);

        // Acquire access lock (in blocking write mode), and reference it in context
        // Notice: this prevents store to be acquired from any context
        let _access = access_lock.write().unwrap();

        let mut removal_register: Vec<K> = Vec::new();

        for (collection_bucket, store) in pool.read().unwrap().iter() {
            let last_used_elapsed = store
                .ref_last_used()
                .read()
                .unwrap()
                .elapsed()
                .unwrap()
                .as_secs();

            if last_used_elapsed >= inactive_after {
                debug!(
                    "found expired {} store pool item: {}; elapsed time: {}s",
                    kind, collection_bucket, last_used_elapsed
                );

                // Notice: the bucket value needs to be cloned, as we cannot reference as value \
                //   that will outlive referenced value once we remove it from its owner set.
                removal_register.push(*collection_bucket);
            } else {
                debug!(
                    "found non-expired {} store pool item: {}; elapsed time: {}s",
                    kind, collection_bucket, last_used_elapsed
                );
            }
        }

        if !removal_register.is_empty() {
            let mut store_pool_write = pool.write().unwrap();

            for collection_bucket in &removal_register {
                store_pool_write.remove(collection_bucket);
            }
        }

        info!(
            "done scanning for {} store pool items to janitor, expired {} items, now has {} items",
            kind,
            removal_register.len(),
            pool.read().unwrap().len()
        );
    }
}

pub trait StoreGenericBuilder<K, S> {
    fn new(pool_key: K) -> Result<S, ()>;
}

pub trait StoreGenericActionBuilder {
    fn proceed_erase_collection(collection_str: &str) -> Result<u32, ()>;

    fn proceed_erase_bucket(collection_str: &str, bucket_str: &str) -> Result<u32, ()>;

    fn dispatch_erase<'a, T: Into<&'a str>>(
        kind: &str,
        collection: T,
        bucket: Option<T>,
        access_lock: &Arc<RwLock<bool>>,
    ) -> Result<u32, ()> {
        let collection_str = collection.into();

        info!("{} erase requested on collection: {}", kind, collection_str);

        // Acquire access lock (in blocking write mode), and reference it in context
        // Notice: this prevents store to be acquired from any context
        let _access = access_lock.write().unwrap();

        if let Some(bucket) = bucket {
            Self::proceed_erase_bucket(collection_str, bucket.into())
        } else {
            Self::proceed_erase_collection(collection_str)
        }
    }
}
