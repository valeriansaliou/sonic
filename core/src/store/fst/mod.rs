// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod backup;
mod pool;
mod util;

use std::fmt;
use std::sync::atomic::{self, AtomicBool};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::Instant;

use fst::{IntoStreamer as _, Streamer as _};
use hashbrown::HashMap;
use regex_syntax::escape as regex_escape;

use crate::config::FstStoreConfig;
use crate::lexer::ranges::LexerRegexRange;

use super::generic::*;

pub use self::pool::{FstStoreId, FstStorePool};
use self::util::*;

pub struct FstStore {
    graph: fst::Set,
    config: Arc<FstStoreConfig>,
    target: FstStoreId,
    pending: Mutex<HashMap<Box<[u8]>, PendingAction>>,
    last_used: Arc<RwLock<Instant>>,
    last_consolidated: Arc<RwLock<Instant>>,
    should_consolidate: AtomicBool,
    // NOTE: This shouldn’t be here, but until a big rewrite let’s not care.
    action_config: FstRepositoryConfig,
}

#[derive(Debug, PartialEq)]
enum PendingAction {
    Push,
    Pop,
}

#[derive(Copy, Clone)]
enum FstStorePathMode {
    Permanent,
    Temporary,
    Backup,
}

impl FstStorePathMode {
    fn extension(&self) -> &'static str {
        match self {
            FstStorePathMode::Permanent => ".fst",
            FstStorePathMode::Temporary => ".fst.tmp",
            FstStorePathMode::Backup => ".fst.bck",
        }
    }
}

#[derive(Debug)]
pub struct FstRepository<'a> {
    store: &'a FstStore,
    config: FstRepositoryConfig,
}

#[derive(Debug, Clone, Copy)]
pub struct FstRepositoryConfig {
    pub prefix_matching_enabled: bool,
    pub fuzzy_matching_enabled: bool,
}

impl Default for FstRepositoryConfig {
    fn default() -> Self {
        Self {
            prefix_matching_enabled: true,
            fuzzy_matching_enabled: true,
        }
    }
}

const WORD_LIMIT_LENGTH: usize = 40;

impl FstStore {
    pub(super) fn open(id: &FstStoreId, config: &FstStoreConfig) -> Result<fst::Set, fst::Error> {
        tracing::debug!("Opening fst graph for {id}");

        let collection_bucket_path = config.store_path(id, FstStorePathMode::Permanent);

        if collection_bucket_path.exists() {
            // Open graph at path for collection
            // SAFETY: This is unsafe, as loaded memory is a memory-mapped file, that cannot be
            //   guaranteed not to be muted while we own a read handle to it. Though, we use
            //   higher-level locking mechanisms on all callers of this method, so we are safe.
            unsafe { fst::Set::from_path(collection_bucket_path) }
        } else {
            // FST does not exist on disk; generate an empty FST for now
            // (until a consolidation task occurs and populates the on-disk FST).
            fst::Set::from_iter(std::iter::empty::<&str>())
        }
    }

    fn cardinality(&self) -> usize {
        self.graph.len()
    }

