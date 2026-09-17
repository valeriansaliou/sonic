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

pub trait StoreGenericPool:
    std::ops::Deref<Target = RwLock<HashMap<Self::StoreId, Arc<Self::Store>, Self::HashBuilder>>>
{
    type StoreId: Hash + Eq;
    type Store: StoreGeneric;
    type HashBuilder: std::hash::BuildHasher;

    fn kind() -> &'static str;

    fn consider_inactive_after_secs(&self) -> u64;

    fn access_lock(&self) -> &RwLock<()>;

    fn proceed_erase_collection(&self, collection_str: &str) -> Result<u32, ()>;

    fn proceed_erase_bucket(&self, collection_str: &str, bucket_str: &str) -> Result<u32, ()>;
}

pub(super) trait StoreGenericPoolExt: StoreGenericPool {
    fn proceed_acquire_cache(
        collection_str: &str,
        store_id: Self::StoreId,
        store: &Arc<Self::Store>,
    ) -> Result<Arc<Self::Store>, ()>
    where
        Self::StoreId: Display,
    {
        let kind = Self::kind();

        tracing::debug!(
            "{kind} store acquired from pool for collection: {} (id: {})",
            collection_str,
            store_id
        );

        // Bump store last used date (avoids early janitor eviction)
        *store.ref_last_used().write().unwrap() = SystemTime::now();

        Ok(Arc::clone(store))
    }

    #[allow(clippy::type_complexity, reason = "We can’t avoid it")]
    fn proceed_acquire_open<'a>(
        &'a self,
        collection_str: &str,
        store_id: Self::StoreId,
        build: impl FnOnce(&'a Self, Self::StoreId) -> Result<Self::Store, ()>,
        write_guard: Option<
            &mut RwLockWriteGuard<'a, HashMap<Self::StoreId, Arc<Self::Store>, Self::HashBuilder>>,
        >,
    ) -> Result<Arc<Self::Store>, ()>
    where
        Self::StoreId: Display + Copy,
    {
        let kind = Self::kind();

        match build(self, store_id) {
            Ok(store) => {
                // Acquire a thread-safe store pool reference in write mode
                let store_pool_write = match write_guard {
                    Some(x) => x,
                    None => &mut self.write().unwrap(),
                };
                let store_box = Arc::new(store);

                store_pool_write.insert(store_id, Arc::clone(&store_box));

                tracing::debug!(
                    "opened and cached {kind} store in pool for collection: {collection_str} (id: {store_id})"
                );

                Ok(store_box)
            }
            Err(_) => {
                tracing::error!(
                    "failed opening {kind} store for collection: {collection_str} (id: {store_id})"
                );

                Err(())
            }
        }
    }

    fn proceed_janitor(&self, filter: impl Fn(&Self::StoreId) -> bool)
    where
        Self::StoreId: Display + Copy,
    {
        let kind = Self::kind();

        tracing::debug!("scanning for {kind} store pool items to janitor");

        // Acquire access lock (in blocking write mode), and reference it in context
        // Notice: this prevents store to be acquired from any context
        let _access = self.access_lock().write().unwrap();

        let mut removal_register: Vec<Self::StoreId> = Vec::new();

        let store_pool_read = self.read().unwrap();

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

            if last_used_elapsed.as_secs() >= self.consider_inactive_after_secs() {
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

            let mut store_pool_write = self.write().unwrap();

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

    fn dispatch_erase(
        &self,
        collection: impl AsRef<str>,
        bucket: Option<impl AsRef<str>>,
    ) -> Result<u32, ()> {
        let kind = Self::kind();
        let collection = collection.as_ref();

        tracing::info!("{kind} erase requested on collection: {collection}");

        if let Some(bucket) = bucket {
            self.proceed_erase_bucket(collection, bucket.as_ref())
        } else {
            self.proceed_erase_collection(collection)
        }
    }
}

impl<Pool: StoreGenericPool> StoreGenericPoolExt for Pool {}

pub(super) fn u32_from_base16(str_b16: &str) -> Result<u32, std::io::Error> {
    use radix::RadixNum;
    use std::io;

    const ATOM_HASH_RADIX: usize = 16;

    let decimal: usize = RadixNum::from_str(str_b16, ATOM_HASH_RADIX)
        .and_then(|num| num.as_decimal())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;

    u32::try_from(decimal).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
}
