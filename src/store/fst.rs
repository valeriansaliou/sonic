// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use fst::{Error as FSTError, Set as FSTSet};
use hashbrown::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

use super::item::StoreItemPart;
use crate::APP_CONF;

pub struct StoreFSTPool;
pub struct StoreFSTBuilder;

pub struct StoreFST {
    graph: FSTSet,
    pending: StoreFSTPending,
    last_used: Arc<RwLock<SystemTime>>,
}

#[derive(Default)]
pub struct StoreFSTPending {
    pop: HashSet<String>,
    push: HashSet<String>,
}

pub struct StoreFSTActionBuilder;

pub struct StoreFSTAction<'a> {
    store: StoreFSTBox,
    bucket: StoreItemPart<'a>,
}

type StoreFSTBox = Arc<StoreFST>;

lazy_static! {
    static ref GRAPH_POOL: Arc<RwLock<HashMap<(String, String), StoreFSTBox>>> =
        Arc::new(RwLock::new(HashMap::new()));
}

impl StoreFSTPool {
    // TODO: implement locks when re-building the fst? (is it necessary?)

    // pub fn acquire<'a, T: Into<&'a str>>(collection: T, bucket: T) -> Result<StoreFSTBox, FSTError> {
    //     let (collection_str, bucket_str) = (collection.into(), bucket.into());

    //     // Acquire general lock, and reference it in context
    //     // Notice: this prevents database to be opened while also erased; or 2 databases on the \
    //     //   same collection to be opened at the same time.
    //     let _write = STORE_WRITE_LOCK.lock().unwrap();

    //     // Acquire a thread-safe store pool reference in read mode
    //     let graph_pool_read = GRAPH_POOL.read().unwrap();

    //     if let Some(store_fst) = graph_pool_read.get(collection_str) {
    //         debug!(
    //             "fst store acquired from pool for collection: {} and bucket: {}",
    //             collection_str, bucket_str
    //         );

    //         // Bump store last used date (avoids early janitor eviction)
    //         let mut last_used_value = store_fst.last_used.write().unwrap();

    //         mem::replace(&mut *last_used_value, SystemTime::now());

    //         // Perform an early drop of the lock (frees up write lock early)
    //         drop(last_used_value);

    //         Ok(store_fst.clone())
    //     } else {
    //         info!(
    //             "fst store not in pool for collection: {} and bucket: {}, opening it",
    //             collection_str, bucket_str
    //         );

    //         match StoreFSTBuilder::new(collection_str, bucket_str) {
    //             Ok(store_fst) => {
    //                 // Important: we need to drop the read reference first, to avoid dead-locking \
    //                 //   when acquiring the RWLock in write mode in this block.
    //                 drop(graph_pool_read);

    //                 // Acquire a thread-safe store pool reference in write mode
    //                 let mut graph_pool_write = GRAPH_POOL.write().unwrap();
    //                 let store_fst_box = Arc::new(store_fst);

    //                 graph_pool_write.insert(collection_str.to_string(), store_fst_box.clone());

    //                 debug!(
    //                     "opened and cached store in pool for collection: {} and bucket: {}",
    //                     collection_str, bucket_str
    //                 );

    //                 Ok(store_fst_box)
    //             }
    //             Err(err) => {
    //                 error!(
    //                     "failed opening store for collection: {} because: {} and bucket: {}",
    //                     collection_str, bucket_str, err
    //                 );

    //                 Err(err)
    //             }
    //         }
    //     }
    // }

    pub fn janitor() {
        debug!("scanning for fst store pool items to janitor");

        let mut store_pool_write = GRAPH_POOL.write().unwrap();
        let mut removal_register: Vec<(String, String)> = Vec::new();

        for (collection_bucket, store_fst) in store_pool_write.iter() {
            let last_used = store_fst.last_used.read().unwrap();

            if last_used.elapsed().unwrap().as_secs() >= APP_CONF.store.fst.pool.inactive_after {
                debug!(
                    "found expired fst store pool item: {}/{} with last used time: {:?}",
                    &collection_bucket.0, &collection_bucket.1, last_used
                );

                // TODO: isnt it dirty to clone value there?
                removal_register.push(collection_bucket.to_owned());
            }
        }

        for collection_bucket in &removal_register {
            store_pool_write.remove(collection_bucket);
        }

        info!(
            "done scanning for fst store pool items to janitor, expired {} items, now has {} items",
            removal_register.len(),
            store_pool_write.len()
        );
    }
}