    fn as_stream(&self) -> fst::set::Stream<'_> {
        self.graph.into_stream()
    }

    fn lookup_begins(&self, word: &str) -> Result<fst::set::Stream<'_, fst_regex::Regex>, ()> {
        // NOTE: This regex maps over an unicode range, for speed reasons at scale.
        //   We found out that the 'match any' syntax ('.*') was super-slow. Using the restrictive
        //   syntax below divided the cost of e.g. a search query by 2. The regex below has been
        //   found out to be nearly zero-cost to compile and execute, for whatever reason.
        // Regex format: '{escaped_word}([{unicode_range}]*)'
        let mut regex_str = regex_escape(word);

        regex_str.push('(');

        LexerRegexRange::from(word)
            .unwrap_or_default()
            .write_to(&mut regex_str)
            // Regex write failed? (this should not happen)
            .map_err(|error| tracing::error!(
                "Could not lookup word in fst via 'begins': {word:?} because regex write failed: {error:?}"
            ))?;

        regex_str.push_str("*)");

        // Proceed word lookup.
        tracing::debug!("Looking-up word in fst via 'begins': {word:?} with regex: {regex_str:?}");

        let regex = fst_regex::Regex::new(&regex_str).map_err(|_error| ())?;

        Ok(self.graph.search(regex).into_stream())
    }

    fn lookup_typos(
        &self,
        word: &str,
        typo_factor: u32,
    ) -> Result<fst::set::Stream<'_, fst_levenshtein::Levenshtein>, ()> {
        tracing::debug!(
            "Looking-up word in fst via 'typos': {word:?} with typo factor: {typo_factor:?}"
        );

        let fuzzy = fst_levenshtein::Levenshtein::new(word, typo_factor).map_err(|_error| ())?;

        Ok(self.graph.search(fuzzy).into_stream())
    }

    fn should_consolidate(&self) {
        let already_scheduled = (self.should_consolidate).swap(true, atomic::Ordering::AcqRel);

        // Bump “last consolidated” time, effectively de-bouncing consolidation
        // to a fixed and predictable tick time in the future.
        *self.last_consolidated.write().unwrap() = Instant::now();

        if already_scheduled {
            tracing::trace!("Graph consolidation re-scheduled for {}", self.target);
        } else {
            tracing::debug!("Graph consolidation scheduled for {}", self.target);
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct ConsolidateStats {
    pub(super) count_moved: usize,
    pub(super) count_pushed: usize,
    pub(super) count_popped: usize,
}

impl FstStore {
    pub(super) fn consolidate(&self, stats: &mut ConsolidateStats) -> Result<bool, ()> {
        use std::collections::{HashSet, VecDeque};
        use std::fs::{self, File};
        use std::io;

        // Acquire write reference to pending actions.
        let mut pending = self.pending.lock().unwrap();

        // Do consolidate? (any change committed)
        // NOTE: If pending set is empty do not consolidate as there may have been
        //   a push then a pop of this push, nulling out any committed change.
        if pending.is_empty() {
            (self.should_consolidate).store(false, atomic::Ordering::Release);
            return Ok(false);
        }

        // Read old FST (or default to empty FST).
        let old_fst = FstStore::open(&self.target, &self.config)
            .map_err(|error| tracing::error!("Error opening old fst: {error:?}"))?;

        // Initialize the new FST (temporary).
        let bucket_tmp_path = (self.config).store_path(&self.target, FstStorePathMode::Temporary);

        let bucket_tmp_path_parent = bucket_tmp_path.parent().unwrap();

        fs::create_dir_all(&bucket_tmp_path_parent).map_err(|error| tracing::error!(
            "Error initializing temporary fst directory at path {bucket_tmp_path_parent:?}: {error:?}"
        ))?;

        // Erase any previously-existing temporary FST (e.g. process stopped while
        // writing the temporary FST); there is no guarantee this succeeds.
        fs::remove_file(&bucket_tmp_path).ok();

        let tmp_fst_file = File::create(&bucket_tmp_path).map_err(|error| {
            tracing::error!(
                "Error initializing temporary fst at path {bucket_tmp_path:?}: {error:?}"
            )
        })?;

        let tmp_fst_writer = io::BufWriter::new(tmp_fst_file);

        // Create a builder that can be used to insert new key-value pairs.
        let mut tmp_fst_builder = fst::SetBuilder::new(tmp_fst_writer).map_err(|error| {
            tracing::error!(
                "Error starting building temporary fst at path {bucket_tmp_path:?}: {error:?}"
            )
        })?;

        // Convert push keys to an ordered vector.
        // NOTE: We must go from a `Vec` to a `VecDeque` to sort values,
        //   which is a requirement for FST insertions.
        let mut ordered_push_vec: Vec<&[u8]> = Vec::with_capacity(pending.len());
        let mut to_pop: HashSet<&[u8]> = HashSet::with_capacity(pending.len());

        for (item, action) in pending.iter() {
            match action {
                PendingAction::Push => {
                    ordered_push_vec.push(item.as_ref());
                }
                PendingAction::Pop => {
                    to_pop.insert(item.as_ref());
                }
            }
        }

        ordered_push_vec.sort();

        let mut ordered_push: VecDeque<&[u8]> = VecDeque::from_iter(ordered_push_vec);

        // Append words not in pop list to new FST (i.e. old words minus pop words).
        let mut old_fst_stream = old_fst.stream();

        'old: while let Some(old_fst_word) = old_fst_stream.next() {
            // Append new words from front? (i.e. push words)
            // NOTE: As an FST is ordered, inserts would fail if they are
            //   committed out-of-order. Thus, the only way to check for
            //   order is there.
            // NOTE: A quick check is done before engaging in the loop, to
            //   prevent any de-optimized jump instruction, as we may call
            //   this code block a lot on large FSTs, and the loop should not
            //   be engaged that often on stabilized FSTs (i.e. mature FSTs).
            while let Some(push_front) = ordered_push.pop_front_if(|&mut push_ref| {
                // Important: stop loop on next front item (always the same).
                push_ref <= old_fst_word
            }) {
                if check_over_limits(
                    tmp_fst_builder.bytes_written() as usize,
                    stats.count_pushed + stats.count_moved,
                    &self.config.graph,
                ) {
                    // FST cannot accept more items (limits reached).
                    tracing::warn!("Limit reached on new from old in fst");

                    // Important: stop the main loop (limit reached).
                    break 'old;
                }

                match tmp_fst_builder.insert(push_front) {
                    // Word inserted in FST.
                    Ok(()) => stats.count_pushed += 1,
                    // Could not insert word in FST.
                    Err(error) => {
                        tracing::error!("Failed inserting new from old in fst: {error:?}")
                    }
                }

                // Continue scanning next word (may also come
                // before this FST word in order).
                continue;
            }

            // Restore old word (if not popped).
            if to_pop.contains(old_fst_word) {
                stats.count_popped += 1;
            } else {
                if check_over_limits(
                    tmp_fst_builder.bytes_written() as usize,
                    stats.count_pushed + stats.count_moved,
                    &self.config.graph,
                ) {
                    // FST cannot accept more items (limits reached).
                    tracing::warn!("Limit reached on old word in fst");

                    // Important: stop the main loop (limit reached).
                    break 'old;
                }

                match tmp_fst_builder.insert(old_fst_word) {
                    // Word moved to FST.
                    Ok(()) => stats.count_moved += 1,
                    // Could not move word to FST.
                    Err(error) => tracing::error!("Failed inserting old word in fst: {error:?}"),
                }
            }
        }

        // Complete FST with last pushed items.
        // NOTE: This is necessary if the FST was empty, or if we have push
        //   items that come after the last ordered word of the FST.
        while let Some(push_front) = ordered_push.pop_front() {
            if check_over_limits(
                tmp_fst_builder.bytes_written() as usize,
                stats.count_pushed + stats.count_moved,
                &self.config.graph,
            ) {
                // FST cannot accept more items (limits reached).
                tracing::warn!("Limit reached on new word from complete in fst");

                // Important: stop the main loop (limit reached).
                break;
            }

            match tmp_fst_builder.insert(push_front) {
                // Word inserted in FST.
                Ok(()) => stats.count_pushed += 1,
                // Could not insert word in FST.
                Err(error) => {
                    tracing::error!("Failed inserting new word from complete in fst: {error:?}")
                }
            }
        }

        // Finish building new FST.
        let should_close = match tmp_fst_builder.finish() {
            Ok(()) => {
                // Replace old FST with new FST (this nukes the old FST).
                // NOTE: There is no need to re-open the new FST, as it will be
                //   automatically opened on its next access.
                let bucket_final_path =
                    (self.config).store_path(&self.target, FstStorePathMode::Permanent);

                // Proceed temporary FST to final FST path rename?
                match fs::rename(&bucket_tmp_path, &bucket_final_path) {
                    Ok(()) => {
                        tracing::debug!("Done consolidating fst at path {bucket_final_path:?}")
                    }
                    Err(error) => tracing::error!(
                        "Error consolidating fst at path {bucket_final_path:?}: {error:?}"
                    ),
                }

                // Should close open store reference to old FST.
                true
            }
            Err(error) => {
                tracing::error!(
                    "Error finishing building temporary fst at path {bucket_tmp_path:?}: {error:?}"
                );

                false
            }
        };

        drop(to_pop);

        // Clear pending actions.
        // TODO(perf): Truncate capacity if too big? Otherwise memory usage
        //   won’t go down after a batch ingestion. Need to find a sensible
        //   value though.
        pending.clear();

        (self.should_consolidate).store(false, atomic::Ordering::Release);

        Ok(should_close)
    }
}

impl GenericStore for FstStore {
    fn kind() -> &'static str {
        "FST"
    }

    fn ref_last_used(&self) -> &RwLock<Instant> {
        &self.last_used
    }
}

