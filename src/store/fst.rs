// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use fst::{Error as FSTError, Set as FSTSet, SetBuilder as FSTSetBuilder, Streamer};
use hashbrown::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::BufWriter;
use std::mem;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::SystemTime;

use crate::APP_CONF;

pub struct StoreFSTPool;
pub struct StoreFSTBuilder;

pub struct StoreFST {
    graph: FSTSet,
    target: StoreFSTKey,
    pending: StoreFSTPending,
    last_used: Arc<RwLock<SystemTime>>,
}

#[derive(Default)]
pub struct StoreFSTPending {
    pop: Arc<RwLock<HashSet<Vec<u8>>>>,
    push: Arc<RwLock<HashSet<Vec<u8>>>>,
}

pub struct StoreFSTActionBuilder;

pub struct StoreFSTAction {
    store: StoreFSTBox,
}

#[derive(Copy, Clone)]
enum StoreFSTPathMode {
    Permanent,
    Temporary,
}

type StoreFSTBox = Arc<StoreFST>;
type StoreFSTKey = (String, String);

lazy_static! {
    pub static ref GRAPH_ACCESS_LOCK: Arc<RwLock<bool>> = Arc::new(RwLock::new(false));
    static ref GRAPH_WRITE_LOCK: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    static ref GRAPH_POOL: Arc<RwLock<HashMap<StoreFSTKey, StoreFSTBox>>> =
        Arc::new(RwLock::new(HashMap::new()));
    static ref GRAPH_CONSOLIDATE: Arc<RwLock<HashSet<StoreFSTKey>>> =
        Arc::new(RwLock::new(HashSet::new()));
}

impl StoreFSTPathMode {
    fn extension(&self) -> &'static str {
        match self {
            StoreFSTPathMode::Permanent => ".fst",
            StoreFSTPathMode::Temporary => ".tmp",
        }
    }
}

impl StoreFSTPool {
    // TODO: implement locks when re-building the fst? (is it necessary?)

    pub fn acquire<'a, T: Into<&'a str>>(
        collection: T,
        bucket: T,
    ) -> Result<StoreFSTBox, FSTError> {
        let (collection_str, bucket_str) = (collection.into(), bucket.into());
        let pool_key = (collection_str.to_string(), bucket_str.to_string());

        // Acquire general lock, and reference it in context
        // Notice: this prevents graph to be opened while also erased; or 2 graphs on the \
        //   same collection to be opened at the same time.
        let _write = GRAPH_WRITE_LOCK.lock().unwrap();

        // Acquire a thread-safe store pool reference in read mode
        let graph_pool_read = GRAPH_POOL.read().unwrap();

        if let Some(store_fst) = graph_pool_read.get(&pool_key) {
            debug!(
                "fst store acquired from pool for collection: {} and bucket: {}",
                collection_str, bucket_str
            );

            // Bump store last used date (avoids early janitor eviction)
            let mut last_used_value = store_fst.last_used.write().unwrap();

            mem::replace(&mut *last_used_value, SystemTime::now());

            // Perform an early drop of the lock (frees up write lock early)
            drop(last_used_value);

            Ok(store_fst.clone())
        } else {
            info!(
                "fst store not in pool for collection: {} and bucket: {}, opening it",
                collection_str, bucket_str
            );

            match StoreFSTBuilder::new(collection_str, bucket_str) {
                Ok(store_fst) => {
                    // Important: we need to drop the read reference first, to avoid dead-locking \
                    //   when acquiring the RWLock in write mode in this block.
                    drop(graph_pool_read);

                    // Acquire a thread-safe store pool reference in write mode
                    let mut graph_pool_write = GRAPH_POOL.write().unwrap();
                    let store_fst_box = Arc::new(store_fst);

                    graph_pool_write.insert(pool_key, store_fst_box.clone());

                    debug!(
                        "opened and cached store in pool for collection: {} and bucket: {}",
                        collection_str, bucket_str
                    );

                    Ok(store_fst_box)
                }
                Err(err) => {
                    error!(
                        "failed opening store for collection: {} because: {} and bucket: {}",
                        collection_str, bucket_str, err
                    );

                    Err(err)
                }
            }
        }
    }

