// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use core::cmp::Eq;
use core::hash::Hash;
use hashbrown::HashMap;
use std::fmt::Debug;
use std::mem;
use std::sync::{Arc, Mutex, RwLock};
use std::time::SystemTime;

pub trait StoreGeneric {
    fn ref_last_used<'a>(&'a self) -> &'a RwLock<SystemTime>;
}

pub trait StoreGenericPool<
    A: Hash + Eq + Copy + Debug,
    S: StoreGeneric,
    B: StoreGenericBuilder<A, S>,
>
{
    fn proceed_acquire_cache(
        kind: &str,
        collection_str: &str,
        bucket_str: &str,
        pool_key: (A, A),
        store: &Arc<S>,
    ) -> Result<Arc<S>, ()> {
        debug!(
            "{} store acquired from pool for collection: {} <{:x?}> / bucket: {} <{:x?}>",
            kind, collection_str, pool_key.0, bucket_str, pool_key.1
        );

        // Bump store last used date (avoids early janitor eviction)
        let mut last_used_value = store.ref_last_used().write().unwrap();

        mem::replace(&mut *last_used_value, SystemTime::now());

        // Perform an early drop of the lock (frees up write lock early)
        drop(last_used_value);

        Ok(store.clone())
    }

    fn proceed_acquire_open(
        kind: &str,
        collection_str: &str,
        bucket_str: &str,
        pool_key: (A, A),
        pool: &Arc<RwLock<HashMap<(A, A), Arc<S>>>>,
    ) -> Result<Arc<S>, ()> {
        match B::new(pool_key.0, pool_key.1) {
            Ok(store) => {
                // Acquire a thread-safe store pool reference in write mode
                let mut store_pool_write = pool.write().unwrap();
                let store_box = Arc::new(store);

                store_pool_write.insert(pool_key, store_box.clone());

                debug!(
                    "opened and cached {} store in pool for collection: {} and bucket: {}",
                    kind, collection_str, bucket_str
                );

                Ok(store_box)
            }
            Err(err) => {
                error!(
                    "failed opening {} store for collection: {} and bucket: {}",
                    kind, collection_str, bucket_str
                );

                Err(err)
            }
        }
    }

    fn proceed_janitor(
        kind: &str,
        pool: &Arc<RwLock<HashMap<(A, A), Arc<S>>>>,
        inactive_after: u64,
    ) {
        debug!("scanning for {} store pool items to janitor", kind);

        let mut store_pool_write = pool.write().unwrap();
        let mut removal_register: Vec<(A, A)> = Vec::new();

        for (collection_bucket, store) in store_pool_write.iter() {
            let last_used_elapsed = store
                .ref_last_used()
                .read()
                .unwrap()
                .elapsed()
                .unwrap()
                .as_secs();

            if last_used_elapsed >= inactive_after {
                debug!(
                    "found expired {} store pool item: <{:x?}>/<{:x?}>; elapsed time: {}s",
                    kind, &collection_bucket.0, &collection_bucket.1, last_used_elapsed
                );

                // Notice: the bucket value needs to be cloned, as we cannot reference as value \
                //   that will outlive referenced value once we remove it from its owner set.
                removal_register.push(*collection_bucket);
            } else {
                debug!(
                    "found non-expired {} store pool item: <{:x?}>/<{:x?}>; elapsed time: {}s",
                    kind, &collection_bucket.0, &collection_bucket.1, last_used_elapsed
                );
            }
        }

        for collection_bucket in &removal_register {
            store_pool_write.remove(collection_bucket);
        }

        info!(
            "done scanning for {} store pool items to janitor, expired {} items, now has {} items",
            kind,
            removal_register.len(),
            store_pool_write.len()
        );
    }
}

pub trait StoreGenericBuilder<A, S> {
    fn new(collection_hash: A, bucket_hash: A) -> Result<S, ()>;
}

pub trait StoreGenericActionBuilder<B, A> {
    fn build(store: B) -> A;

    fn proceed_erase_bucket(collection_str: &str, bucket_str: &str) -> Result<u32, ()>;

    fn proceed_erase_collection(collection_str: &str) -> Result<u32, ()>;

    fn dispatch_erase<'a, T: Into<&'a str>>(
        kind: &str,
        collection: T,
        bucket: Option<T>,
        access_lock: &Arc<RwLock<bool>>,
        write_lock: &Arc<Mutex<bool>>,
    ) -> Result<u32, ()> {
        let collection_str = collection.into();

        info!("{} erase requested on collection: {}", kind, collection_str);

        // Acquire write + access locks, and reference it in context
        // Notice: write lock prevents store to be acquired from any context; while access lock \
        //   lets the erasure process wait that any thread using the store is done with work.
        let (_access, _write) = (access_lock.write().unwrap(), write_lock.lock().unwrap());

        if let Some(bucket) = bucket {
            Self::proceed_erase_bucket(collection_str, bucket.into())
        } else {
            Self::proceed_erase_collection(collection_str)
        }
    }
}
