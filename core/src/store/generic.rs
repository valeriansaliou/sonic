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
use std::sync::{Arc, RwLock, RwLockWriteGuard};
use std::time::{Duration, SystemTime};

pub trait StoreGeneric {
    fn ref_last_used(&self) -> &RwLock<SystemTime>;
}

pub(super) fn proceed_acquire_cache<Pool: StoreGenericPool>(
    collection_str: &str,
    pool_key: Pool::Key,
    store: &Arc<Pool::Store>,
) -> Result<Arc<Pool::Store>, ()>
where
    Pool::Key: Display,
{
    let kind = Pool::kind();

    tracing::debug!(
        "{kind} store acquired from pool for collection: {} (pool key: {})",
        collection_str,
        pool_key
    );

    // Bump store last used date (avoids early janitor eviction)
    *store.ref_last_used().write().unwrap() = SystemTime::now();

    Ok(Arc::clone(store))
}

pub(super) fn proceed_acquire_open<'a, Pool: StoreGenericPool>(
    pool: &'a Pool,
    collection_str: &str,
    pool_key: Pool::Key,
    build: impl FnOnce(&'a Pool, Pool::Key) -> Result<Pool::Store, ()>,
    write_guard: Option<
        &mut RwLockWriteGuard<'a, HashMap<Pool::Key, Arc<Pool::Store>, Pool::HashBuilder>>,
    >,
) -> Result<Arc<Pool::Store>, ()>
where
    Pool::Key: Display + Copy,
{
    let kind = Pool::kind();

    match build(pool, pool_key) {
        Ok(store) => {
            // Acquire a thread-safe store pool reference in write mode
            let store_pool_write = match write_guard {
                Some(x) => x,
                None => &mut pool.write().unwrap(),
            };
            let store_box = Arc::new(store);

            store_pool_write.insert(pool_key, Arc::clone(&store_box));

            tracing::debug!(
                "opened and cached {kind} store in pool for collection: {collection_str} (pool key: {pool_key})"
            );

            Ok(store_box)
        }
        Err(_) => {
            tracing::error!(
                "failed opening {kind} store for collection: {collection_str} (pool key: {pool_key})"
            );

            Err(())
        }
    }
}

pub(super) fn proceed_janitor<Pool: StoreGenericPool>(
    pool: &Pool,
    filter: impl Fn(&Pool::Key) -> bool,
) where
    Pool::Key: Display + Copy,
{
    let kind = Pool::kind();

    tracing::debug!("scanning for {kind} store pool items to janitor");

    // Acquire access lock (in blocking write mode), and reference it in context
    // Notice: this prevents store to be acquired from any context
    let _access = pool.access_lock().write().unwrap();

    let mut removal_register: Vec<Pool::Key> = Vec::new();

    let store_pool_read = pool.read().unwrap();

    for (collection_bucket, store) in store_pool_read.iter().filter(|(key, _)| filter(key)) {
        // Important: be lenient with system clock going back to a past duration, since \
        //   we may be running in a virtualized environment where clock is not guaranteed \
        //   to be monotonic. This is done to avoid poisoning associated mutexes by \
        //   crashing on unwrap().
        let last_used_elapsed = (store.ref_last_used().read().unwrap())
            .elapsed()
            .unwrap_or_else(|err| {
                tracing::error!(
                    "store pool item: {} last used duration clock issue, zeroing: {}",
                    collection_bucket,
                    err
                );

                // Assuming a zero seconds fallback duration
                Duration::ZERO
            });

        if last_used_elapsed.as_secs() >= pool.consider_inactive_after_secs() {
            tracing::debug!(
                "found expired {kind} store pool item: {}; elapsed time: {last_used_elapsed:.1?}",
                collection_bucket
            );

            // Notice: the bucket value needs to be cloned, as we cannot reference as value \
            //   that will outlive referenced value once we remove it from its owner set.
            removal_register.push(*collection_bucket);
        } else {
            tracing::debug!(
                "found non-expired {kind} store pool item: {}; elapsed time: {last_used_elapsed:.1?}",
                collection_bucket
            );
        }
    }

    let store_pool_read = if removal_register.is_empty() {
        store_pool_read
    } else {
        drop(store_pool_read);

        let mut store_pool_write = pool.write().unwrap();

        for collection_bucket in removal_register.iter() {
            store_pool_write.remove(collection_bucket);
        }

        RwLockWriteGuard::downgrade(store_pool_write)
    };

    tracing::info!(
        "done scanning for {kind} store pool items to janitor, expired {} items, now has {} items",
        removal_register.len(),
        store_pool_read.len(),
    );
}

pub(super) fn dispatch_erase<P: StoreGenericPool>(
    pool: &P,
    collection: impl AsRef<str>,
    bucket: Option<impl AsRef<str>>,
) -> Result<u32, ()> {
    let collection = collection.as_ref();

    tracing::info!("{} erase requested on collection: {collection}", P::kind());

    if let Some(bucket) = bucket {
        pool.proceed_erase_bucket(collection, bucket.as_ref())
    } else {
        pool.proceed_erase_collection(collection)
    }
}

pub trait StoreGenericPool:
    std::ops::Deref<Target = RwLock<HashMap<Self::Key, Arc<Self::Store>, Self::HashBuilder>>>
{
    type Key: Hash + Eq;
    type Store: StoreGeneric;
    type HashBuilder: std::hash::BuildHasher;

    fn kind() -> &'static str;

    fn consider_inactive_after_secs(&self) -> u64;

    fn access_lock(&self) -> &RwLock<()>;

    fn proceed_erase_collection(&self, collection_str: &str) -> Result<u32, ()>;

    fn proceed_erase_bucket(&self, collection_str: &str, bucket_str: &str) -> Result<u32, ()>;
}