    pub fn janitor() {
        debug!("scanning for fst store pool items to janitor");

        let mut store_pool_write = GRAPH_POOL.write().unwrap();
        let mut removal_register: Vec<StoreFSTKey> = Vec::new();

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

    pub fn consolidate() {
        debug!("scanning for fst store pool items to consolidate");

        // TODO: do not consolidate as often please, try to consolidate every 5 min rather than \
        //   every 30 seconds. Lower the HZ of the tasker system for certain heavy tasks, or \
        //   check a 'last_consolidated' time for each store and only proceed the old enough ones, \
        //   which is better to spread out consolidation steps over time over a large number of \
        //   very active buckets.

        // Acquire write + access locks, and reference it in context
        // Notice: write lock prevents graph to be acquired from any context
        let _write = GRAPH_WRITE_LOCK.lock().unwrap();

        let (mut count_moved, mut count_pushed, mut count_popped) = (0, 0, 0);

        let mut graph_consolidate_write = GRAPH_CONSOLIDATE.write().unwrap();

        if graph_consolidate_write.len() > 0 {
            let mut graph_pool_write = GRAPH_POOL.write().unwrap();

            // Prepare close stack (used once whole set is scanned)
            let mut close_stack: Vec<&StoreFSTKey> = Vec::new();

            // Proceed FST consolidation for each store key
            for key in &*graph_consolidate_write {
                if let Some(store) = graph_pool_write.get(&key) {
                    let consolidate_counts = Self::consolidate_item(store);

                    count_moved += consolidate_counts.1;
                    count_pushed += consolidate_counts.2;
                    count_popped += consolidate_counts.3;

                    // Stack cached store close request?
                    if consolidate_counts.0 == true {
                        close_stack.push(&key);
                    }
                }
            }

            // Close all stacked stores
            for close_item in close_stack {
                graph_pool_write.remove(close_item);
            }

            // Void the set of items to be consolidated (this has been processed)
            *graph_consolidate_write = HashSet::new();
        }

        info!(
            "done scanning for fst store pool items to consolidate (move: {}, push: {}, pop: {})",
            count_moved, count_pushed, count_popped
        );
    }

    fn consolidate_item(store: &StoreFSTBox) -> (bool, usize, usize, usize) {
        let (mut should_close, mut count_moved, mut count_pushed, mut count_popped) =
            (false, 0, 0, 0);

        // Acquire write references to pending sets
        let (mut pending_push_write, mut pending_pop_write) = (
            store.pending.push.write().unwrap(),
            store.pending.pop.write().unwrap(),
        );

        // Do consolidate? (any change commited)
        // Notice: if both pending sets are empty do not consolidate as there may have been a \
        //   push then a pop of this push, nulling out any commited change.
        if pending_push_write.len() > 0 || pending_pop_write.len() > 0 {
            // Read old FST (or default to empty FST)
            if let Ok(old_fst) = StoreFSTBuilder::open(&store.target.0, &store.target.1) {
                // Initialize the new FST (temporary)
                let bucket_tmp_path = StoreFSTBuilder::path(
                    StoreFSTPathMode::Temporary,
                    &store.target.0,
                    Some(&store.target.1),
                );

                let bucket_tmp_path_parent = bucket_tmp_path.parent().unwrap();

                if fs::create_dir_all(&bucket_tmp_path_parent).is_ok() == true {
                    // Erase any previously-existing temporary FST (eg. process stopped while \
                    //   writing the temporary FST); there is no guarantee this succeeds.
                    fs::remove_file(&bucket_tmp_path).ok();

                    if let Ok(tmp_fst_file) = File::create(&bucket_tmp_path) {
                        let tmp_fst_writer = BufWriter::new(tmp_fst_file);

                        // Create a builder that can be used to insert new key-value pairs.
                        if let Ok(mut tmp_fst_builder) = FSTSetBuilder::new(tmp_fst_writer) {
                            // Append words not in pop list to new FST (ie. old words minus pop words)
                            let mut old_fst_stream = old_fst.stream();

                            while let Some(old_fst_word) = old_fst_stream.next() {
                                if pending_pop_write.contains(old_fst_word) == false {
                                    tmp_fst_builder.insert(old_fst_word).ok();

                                    count_moved += 1;
                                } else {
                                    count_popped += 1;
                                }
                            }

                            // Append pushed words to new FST (ie. push words)
                            for push_word in &*pending_push_write {
                                tmp_fst_builder.insert(push_word).ok();

                                count_pushed += 1;
                            }

                            // Finish building new FST
                            if tmp_fst_builder.finish().is_ok() == true {
                                // Should close open store reference to old FST
                                should_close = true;

                                // Replace old FST with new FST (this nukes the old FST)
                                // Notice: there is no need to re-open the new FST, as it will be \
                                //   automatically opened on its next access.
                                let bucket_final_path = StoreFSTBuilder::path(
                                    StoreFSTPathMode::Permanent,
                                    &store.target.0,
                                    Some(&store.target.1),
                                );

                                if std::fs::rename(&bucket_tmp_path, &bucket_final_path).is_ok()
                                    == true
                                {
                                    info!("done consolidate fst at path: {:?}", bucket_final_path);
                                } else {
                                    error!(
                                        "error consolidating fst at path: {:?}",
                                        bucket_final_path
                                    );
                                }
                            } else {
                                error!(
                                    "error finishing building temporary fst at path: {:?}",
                                    bucket_tmp_path
                                );
                            }
                        } else {
                            error!(
                                "error starting building temporary fst at path: {:?}",
                                bucket_tmp_path
                            );
                        }
                    } else {
                        error!(
                            "error initializing temporary fst at path: {:?}",
                            bucket_tmp_path
                        );
                    }
                } else {
                    error!(
                        "error initializing temporary fst directory at path: {:?}",
                        bucket_tmp_path_parent
                    );
                }
            }

            // Reset all pending sets
            *pending_push_write = HashSet::new();
            *pending_pop_write = HashSet::new();
        } else {
            error!("error opening old fst");
        }

        (should_close, count_moved, count_pushed, count_popped)
    }
}

impl StoreFSTBuilder {
    pub fn new(collection: &str, bucket: &str) -> Result<StoreFST, FSTError> {
        Self::open(collection, bucket).map(|graph| StoreFST {
            graph: graph,
            target: (collection.to_string(), bucket.to_string()),
            pending: StoreFSTPending::default(),
            last_used: Arc::new(RwLock::new(SystemTime::now())),
        })
    }