impl FstStore {
    pub fn to_repository<'a>(&'a self) -> FstRepository<'a> {
        FstRepository {
            store: self,
            config: self.action_config,
        }
    }
}

impl<'a> FstRepository<'a> {
    fn push_word(
        &self,
        word: &str,
        pending: &mut MutexGuard<'_, HashMap<Box<[u8]>, PendingAction>>,
    ) -> bool {
        // Word over limit? (abort, the FST does not perform well over large words)
        if Self::word_over_limit(word) {
            return false;
        }

        // PERF: Evaluate some checks lazily. Benchmarks show a +6.5% throughput.
        let pending_len = pending.len();
        let should_insert = || {
            let graph_contains_word = self.store.graph.contains(&word);

            // NOTE: Also check whether FST is over limits or not from there, to avoid
            //   stacking words that could never be consolidated to final FST anyway.
            let is_fst_over_limits = {
                let graph_fst = self.store.graph.as_fst();
                check_over_limits(graph_fst.size(), graph_fst.len(), &self.store.config.graph)
            };

            // PERF: To be correct we’d have to filter to keep only “push” actions,
            //   but in a real-world situation we’d trigger this condition only
            //   during a batch ingestion; when all actions are “push”.
            let has_too_many_pending =
                (self.store.config.graph.max_words).is_some_and(|max| pending_len >= max);

            !graph_contains_word && !is_fst_over_limits && !has_too_many_pending
        };

        let word_bytes = word.as_bytes();

        match pending.get_mut(word_bytes) {
            Some(PendingAction::Push) => return false,
            // Remove scheduled “pop”? (void a previous un-consolidated commit)
            Some(pending_action @ PendingAction::Pop) => {
                if should_insert() {
                    *pending_action = PendingAction::Push;
                } else {
                    pending.remove(word_bytes);
                    return false;
                }
            }
            None => {
                if should_insert() {
                    pending.insert(Box::from(word_bytes), PendingAction::Push);
                } else {
                    return false;
                }
            }
        }

        self.store.should_consolidate();

        true
    }

