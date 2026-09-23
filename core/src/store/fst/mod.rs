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
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

use fst::{IntoStreamer as _, Streamer as _};
use hashbrown::HashSet;
use regex_syntax::escape as regex_escape;

use crate::lexer::ranges::LexerRegexRange;

use super::generic::*;

pub use self::pool::{FstStoreId, FstStorePool};
use self::util::*;

pub struct FstStore {
    graph: fst::Set,
    target: FstStoreId,
    pending: FstStorePending,
    last_used: Arc<RwLock<SystemTime>>,
    last_consolidated: Arc<RwLock<SystemTime>>,
    graph_consolidate: Arc<RwLock<HashSet<FstStoreId>>>,
    // NOTE: This shouldn’t be here, but until a big rewrite let’s not care.
    action_config: FstRepositoryConfig,
}

#[derive(Default)]
pub struct FstStorePending {
    pop: Arc<RwLock<HashSet<Vec<u8>>>>,
    push: Arc<RwLock<HashSet<Vec<u8>>>>,
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
        let id = &self.target;

        // Check if not already scheduled.
        if self.graph_consolidate.read().unwrap().contains(id) {
            tracing::debug!("Graph consolidation already scheduled on pool: {id}");
            return;
        };

        // Schedule target for next consolidation tick (i.e. collection + bucket tuple).
        self.graph_consolidate.write().unwrap().insert(id.clone());

        // Bump “last consolidated” time, effectively de-bouncing consolidation
        // to a fixed and predictable tick time in the future.
        let mut last_consolidated_value = self.last_consolidated.write().unwrap();

        *last_consolidated_value = SystemTime::now();

        // Perform an early drop of the lock (frees up write lock early).
        drop(last_consolidated_value);

        tracing::info!("Graph consolidation scheduled on pool: {id}");
    }
}

impl StoreGeneric for FstStore {
    fn ref_last_used(&self) -> &RwLock<SystemTime> {
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
    pub fn push_word(&self, word: &str, fst_store_config: &crate::config::FstStoreConfig) -> bool {
        // Word over limit? (abort, the FST does not perform well over large words)
        if Self::word_over_limit(word) {
            return false;
        }

        let word_bytes = word.as_bytes();

        // Nuke word from 'pop' set? (void a previous un-consolidated commit)
        if self.store.pending.pop.read().unwrap().contains(word_bytes) {
            self.store.pending.pop.write().unwrap().remove(word_bytes);
        }

        // Add word in 'push' set? (only if word is not in FST)
        // NOTE: also check whether FST is over limits or not from there, to avoid
        //   stacking words that could never be consolidated to final FST anyway.
        let graph_fst = self.store.graph.as_fst();

        if self.store.graph.contains(&word) {
            return false;
        }

        if check_over_limits(graph_fst.size(), graph_fst.len(), &fst_store_config.graph) {
            return false;
        }

        {
            let pending_push_guard = self.store.pending.push.read().unwrap();

            if pending_push_guard.contains(word_bytes)
                || pending_push_guard.len() >= fst_store_config.graph.max_words
            {
                return false;
            }
        }

        (self.store.pending.push.write().unwrap()).insert(word_bytes.to_vec());

        self.store.should_consolidate();

        true
    }

    pub fn pop_word(&self, word: &str) -> bool {
        // Word over limit? (abort, the FST does not perform well over large words)
        if Self::word_over_limit(word) {
            return false;
        }

        let word_bytes = word.as_bytes();

        // Nuke word from 'push' set? (void a previous un-consolidated commit)
        if self.store.pending.push.read().unwrap().contains(word_bytes) {
            self.store.pending.push.write().unwrap().remove(word_bytes);
        }

        if !self.store.graph.contains(word_bytes) {
            return false;
        }

        // Add word in 'pop' set? (only if word is in FST)
        if self.store.pending.pop.read().unwrap().contains(word_bytes) {
            return false;
        }

        (self.store.pending.pop.write().unwrap()).insert(word_bytes.to_vec());

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

    pub(in crate::store::fst) fn test_fst_store_config() -> Arc<crate::config::FstStoreConfig> {
        Arc::new(
            config::Config::builder()
                .add_source(config::File::from_str(
                    crate::config::tests::defaults_toml(),
                    config::FileFormat::Toml,
                ))
                .build()
                .unwrap()
                .get::<crate::config::FstStoreConfig>("store.fst")
                .unwrap(),
        )
    }
}

// MARK: - Boilerplate

impl fmt::Debug for FstStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use crate::util::fmt::AsPrettyRwLock;

        // NOTE: Deconstructing to future-proof this function.
        let Self {
            graph,
            target,
            pending,
            last_used,
            last_consolidated,
            graph_consolidate,
            action_config,
        } = self;

        f.debug_struct("FstStore")
            .field("graph", graph)
            .field("target", target)
            .field("pending", pending)
            .field("last_used", &AsPrettyRwLock(last_used))
            .field("last_consolidated", &AsPrettyRwLock(last_consolidated))
            .field("graph_consolidate", &AsPrettyRwLock(graph_consolidate))
            .field("action_config", action_config)
            .finish()
    }
}

impl fmt::Debug for FstStorePending {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use crate::util::fmt::AsPrettyRwLock;

        // NOTE: Deconstructing to future-proof this function.
        let Self { pop, push } = self;

        f.debug_struct("FstStorePending")
            .field("pop", &AsPrettyRwLock(pop))
            .field("push", &AsPrettyRwLock(push))
            .finish()
    }
}