    fn open(collection: &str, bucket: &str) -> Result<FSTSet, FSTError> {
        debug!(
            "opening finite-state transducer graph for collection: {} and bucket: {}",
            collection, bucket
        );

        let collection_bucket_path =
            Self::path(StoreFSTPathMode::Permanent, collection, Some(bucket));

        if collection_bucket_path.exists() == true {
            // TODO: IMPORTANT >> It is up to the caller to enforce that the memory map is not \
            //   modified while it is opened. >> we need to ensure proper locking and avoid opening \
            //   this mmap file while a producer is writing to it, otherwise boom, crash.

            // Open graph at path for collection
            // Notice: this is unsafe, as loaded memory is a memory-mapped file, that cannot be \
            //   garanteed not to be muted while we own a read handle to it.
            unsafe { FSTSet::from_path(collection_bucket_path) }
        } else {
            // FST does not exist on disk, generate an empty FST for now; until a consolidation \
            //   task occurs and populates the on-disk-FST.
            let empty_iter: Vec<&str> = vec![];

            FSTSet::from_iter(empty_iter)
        }
    }

    fn path(mode: StoreFSTPathMode, collection: &str, bucket: Option<&str>) -> PathBuf {
        let mut final_path = APP_CONF.store.fst.path.join(collection);

        if let Some(bucket) = bucket {
            final_path = final_path.join(format!("{}{}", bucket, mode.extension()));
        }

        final_path
    }
}

impl StoreFST {
    pub fn contains(&self, sequence: &str) -> bool {
        let sequence_bytes = sequence.as_bytes();

        // 1. Check in 'pop' set (if value is inside, it means it should not exist)
        if self.pending.pop.read().unwrap().contains(sequence_bytes) == true {
            return false;
        }

        // 2. Check in 'push' set (if value is inside, it means it should exist)
        if self.pending.push.read().unwrap().contains(sequence_bytes) == true {
            return true;
        }

        // 3. Check in 'fst' (final consolidated graph)
        self.graph.contains(sequence)
    }