    pub fn push_words<'t>(&self, terms: impl Iterator<Item = &'t str>) {
        let mut pending = self.store.pending.lock().unwrap();

        for term in terms {
            if self.push_word(term, &mut pending) {
                tracing::trace!("push term committed to graph: {term}");
            }
        }
    }

    pub fn pop_word(&self, word: &str) -> bool {
        // Word over limit? (abort, the FST does not perform well over large words)
        if Self::word_over_limit(word) {
            return false;
        }

        // PERF: Perform some checks before acquiring any lock, so it stays
        //   in scope as briefly as possible.
        let graph_contains_word = self.store.graph.contains(&word);

        let should_insert = graph_contains_word;

        let word_bytes = word.as_bytes();

        let mut pending = self.store.pending.lock().unwrap();

        match pending.get_mut(word_bytes) {
            Some(PendingAction::Pop) => return false,
            // Remove scheduled “push”? (void a previous un-consolidated commit)
            Some(pending_action @ PendingAction::Push) => {
                if should_insert {
                    *pending_action = PendingAction::Pop;
                } else {
                    // drop(pending_action);
                    pending.remove(word_bytes);
                    return false;
                }
            }
            None => {
                if should_insert {
                    pending.insert(Box::from(word_bytes), PendingAction::Pop);
                } else {
                    return false;
                }
            }
        }

        drop(pending);

        self.store.should_consolidate();

        true
    }

    pub fn suggest_words(
        &self,
        from_word: &str,
        // Length before stemming. Useful to apply fuzzy matching rules based
        // on user input.
        original_word_len: usize,
        limit: usize,
        max_typo_factor: Option<u32>,
    ) -> Option<impl ExactSizeIterator<Item = (String, u16)> + DoubleEndedIterator + use<>> {
        use indexmap::IndexMap;

        // Word over limit? (abort, the FST does not perform well over large words)
        if Self::word_over_limit(from_word) {
            return None;
        }

        let mut found_words: IndexMap<String, u16> = IndexMap::with_capacity(limit);

        if self.config.prefix_matching_enabled {
            // Try to complete provided word
            if let Some(stream) = self.lookup_begins(from_word, original_word_len) {
                for (word, score) in stream {
                    if found_words.contains_key(&word) {
                        continue;
                    }

                    found_words.insert(word, score);

                    // Requested limit reached? Stop there.
                    if found_words.len() >= limit {
                        break;
                    }
                }
            }
        }

        // Try to fuzzy-suggest other words? (e.g. correct typos)
        if self.config.fuzzy_matching_enabled && found_words.len() < limit {
            // Allow more typos in word as the word gets longer, up to a maximum limit
            let max_typo_factor = max_typo_factor.unwrap_or(typo_factor(original_word_len));
            let mut typo_factor = 1u32;

            // TODO: Rework the Levenshtein query feature to avoid repeating
            //   the same query over and over again. Maybe try to see if
            //   `fst_levenshtein` can return distances in its response.
            while found_words.len() < limit && typo_factor <= max_typo_factor {
                let Some(stream) = self.lookup_typos(from_word, typo_factor) else {
                    break;
                };

                for (word, score) in stream {
                    if found_words.contains_key(&word) {
                        continue;
                    }

                    found_words.insert(word, score);

                    // Requested limit reached? Stop there.
                    if found_words.len() >= limit {
                        break;
                    }
                }

                typo_factor += 1;
            }
        }

        if !found_words.is_empty() {
            Some(found_words.into_iter())
        } else {
            None
        }
    }

    pub fn lookup_begins(
        &self,
        word: &str,
        // Length before stemming. Useful to calculate correct score.
        original_word_len: usize,
    ) -> Option<impl Iterator<Item = (String, u16)>> {
        // Word over limit? (abort, the FST does not perform well over large words)
        if Self::word_over_limit(word) {
            return None;
        }

        if !self.config.prefix_matching_enabled {
            return None;
        }

        let Ok(stream) = self.store.lookup_begins(word) else {
            return None;
        };

        tracing::debug!(?word, "looking up for word in 'begins' fst stream");

        Some(FstStreamIterator(stream).map(move |word| {
            // WARN: Calculating distance to original word length might
            //   yield weird results when combines with stemming.
            let distance: usize = original_word_len.abs_diff(word.len());
            let score = u16::try_from(distance).unwrap_or(u16::MAX);
            (word, score)
        }))
    }

    pub fn lookup_typos(
        &self,
        word: &str,
        typo_factor: u32,
    ) -> Option<impl Iterator<Item = (String, u16)>> {
        if !self.config.fuzzy_matching_enabled {
            return None;
        }

        let Ok(stream) = self.store.lookup_typos(word, typo_factor) else {
            return None;
        };

        tracing::debug!(
            ?word,
            typo_factor,
            "looking up for word in 'typos' fst stream"
        );

        // NOTE: Returning the same score for every word works only
        //   because we re-run `lookup_typos` for increasingly
        //   larger typo factors and do not re-insert existing
        //   values. As explained in previous TODO, we should try
        //   to get the real distance back from `fst_levenshtein`.
        let score = u16::try_from(typo_factor).unwrap_or(u16::MAX);

        Some(FstStreamIterator(stream).map(move |word| (word, score)))
    }

    pub fn list_words(&self, limit: usize, offset: usize) -> Result<Vec<String>, ()> {
        let stream = self.store.as_stream();

        // Enumerate words from FST stream.
        match stream
            .into_strs()
            .map(|words| words.into_iter().skip(offset).take(limit).collect())
        {
            Err(err) => {
                tracing::debug!("conversion of stream failed: {err:?}");
                Err(())
            }
            Ok(words) => Ok(words),
        }
    }

    pub fn count_words(&self) -> usize {
        self.store.cardinality()
    }

    fn word_over_limit(word: &str) -> bool {
        if word.len() > WORD_LIMIT_LENGTH {
            tracing::debug!("got over-limit fst word: {word:?}");

            true
        } else {
            false
        }
    }
}

