// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use core::cmp::Eq;
use core::hash::Hash;
use hashbrown::HashMap;
use std::fmt::Display;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

pub trait StoreGeneric {
    fn ref_last_used(&self) -> &RwLock<SystemTime>;
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
        tracing::debug!(
            "{} store acquired from pool for collection: {} (pool key: {})",
            kind,
            collection_str,
            pool_key
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
        builder: &B,
    ) -> Result<Arc<S>, ()> {
        match builder.build(pool_key) {
            Ok(store) => {
                // Acquire a thread-safe store pool reference in write mode
                let mut store_pool_write = pool.write().unwrap();
                let store_box = Arc::new(store);

                store_pool_write.insert(pool_key, store_box.clone());

                tracing::debug!(
                    "opened and cached {} store in pool for collection: {} (pool key: {})",
                    kind,
                    collection_str,
                    pool_key
                );

                Ok(store_box)
            }
            Err(_) => {
                tracing::error!(
                    "failed opening {} store for collection: {} (pool key: {})",
                    kind,
                    collection_str,
                    pool_key
                );

                Err(())
            }
        }
    }

    fn proceed_janitor(
        kind: &str,
        pool: &Arc<RwLock<HashMap<K, Arc<S>>>>,
        inactive_after: u64,
        access_lock: &Arc<RwLock<()>>,
    ) {
        tracing::debug!("scanning for {} store pool items to janitor", kind);

        let mut removal_register: Vec<K> = Vec::new();

        for (collection_bucket, store) in pool.read().unwrap().iter() {
            // Important: be lenient with system clock going back to a past duration, since \
            //   we may be running in a virtualized environment where clock is not guaranteed \
            //   to be monotonic. This is done to avoid poisoning associated mutexes by \
            //   crashing on unwrap().
            let last_used_elapsed = store
                .ref_last_used()
                .read()
                .unwrap()
                .elapsed()
                .unwrap_or_else(|err| {
                    tracing::error!(
                        "store pool item: {} last used duration clock issue, zeroing: {}",
                        collection_bucket,
                        err
                    );

                    // Assuming a zero seconds fallback duration
                    Duration::from_secs(0)
                })
                .as_secs();

            if last_used_elapsed >= inactive_after {
                tracing::debug!(
                    "found expired {} store pool item: {}; elapsed time: {}s",
                    kind,
                    collection_bucket,
                    last_used_elapsed
                );

                // Notice: the bucket value needs to be cloned, as we cannot reference as value \
                //   that will outlive referenced value once we remove it from its owner set.
                removal_register.push(*collection_bucket);
            } else {
                tracing::debug!(
                    "found non-expired {} store pool item: {}; elapsed time: {}s",
                    kind,
                    collection_bucket,
                    last_used_elapsed
                );
            }
        }

        if !removal_register.is_empty() {
            // Block structural operations only when there is actual removal work.
            let _access = access_lock.write().unwrap();
            let mut store_pool_write = pool.write().unwrap();

            for collection_bucket in &removal_register {
                let should_remove = store_pool_write
                    .get(collection_bucket)
                    .and_then(|store| store.ref_last_used().read().ok())
                    .and_then(|last_used| last_used.elapsed().ok())
                    .is_some_and(|elapsed| elapsed.as_secs() >= inactive_after);
                if should_remove {
                    store_pool_write.remove(collection_bucket);
                }
            }
        }

        tracing::info!(
            "done scanning for {} store pool items to janitor, expired {} items, now has {} items",
            kind,
            removal_register.len(),
            pool.read().unwrap().len()
        );
    }
}

pub trait StoreGenericBuilder<K, S> {
    fn build(&self, pool_key: K) -> Result<S, ()>;
}

pub trait StoreGenericActionBuilder {
    fn proceed_erase_collection(&self, collection_str: &str) -> Result<u32, ()>;

    fn proceed_erase_bucket(&self, collection_str: &str, bucket_str: &str) -> Result<u32, ()>;

    fn dispatch_erase<T: AsRef<str>>(
        &self,
        kind: &str,
        collection: T,
        bucket: Option<T>,
    ) -> Result<u32, ()> {
        let collection = collection.as_ref();

        tracing::info!("{} erase requested on collection: {}", kind, collection);

        if let Some(bucket) = bucket {
            self.proceed_erase_bucket(collection, bucket.as_ref())
        } else {
            self.proceed_erase_collection(collection)
        }
    }
}