    pub fn should_consolidate(&self) {
        // Check if not already scheduled
        if GRAPH_CONSOLIDATE.write().unwrap().contains(&self.target) == false {
            let mut graph_consolidate_write = GRAPH_CONSOLIDATE.write().unwrap();

            // Schedule target for next consolidation tick (ie. collection + bucket tuple)
            graph_consolidate_write.insert(self.target.clone());

            info!(
                "graph consolidation scheduled on bucket: {} for collection: {}",
                self.target.0, self.target.1
            );
        } else {
            debug!(
                "graph consolidation already scheduled on bucket: {} for collection: {}",
                self.target.0, self.target.1
            );
        }
    }
}

impl StoreFSTActionBuilder {
    pub fn read(store: StoreFSTBox) -> StoreFSTAction {
        let action = Self::build(store);

        debug!("begin action read block");

        // TODO: handle the rwlock things on (collection, bucket) tuple (unpack bucket store \
        //   and return it); read lock; return a lock guard to ensure it auto-unlocks when caller \
        //   goes out of scope.

        debug!("began action read block");

        action
    }

    pub fn write(store: StoreFSTBox) -> StoreFSTAction {
        let action = Self::build(store);

        debug!("begin action write block");

        // TODO: handle the rwlock things on (collection, bucket) tuple (unpack bucket store \
        //   and return it); write lock; return a lock guard to ensure it auto-unlocks when caller \
        //   goes out of scope.

        debug!("began action write block");

        action
    }

    pub fn erase<'a, T: Into<&'a str>>(collection: T, bucket: Option<T>) -> Result<u32, ()> {
        let collection_str = collection.into();

        info!("erase requested on collection: {}", collection_str);

        // Acquire write + access locks, and reference it in context
        // Notice: write lock prevents graph to be acquired from any context; while access lock \
        //   lets the erasure process wait that any thread using the graph is done with work.
        let (_access, _write) = (
            GRAPH_ACCESS_LOCK.write().unwrap(),
            GRAPH_WRITE_LOCK.lock().unwrap(),
        );

        if let Some(bucket) = bucket {
            Self::erase_bucket(collection_str, bucket.into())
        } else {
            Self::erase_collection(collection_str)
        }
    }

    fn erase_collection(collection_str: &str) -> Result<u32, ()> {
        let path_mode = StoreFSTPathMode::Permanent;
        let collection_path = StoreFSTBuilder::path(path_mode, collection_str, None);

        if collection_path.exists() == true {
            debug!(
                "collection store exists, erasing: {}/* at path: {:?}",
                collection_str, &collection_path
            );

            // Scan collection directory for contained bucket files
            let mut buckets: Vec<String> = Vec::new();

            if let Ok(entries) = fs::read_dir(&collection_path) {
                let fst_extension = path_mode.extension();
                let fst_extension_len = fst_extension.len();

                for entry in entries {
                    if let Ok(entry) = entry {
                        if let Some(entry_name) = entry.file_name().to_str() {
                            let entry_name_len = entry_name.len();

                            // FST file found? This is a bucket.
                            if entry_name_len > fst_extension_len
                                && entry_name.ends_with(fst_extension) == true
                            {
                                buckets.push(String::from(
                                    &entry_name[..(entry_name_len - fst_extension_len)],
                                ));
                            }
                        }
                    }
                }
            } else {
                warn!("failed reading directory: {:?}", collection_path);
            }

            // Force a FST graph close (on all contained buckets)
            {
                let mut graph_pool_write = GRAPH_POOL.write().unwrap();

                // TODO: this is ugly, do we really need to create a heap string on each iter?!
                for bucket in buckets {
                    debug!("forcibly closing graph bucket: {}", bucket);

                    graph_pool_write.remove(&(collection_str.to_string(), bucket));
                }
            }

            // Remove FST graph storage from filesystem
            let erase_result = fs::remove_dir_all(&collection_path);

            if erase_result.is_ok() == true {
                debug!("done with collection erasure");

                Ok(1)
            } else {
                Err(())
            }
        } else {
            debug!(
                "collection store does not exist, consider already erased: {}/* at path: {:?}",
                collection_str, &collection_path
            );

            Ok(0)
        }
    }