/// Allow more typos in word as the word gets longer, up to a maximum limit.
pub(crate) fn typo_factor(word_len: usize) -> u32 {
    match word_len {
        1..=3 => 0,
        4..=6 => 1,
        7..=9 => 2,
        _ => 3,
    }
}

// MARK: - Helpers

#[repr(transparent)]
struct FstStreamIterator<'a, A: fst::Automaton>(fst::set::Stream<'a, A>);

impl<'a, A: fst::Automaton> Iterator for FstStreamIterator<'a, A> {
    type Item = String;

    fn next(&mut self) -> Option<Self::Item> {
        match self.0.next() {
            Some(bytes) => match str::from_utf8(bytes) {
                Ok(str) => Some(str.to_owned()),
                Err(_) => None,
            },
            None => None,
        }
    }
}

// MARK: - Tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_proceeds_primitives() {
        let fst_pool = test_fst_pool();

        let store = fst_pool
            .acquire("c:test:2".into(), "b:test:2".into())
            .unwrap();

        assert!(store.lookup_typos("valerien", 1).is_ok());
    }

    // MARK: Helpers

    pub(in crate::store::fst) fn test_fst_pool() -> FstStorePool {
        let fst_store_config = test_fst_store_config();

        FstStorePool::new(fst_store_config, Default::default())
    }

    pub(in crate::store::fst) fn test_fst_store_config() -> Arc<FstStoreConfig> {
        Arc::new(
            config::Config::builder()
                .add_source(config::File::from_str(
                    crate::config::tests::defaults_toml(),
                    config::FileFormat::Toml,
                ))
                .build()
                .unwrap()
                .get::<FstStoreConfig>("store.fst")
                .unwrap(),
        )
    }
}

// MARK: - Boilerplate

impl fmt::Debug for FstStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use crate::util::fmt::{AsPrettyMutex, AsPrettyRwLock};

        // NOTE: Deconstructing to future-proof this function.
        let Self {
            graph,
            target,
            pending,
            last_used,
            last_consolidated,
            should_consolidate,
            action_config,
            // NOTE: We don’t care about the configuration,
            //   we can see it elsewhere if needed.
            config: _,
        } = self;

        f.debug_struct("FstStore")
            .field("graph", graph)
            .field("target", target)
            .field("pending", &AsPrettyMutex(pending))
            .field("last_used", &AsPrettyRwLock(last_used))
            .field("last_consolidated", &AsPrettyRwLock(last_consolidated))
            .field("should_consolidate", should_consolidate)
            .field("action_config", action_config)
            .finish_non_exhaustive()
    }
}