impl StoreFSTBuilder {
    pub fn new(collection: &str, bucket: &str) -> Result<StoreFST, FSTError> {
        Self::open(collection, bucket).map(|graph| StoreFST {
            graph: graph,
            pending: StoreFSTPending::default(),
            last_used: Arc::new(RwLock::new(SystemTime::now())),
        })
    }

    fn open(collection: &str, bucket: &str) -> Result<FSTSet, FSTError> {
        debug!(
            "opening finite-state transducer graph for collection: {} and bucket: {}",
            collection, bucket
        );

        // TODO: IMPORTANT >> It is up to the caller to enforce that the memory map is not \
        //   modified while it is opened. >> we need to ensure proper locking and avoid opening \
        //   this mmap file while a producer is writing to it, otherwise boom, crash.

        // Open database at path for collection
        // Notice: this is unsafe, as loaded memory is a memory-mapped file, that cannot be \
        //   garanteed not to be muted while we own a read handle to it.
        unsafe { FSTSet::from_path(Self::path(collection, bucket)) }
    }

    fn path(collection: &str, bucket: &str) -> PathBuf {
        let bucket_fst = format!("{}.fst", bucket);

        APP_CONF.store.fst.path.join(collection).join(bucket_fst)
    }
}

impl StoreFST {
    pub fn contains(&self, sequence: &str) -> bool {
        // 1. Check in 'pop' set (if value is inside, it means it should not exist)
        if self.pending.pop.contains(sequence) == true {
            return false;
        }

        // 2. Check in 'push' set (if value is inside, it means it should exist)
        if self.pending.push.contains(sequence) == true {
            return true;
        }

        // 3. Check in 'fst' (final consolidated graph)
        self.graph.contains(sequence)
    }
}

impl StoreFSTActionBuilder {
    pub fn read<'a>(bucket: StoreItemPart<'a>, store: StoreFSTBox) -> StoreFSTAction<'a> {
        let action = Self::build(bucket, store);

        debug!("begin action read block");

        // TODO: handle the rwlock things on (collection, bucket) tuple (unpack bucket store \
        //   and return it); read lock; return a lock guard to ensure it auto-unlocks when caller \
        //   goes out of scope.

        debug!("began action read block");

        action
    }

    pub fn write<'a>(bucket: StoreItemPart<'a>, store: StoreFSTBox) -> StoreFSTAction<'a> {
        let action = Self::build(bucket, store);

        debug!("begin action write block");

        // TODO: handle the rwlock things on (collection, bucket) tuple (unpack bucket store \
        //   and return it); write lock; return a lock guard to ensure it auto-unlocks when caller \
        //   goes out of scope.

        debug!("began action write block");

        action
    }

    pub fn erase<'a, T: Into<&'a str>>(collection: T) -> Result<u32, ()> {
        let collection_str = collection.into();

        info!("erase requested on collection: {}", collection_str);

        // TODO

        Err(())
    }

    fn build<'a>(bucket: StoreItemPart<'a>, store: StoreFSTBox) -> StoreFSTAction<'a> {
        StoreFSTAction {
            store: store,
            bucket: bucket,
        }
    }
}

impl<'a> StoreFSTAction<'a> {
    // TODO: CHANNEL SUGGEST -> suggest FST
    // TODO: CHANNEL SEARCH -> complete not-found words via FST +/ levenshtein distance complete
    // TODO: CHANNEL PUSH -> push_word for each word
    // TODO: CHANNEL POP + FLUSHO -> pop_word for each word
    // TODO: CHANNEL FLUSHB + FLUSHC -> erase FST

    pub fn push_word(&self, word: &str) -> Result<(), ()> {
        // TODO: nuke from pop if exists in pop first
        // TODO: add in push

        Err(())
    }

    pub fn pop_word(&self, word: &str) -> Result<(), ()> {
        // TODO: nuke from push if exists in push first
        // TODO: add in pop

        Err(())
    }

    pub fn has_word(&self, word: &str) -> Result<bool, ()> {
        Ok(self.store.contains(word))
    }

    pub fn suggest_word(&self, from_word: &str, limit: u16) -> Result<Vec<String>, ()> {
        // TODO: search 'limit' words in FST
        // TODO: check if 'store.contains' to avoid removed words

        Err(())
    }
}