    fn erase_bucket(collection_str: &str, bucket_str: &str) -> Result<u32, ()> {
        debug!(
            "sub-erase on bucket: {} for collection: {}",
            bucket_str, collection_str
        );

        let bucket_path = StoreFSTBuilder::path(
            StoreFSTPathMode::Permanent,
            collection_str,
            Some(bucket_str),
        );

        if bucket_path.exists() == true {
            debug!(
                "bucket graph exists, erasing: {}/{} at path: {:?}",
                collection_str, bucket_str, &bucket_path
            );

            // Force a FST graph close
            {
                GRAPH_POOL
                    .write()
                    .unwrap()
                    .remove(&(collection_str.to_string(), bucket_str.to_string()));
            }

            // Remove FST graph storage from filesystem
            let erase_result = fs::remove_file(&bucket_path);

            if erase_result.is_ok() == true {
                debug!("done with bucket erasure");

                Ok(1)
            } else {
                Err(())
            }
        } else {
            debug!(
                "bucket graph does not exist, consider already erased: {}/{} at path: {:?}",
                collection_str, bucket_str, &bucket_path
            );

            Ok(0)
        }
    }

    fn build(store: StoreFSTBox) -> StoreFSTAction {
        StoreFSTAction { store: store }
    }
}

impl StoreFSTAction {
    // TODO: == Add FST to the following: ==
    // TODO: CHANNEL FLUSHO -> pop_word for each word
    // TODO: CHANNEL SEARCH -> complete not-found words via FST +/ levenshtein distance complete

    pub fn push_word(&self, word: &str) -> bool {
        let word_bytes = word.as_bytes();

        // Nuke word from 'pop' set? (void a previous un-consolidated commit)
        if self.store.pending.pop.read().unwrap().contains(word_bytes) == true {
            self.store.pending.pop.write().unwrap().remove(word_bytes);
        }

        // Add word in 'push' set? (only if word is not in FST)
        if self.store.graph.contains(&word) == false
            && self.store.pending.push.read().unwrap().contains(word_bytes) == false
        {
            self.store
                .pending
                .push
                .write()
                .unwrap()
                .insert(word_bytes.to_vec());

            self.store.should_consolidate();

            // Pushed
            true
        } else {
            // Not pushed
            false
        }
    }

    pub fn pop_word(&self, word: &str) -> bool {
        let word_bytes = word.as_bytes();

        // Nuke word from 'push' set? (void a previous un-consolidated commit)
        if self.store.pending.push.read().unwrap().contains(word_bytes) == true {
            self.store.pending.push.write().unwrap().remove(word_bytes);
        }

        // Add word in 'pop' set? (only if word is in FST)
        if self.store.graph.contains(word_bytes) == true
            && self.store.pending.pop.read().unwrap().contains(word_bytes) == false
        {
            self.store
                .pending
                .pop
                .write()
                .unwrap()
                .insert(word_bytes.to_vec());

            self.store.should_consolidate();

            // Popped
            true
        } else {
            // Not popped
            false
        }
    }

    pub fn has_word(&self, word: &str) -> bool {
        // Notice: this method checks if word exists either in un-commited or commited stores.
        self.store.contains(word)
    }

    pub fn suggest_words(&self, _from_word: &str, _limit: u16) -> Option<Vec<String>> {
        // TODO: search 'limit' words in FST (iteratively, until limit is reached)
        // TODO: check if 'self.has_word(word)' to avoid removed words (refine when limit \
        //   isnt reached)

        None
    }
}
