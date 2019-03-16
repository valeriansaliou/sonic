// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use fst::set::Stream as FSTStream;
use fst::{
    Automaton, Error as FSTError, IntoStreamer, Set as FSTSet, SetBuilder as FSTSetBuilder,
    Streamer,
};
use fst_levenshtein::Levenshtein;
use fst_regex::Regex;
use hashbrown::{HashMap, HashSet};
use regex_syntax::escape as regex_escape;
use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::BufWriter;
use std::iter::FromIterator;
use std::mem;
use std::path::PathBuf;
use std::str;
use std::sync::{Arc, Mutex, RwLock};
use std::time::SystemTime;

use crate::APP_CONF;
use super::keyer::StoreKeyerHasher;

pub struct StoreFSTPool;
pub struct StoreFSTBuilder;

pub struct StoreFST {
    graph: FSTSet,
    target: StoreFSTKey,
    pending: StoreFSTPending,
    last_used: Arc<RwLock<SystemTime>>,
    last_consolidated: Arc<RwLock<SystemTime>>,
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

pub struct StoreFSTMisc;

#[derive(Copy, Clone)]
enum StoreFSTPathMode {
    Permanent,
    Temporary,
}

type StoreFSTAtom = u32;
type StoreFSTBox = Arc<StoreFST>;
type StoreFSTKey = (StoreFSTAtom, StoreFSTAtom);

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
    pub fn acquire<'a, T: Into<&'a str>>(
        collection: T,
        bucket: T,
    ) -> Result<StoreFSTBox, FSTError> {
        let (collection_str, bucket_str) = (collection.into(), bucket.into());

        let pool_key = (
            StoreKeyerHasher::to_compact(collection_str),
            StoreKeyerHasher::to_compact(bucket_str),
        );

        // Acquire general lock, and reference it in context
        // Notice: this prevents graph to be opened while also erased; or 2 graphs on the \
        //   same collection to be opened at the same time.
        let _write = GRAPH_WRITE_LOCK.lock().unwrap();

        // Acquire a thread-safe store pool reference in read mode
        let graph_pool_read = GRAPH_POOL.read().unwrap();

        if let Some(store_fst) = graph_pool_read.get(&pool_key) {
            debug!(
                "fst store acquired from pool for collection: {} <{:x?}> / bucket: {} <{:x?}>",
                collection_str, pool_key.0, bucket_str, pool_key.1
            );

            // Bump store last used date (avoids early janitor eviction)
            let mut last_used_value = store_fst.last_used.write().unwrap();

            mem::replace(&mut *last_used_value, SystemTime::now());

            // Perform an early drop of the lock (frees up write lock early)
            drop(last_used_value);

            Ok(store_fst.clone())
        } else {
            info!(
                "fst store not in pool for collection: {} <{:x?}> / bucket: {} <{:x?}>, opening it",
                collection_str, pool_key.0, bucket_str, pool_key.1
            );

            match StoreFSTBuilder::new(pool_key.0, pool_key.1) {
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
                    "found expired fst store pool item: <{:x?}>/<{:x?}>; last used time: {:?}",
                    &collection_bucket.0, &collection_bucket.1, last_used
                );

                // Notice: the bucket value needs to be cloned, as we cannot reference as value \
                //   that will outlive referenced value once we remove it from its owner set.
                removal_register.push(*collection_bucket);
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

    pub fn consolidate(force: bool) {
        debug!("scanning for fst store pool items to consolidate");

        // Notice: we do not consolidate all items at each tick, we try to even out multiple \
        //   consolidation tasks over time. This lowers the overall HZ of the tasker system for \
        //   certain heavy tasks, which is better to spread out consolidation steps over time over \
        //   a large number of very active buckets.

        // Acquire write + access locks, and reference it in context
        // Notice: write lock prevents graph to be acquired from any context; while access lock \
        //   lets the consolidate process wait that any thread using the graph is done with work.
        let (_access, _write) = (
            GRAPH_ACCESS_LOCK.write().unwrap(),
            GRAPH_WRITE_LOCK.lock().unwrap(),
        );

        let (mut count_moved, mut count_pushed, mut count_popped) = (0, 0, 0);

        if GRAPH_CONSOLIDATE.read().unwrap().len() > 0 {
            let mut graph_pool_write = GRAPH_POOL.write().unwrap();

            // Prepare close stack (used once whole set is scanned)
            let mut close_stack: Vec<StoreFSTKey> = Vec::new();

            // Proceed FST consolidation for each store key
            {
                let graph_consolidate_read = GRAPH_CONSOLIDATE.read().unwrap();

                for key in &*graph_consolidate_read {
                    if let Some(store) = graph_pool_write.get(&key) {
                        let not_consolidated_for = store
                            .last_consolidated
                            .read()
                            .unwrap()
                            .elapsed()
                            .unwrap()
                            .as_secs();

                        // Should we consolidate right now?
                        if force == true
                            || not_consolidated_for >= APP_CONF.store.fst.graph.consolidate_after
                        {
                            info!(
                                "fst key: {:?} not consolidated for: {} seconds, may consolidate",
                                key, not_consolidated_for
                            );

                            let consolidate_counts = Self::consolidate_item(store);

                            count_moved += consolidate_counts.1;
                            count_pushed += consolidate_counts.2;
                            count_popped += consolidate_counts.3;

                            // Stack cached store close request?
                            if consolidate_counts.0 == true {
                                // Notice: unfortunately we need to clone the key value there, as \
                                //   we cannot reference the value and then use it to unassign \
                                //   the reference from its set w/o breaking the borrow checker \
                                //   rules. ie. there is just no other way around.
                                close_stack.push(*key);
                            }
                        } else {
                            debug!(
                                "fst key: {:?} not consolidated for: {} seconds, no consolidate",
                                key, not_consolidated_for
                            );
                        }
                    }
                }
            }

            // Close all stacked stores
            {
                let mut graph_consolidate_write = GRAPH_CONSOLIDATE.write().unwrap();

                for close_item in close_stack {
                    // Nuke old opened FST
                    // Notice: last consolidated date will be bumped to a new date in the future \
                    //   when a push or pop operation will be done, thus effectively scheduling a \
                    //   consolidation in the future properly.
                    graph_pool_write.remove(&close_item);

                    // Void the set of items to be consolidated (this has been processed)
                    graph_consolidate_write.remove(&close_item);
                }
            }
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
            if let Ok(old_fst) = StoreFSTBuilder::open(store.target.0, store.target.1) {
                // Initialize the new FST (temporary)
                let bucket_tmp_path = StoreFSTBuilder::path(
                    StoreFSTPathMode::Temporary,
                    store.target.0,
                    Some(store.target.1),
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
                            // Convert push keys to an ordered vector
                            // Notice: we must go from a Vec to a VecDeque as to sort values, \
                            //   which is a requirement for FST insertions.
                            let mut ordered_push_vec: Vec<&[u8]> =
                                Vec::from_iter(pending_push_write.iter().map(|item| item.as_ref()));

                            ordered_push_vec.sort();

                            let mut ordered_push: VecDeque<&[u8]> =
                                VecDeque::from_iter(ordered_push_vec);

                            // Append words not in pop list to new FST (ie. old words minus pop words)
                            let mut old_fst_stream = old_fst.stream();

                            while let Some(old_fst_word) = old_fst_stream.next() {
                                // Append new words from front? (ie. push words)
                                // Notice: as an FST is ordered, inserts would fail if they are \
                                //   commited out-of-order. Thus, the only way to check for \
                                //   order is there.
                                while let Some(push_front_ref) = ordered_push.front() {
                                    if *push_front_ref <= old_fst_word {
                                        // Pop front item and consume it
                                        if let Some(push_front) = ordered_push.pop_front() {
                                            if let Err(err) = tmp_fst_builder.insert(push_front) {
                                                error!(
                                                    "failed inserting new word from old in fst: {}",
                                                    err
                                                );
                                            }

                                            count_pushed += 1;

                                            // Continue scanning next word (may also come before \
                                            //   this FST word in order)
                                            continue;
                                        }
                                    }

                                    // Important: stop loop on next front item (always the same)
                                    break;
                                }

                                // Restore old word (if not popped)
                                if pending_pop_write.contains(old_fst_word) == false {
                                    if let Err(err) = tmp_fst_builder.insert(old_fst_word) {
                                        error!("failed inserting old word in fst: {}", err);
                                    }

                                    count_moved += 1;
                                } else {
                                    count_popped += 1;
                                }
                            }

                            // Complete FST with last pushed items
                            // Notice: this is necessary if the FST was empty, or if we have push \
                            //   items that come after the last ordered word of the FST.
                            while let Some(push_front) = ordered_push.pop_front() {
                                if let Err(err) = tmp_fst_builder.insert(push_front) {
                                    error!(
                                        "failed inserting new word from complete in fst: {}",
                                        err
                                    );
                                }

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
                                    store.target.0,
                                    Some(store.target.1),
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
    pub fn new(collection_hash: StoreFSTAtom, bucket_hash: StoreFSTAtom) -> Result<StoreFST, FSTError> {
        Self::open(collection_hash, bucket_hash).map(|graph| {
            let now = SystemTime::now();

            StoreFST {
                graph: graph,
                target: (collection_hash, bucket_hash),
                pending: StoreFSTPending::default(),
                last_used: Arc::new(RwLock::new(now)),
                last_consolidated: Arc::new(RwLock::new(now)),
            }
        })
    }

    fn open(collection_hash: StoreFSTAtom, bucket_hash: StoreFSTAtom) -> Result<FSTSet, FSTError> {
        debug!(
            "opening finite-state transducer graph for collection: <{:x?}> and bucket: <{:x?}>",
            collection_hash, bucket_hash
        );

        let collection_bucket_path =
            Self::path(StoreFSTPathMode::Permanent, collection_hash, Some(bucket_hash));

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

    fn path(mode: StoreFSTPathMode, collection_hash: StoreFSTAtom, bucket_hash: Option<StoreFSTAtom>) -> PathBuf {
        let mut final_path = APP_CONF.store.fst.path.join(format!("{:x?}", collection_hash));

        if let Some(bucket_hash) = bucket_hash {
            final_path = final_path.join(format!("{:x?}{}", bucket_hash, mode.extension()));
        }

        final_path
    }
}

impl StoreFST {
    pub fn cardinality(&self) -> usize {
        self.graph.len()
    }

    pub fn lookup_begins(&self, word: &str) -> Result<FSTStream<Regex>, ()> {
        let regex_str = format!("{}.*", regex_escape(word));

        debug!(
            "looking-up word in fst via 'begins': {} with regex: {}",
            word, regex_str
        );

        if let Ok(regex) = Regex::new(&regex_str) {
            Ok(self.graph.search(regex).into_stream())
        } else {
            Err(())
        }
    }

    pub fn lookup_typos(&self, word: &str) -> Result<FSTStream<Levenshtein>, ()> {
        // Allow more typos in word as the word gets longer, up to a maximum limit
        let typo_factor = match word.len() {
            1 | 2 | 3 => 0,
            4 | 5 | 6 => 1,
            7 | 8 | 9 => 2,
            _ => 3,
        };

        debug!(
            "looking-up word in fst via 'typos': {} with typo factor: {}",
            word, typo_factor
        );

        if let Ok(fuzzy) = Levenshtein::new(word, typo_factor) {
            Ok(self.graph.search(fuzzy).into_stream())
        } else {
            Err(())
        }
    }

    pub fn should_consolidate(&self) {
        // Check if not already scheduled
        if GRAPH_CONSOLIDATE.write().unwrap().contains(&self.target) == false {
            let mut graph_consolidate_write = GRAPH_CONSOLIDATE.write().unwrap();

            // Schedule target for next consolidation tick (ie. collection + bucket tuple)
            graph_consolidate_write.insert(self.target);

            // Bump 'last consolidated' time, effectively de-bouncing consolidation to a fixed \
            //   and predictible tick time in the future.
            let mut last_consolidated_value = self.last_consolidated.write().unwrap();

            mem::replace(&mut *last_consolidated_value, SystemTime::now());

            info!(
                "graph consolidation scheduled on bucket: <{:x?}> for collection: <{:x?}>",
                self.target.0, self.target.1
            );
        } else {
            debug!(
                "graph consolidation already scheduled on bucket: <{:x?}> for collection: <{:x?}>",
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

        info!("fst erase requested on collection: {}", collection_str);

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
        let collection_atom = StoreKeyerHasher::to_compact(collection_str);
        let collection_path = StoreFSTBuilder::path(path_mode, collection_atom, None);

        // Force a FST graph close (on all contained buckets)
        // Notice: we first need to scan for opened buckets in-memory, as not all FSTs may be \
        //   commited to disk; thus some FST stores that exist in-memory may not exist on-disk.
        let mut bucket_atoms: Vec<StoreFSTAtom> = Vec::new();

        {
            let graph_pool_read = GRAPH_POOL.read().unwrap();

            for target_key in graph_pool_read.keys() {
                if target_key.0 == collection_atom {
                    bucket_atoms.push(target_key.1);
                }
            }
        }

        if bucket_atoms.is_empty() == false {
            debug!(
                "will force-close {} fst buckets for collection: {}",
                bucket_atoms.len(), collection_str
            );

            let (mut graph_pool_write, mut graph_consolidate_write) = (
                GRAPH_POOL.write().unwrap(),
                GRAPH_CONSOLIDATE.write().unwrap(),
            );

            for bucket_atom in bucket_atoms {
                debug!(
                    "fst bucket graph force close for bucket: {}/<{:x?}>",
                    collection_str, bucket_atom
                );

                let bucket_target = (collection_atom, bucket_atom);

                graph_pool_write.remove(&bucket_target);
                graph_consolidate_write.remove(&bucket_target);
            }
        }

        // Remove all FSTs on-disk
        if collection_path.exists() == true {
            debug!(
                "fst collection store exists, erasing: {}/* at path: {:?}",
                collection_str, &collection_path
            );

            // Remove FST graph storage from filesystem
            let erase_result = fs::remove_dir_all(&collection_path);

            if erase_result.is_ok() == true {
                debug!("done with fst collection erasure");

                Ok(1)
            } else {
                Err(())
            }
        } else {
            debug!(
                "fst collection store does not exist, consider already erased: {}/* at path: {:?}",
                collection_str, &collection_path
            );

            Ok(0)
        }
    }

    fn erase_bucket(collection_str: &str, bucket_str: &str) -> Result<u32, ()> {
        debug!(
            "sub-erase on fst bucket: {} for collection: {}",
            bucket_str, collection_str
        );

        let (collection_atom, bucket_atom) = (
            StoreKeyerHasher::to_compact(collection_str),
            StoreKeyerHasher::to_compact(bucket_str),
        );

        let bucket_path = StoreFSTBuilder::path(
            StoreFSTPathMode::Permanent,
            collection_atom,
            Some(bucket_atom),
        );

        // Force a FST graph close
        {
            debug!(
                "fst bucket graph force close for bucket: {}/{}",
                collection_str, bucket_str
            );

            let bucket_target = (collection_atom, bucket_atom);

            GRAPH_POOL.write().unwrap().remove(&bucket_target);
            GRAPH_CONSOLIDATE.write().unwrap().remove(&bucket_target);
        }

        // Remove FST on-disk
        if bucket_path.exists() == true {
            debug!(
                "fst bucket graph exists, erasing: {}/{} at path: {:?}",
                collection_str, bucket_str, &bucket_path
            );

            // Remove FST graph storage from filesystem
            let erase_result = fs::remove_file(&bucket_path);

            if erase_result.is_ok() == true {
                debug!("done with fst bucket erasure");

                Ok(1)
            } else {
                Err(())
            }
        } else {
            debug!(
                "fst bucket graph does not exist, consider already erased: {}/{} at path: {:?}",
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

    pub fn suggest_words(&self, from_word: &str, limit: usize) -> Option<Vec<String>> {
        let mut found_words = Vec::new();

        // Try to complete provided word
        if let Ok(stream) = self.store.lookup_begins(from_word) {
            debug!("looking up for word: {} in 'begins' fst stream", from_word);

            Self::find_words_stream(stream, &mut found_words, limit);
        }

        // Try to fuzzy-suggest other words? (eg. correct typos)
        if found_words.len() < limit {
            if let Ok(stream) = self.store.lookup_typos(from_word) {
                debug!("looking up for word: {} in 'typos' fst stream", from_word);

                Self::find_words_stream(stream, &mut found_words, limit);
            }
        }

        if found_words.is_empty() == false {
            Some(found_words)
        } else {
            None
        }
    }

    pub fn count_words(&self) -> usize {
        self.store.cardinality()
    }

    fn find_words_stream<A: Automaton>(
        mut stream: FSTStream<A>,
        found_words: &mut Vec<String>,
        limit: usize,
    ) {
        while let Some(word) = stream.next() {
            if let Ok(word_str) = str::from_utf8(word) {
                let word_string = word_str.to_string();

                if found_words.contains(&word_string) == false {
                    found_words.push(word_string);

                    // Requested limit reached? Stop there.
                    if found_words.len() >= limit {
                        break;
                    }
                }
            }
        }
    }
}

impl StoreFSTMisc {
    pub fn count_collection_buckets<'a, T: Into<&'a str>>(collection: T) -> Result<usize, ()> {
        let mut count = 0;

        let path_mode = StoreFSTPathMode::Permanent;

        let collection_atom = StoreKeyerHasher::to_compact(collection.into());
        let collection_path = StoreFSTBuilder::path(path_mode, collection_atom, None);

        if collection_path.exists() == true {
            // Scan collection directory for contained buckets (count them)
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
                                count += 1;
                            }
                        }
                    }
                }
            } else {
                warn!("failed reading directory for count: {:?}", collection_path);

                return Err(());
            }
        }

        Ok(count)
    }
}
