// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::borrow::Cow;

use hashbrown::HashMap;
use indexmap::IndexMap;

use super::types::{QueryMatchScore, QueryResultScore, QuerySearchLimit, QuerySearchOffset};
use crate::lexer::preprocessor::{Preprocessor, PreprocessorOutput};
pub use crate::lexer::snippet_matching::QuerySnippetsConfig;
use crate::lexer::snippet_matching::{self, Snippet, get_best_snippets};
use crate::scoring::*;
use crate::store::fst::{FstRepository, typo_factor};
use crate::store::kv::{KvRepositoryReadOnly, TermToIidsEntry};
use crate::store::{Bucket, GenericIid, GenericOid, StoreItemPart, StoreTermHash};
use crate::util::u32_to_usize;

// TODO: Make this configurable?
const MAX_QUERY_TOKENS: usize = 20;

pub struct QueryOptions {
    pub limit: QuerySearchLimit,
    pub offset: QuerySearchOffset,
}

#[derive(Debug)]
pub struct QueryResult {
    pub oid: String,
    pub score: f32,
}

#[derive(Debug)]
struct QueryCandidate<'a> {
    term: Cow<'a, str>,
    term_hash: StoreTermHash,
    df: u64,
    idf: f32,
    weight: f32,
    is_required: bool,
}

impl super::Executor {
    /// Given input tokens, returns an ordered list of words to consider
    /// (with typo corrections, etc.).
    ///
    /// This could be seen as some kind of query planner, in a way.
    fn get_query_candidates<'input>(
        &self,
        input: &'input PreprocessorOutput<'input>,
        kv_repo: &KvRepositoryReadOnly<'_>,
        fst_repo: &FstRepository<'_>,
    ) -> Result<Vec<QueryCandidate<'input>>, ()> {
        let alternates_try = self.app_conf.search.query_alternates_try;
        let min_idf = (self.app_conf.search).query_minimum_term_idf_default;
        let min_doc_count = (self.app_conf.search).query_minimum_term_idf_minimum_object_count;

        // PERF: Get document count once (used to compute idfs).
        let document_count = kv_repo
            .get_object_count()
            .map_err(|()| tracing::warn!("Error getting object count"))?;

        #[cfg(debug_assertions)]
        if input.token_count() > MAX_QUERY_TOKENS {
            tracing::debug!("Query is very long: {:?}", input.original_text());
        }

        let mut query_candidates =
            Vec::<QueryCandidate<'input>>::with_capacity(input.token_count());

        let trusted_df = trusted_df(document_count);

        'tokens: for token in input.tokens() {
            let term = token.as_normalized();

            let token_hash = token.hash();
            let token_df = kv_repo.get_document_frequency(&token_hash);
            let token_idf = bm25_idf(document_count, token_df);

            let is_required = token.is_special();

            if token_idf < min_idf && !is_required {
                tracing::trace!(
                    ?term,
                    idf = token_idf,
                    min_idf,
                    "Ignoring query term: idf too low"
                );
                continue 'tokens;
            }

            query_candidates.push(QueryCandidate {
                term: Cow::Borrowed(term),
                term_hash: token_hash,
                df: token_df,
                idf: token_idf,
                weight: 1.,
                is_required,
            });

            // Don’t loose match special tokens.
            if token.is_special() {
                continue 'tokens;
            }

            // Don’t loose match frequent tokens (likely full word with no typo).
            if token_df >= trusted_df {
                continue 'tokens;
            }

            let max_typo_factor = typo_factor(
                token.as_original().len(),
                &self.app_conf.search.typo_factor_word_lengths,
            );

            let Some(suggestions) = fst_repo.suggest_words(term, alternates_try, max_typo_factor)
            else {
                tracing::trace!(?term, "Did not get any word suggestion for term");
                continue 'tokens;
            };

            for (suggestion, weight) in suggestions {
                tracing::trace!(?suggestion, ?term, "Got word suggestion for term");

                let suggestion_hash = StoreTermHash::from(suggestion.as_str());
                let suggestion_df = kv_repo.get_document_frequency(&suggestion_hash);
                let suggestion_idf = bm25_idf(document_count, suggestion_df);

                if suggestion_idf < min_idf && !is_required {
                    tracing::trace!(
                        ?term,
                        idf = suggestion_idf,
                        min_idf,
                        "Ignoring suggested term: idf too low"
                    );
                    continue 'tokens;
                }

                if document_count < min_doc_count
                    || should_correct(token_df, suggestion_df, trusted_df)
                {
                    query_candidates.push(QueryCandidate {
                        term: Cow::Owned(suggestion),
                        term_hash: suggestion_hash,
                        df: suggestion_df,
                        idf: suggestion_idf,
                        weight,
                        is_required: false,
                    });
                } else {
                    tracing::trace!(?suggestion, ?term, "Word suggestion deemed not worth it");
                }
            }
        }

        // Sort terms by df ascending, keeping special terms first.
        // NOTE: A lower df means a term less frequent in the corpus, hence a
        //   likely more important meaning in the query (or a typo).
        query_candidates.sort_by(|a, b| {
            // Keep special terms first
            (a.is_required.cmp(&b.is_required).reverse())
                // Sort by df ascending
                .then(a.df.cmp(&b.df))
        });

        // PERF: Truncate to keep only the most meaningful terms.
        query_candidates.truncate(MAX_QUERY_TOKENS);

        Ok(query_candidates)
    }

    fn query_inner(
        &self,
        input: &'input PreprocessorOutput<'input>,
        options: QueryOptions,
        kv_repo: &KvRepositoryReadOnly<'_>,
        fst_repo: &FstRepository<'_>,
    ) -> Result<(Vec<QueryResult>, Vec<QueryCandidate<'input>>), ()> {
        let QueryOptions { limit, offset } = options;

        let mut retain_word_objects = self.app_conf.search.query_retain_word_objects;
        retain_word_objects = retain_word_objects.max(u32_to_usize(offset) + usize::from(limit));

        // #[cfg(debug_assertions)]
        // eprintln!("q: {}", input.original_text());
        #[cfg(debug_assertions)]
        if input.token_count() > MAX_QUERY_TOKENS {
            tracing::debug!("Query is very long: {:?}", input.original_text());
        }

        let query_candidates = self.get_query_candidates(input, kv_repo, fst_repo)?;

        let scoring_matrix =
            ScoringMatrix::build(retain_word_objects, &query_candidates, |term_hash| {
                kv_repo.get_term_to_iids(term_hash)
            });

        // TODO(perf): Don’t pre-allocate that much.
        let mut all_results = Vec::with_capacity(scoring_matrix.len());
        all_results.extend(scoring_matrix.into_iter(&query_candidates).map(
            |(iid, term_scores)| {
                // Flatten scores, taking into account missing matches (thanks to `None`).
                let score = overall_score(term_scores.iter().copied());

                (iid, score)
            },
        ));

        // Sort results.
        all_results.sort_by(|a, b| a.1.total_cmp(&b.1).reverse());

        let page_results: Vec<QueryResult> = (all_results.into_iter())
            // Perform paging (1/2).
            .skip(u32_to_usize(offset))
            // Resolve IIDs to OIDs.
            // NOTE: This must happen before `.take(limit)`, as otherwise
            //   there’s no guarantee results will contain `limit` results.
            //   This should however happen after `.skip(offset)` so it’s
            //   computed lazily.
            .flat_map(|(iid, score)| {
                if let Ok(Some(oid)) = kv_repo.get_iid_to_oid(iid) {
                    Some(QueryResult { oid, score })
                } else {
                    tracing::error!("failed getting search executor iid-to-oid");
                    None
                }
            })
            // Perform paging (2/2).
            .take(usize::from(limit))
            .collect();

        tracing::info!("got search executor final results: {page_results:?}");

        Ok((page_results, query_candidates))
    }

    pub fn query_(
        &self,
        collection: StoreItemPart,
        bucket: Bucket,
        input: &PreprocessorOutput,
        options: QueryOptions,
    ) -> Result<Vec<QueryResult>, ()> {
        // SAFETY: Acquire database access read lock, and reference it in context.
        //   This prevents the database from being erased while using it in this block.
        let _kv_read_guard = self.kv_pool.lock_read_access();
        let _fst_read_guard = self.fst_pool.lock_read_access();

        let Some(kv_store) = self.kv_pool.acquire(false, collection, None)? else {
            tracing::debug!(
                "collection store does not exist, consider {bucket:?} from {collection:?} empty"
            );
            return Ok(vec![]);
        };
        let fst_store = self.fst_pool.acquire(collection, bucket)?;

        // SAFETY: Acquire bucket store read lock.
        executor_kv_lock_read!(kv_store);

        let kv_repo = kv_store.to_repository_read_only(bucket);
        let fst_repo = fst_store.to_repository();

        self.query_inner(&input, options, &kv_repo, &fst_repo)
            .map(|(results, _)| results)
    }

    #[inline]
    pub fn query(
        &self,
        collection: StoreItemPart,
        bucket: Bucket,
        input: PreprocessorOutput,
        options: QueryOptions,
    ) -> Result<impl ExactSizeIterator<Item = String>, ()> {
        self.query_(collection, bucket, &input, options)
            .map(|results| results.into_iter().map(|res| res.oid))
    }

    pub fn query_with_snippets(
        &self,
        collection: StoreItemPart,
        bucket: Bucket,
        input: PreprocessorOutput,
        options: QueryOptions,
        lang: Option<whatlang::Lang>,
        snippets_config: &QuerySnippetsConfig,
    ) -> Result<Vec<MatchWithSnippets>, ()> {
        // SAFETY: Acquire database access read lock, and reference it in context.
        //   This prevents the database from being erased while using it in this block.
        let _kv_read_guard = self.kv_pool.lock_read_access();
        let _fst_read_guard = self.fst_pool.lock_read_access();

        let Some(kv_store) = self.kv_pool.acquire(false, collection, None)? else {
            tracing::debug!(
                "collection store does not exist, consider {bucket:?} from {collection:?} empty"
            );
            return Ok(vec![]);
        };
        let fst_store = self.fst_pool.acquire(collection, bucket)?;

        // SAFETY: Acquire bucket store read lock.
        executor_kv_lock_read!(kv_store);

        let kv_repo = kv_store.to_repository_read_only(bucket);
        let fst_repo = fst_store.to_repository();

        // ------------

        let (query_results, query_candidates) =
            self.query_inner(&input, options, &kv_repo, &fst_repo)?;

        let object_store_pool = self.object_store_pool.acquire(false, collection, None)?;

        let Some(object_store_pool) = object_store_pool else {
            tracing::debug!(
                "Object store does not exist, consider {bucket:?} from {collection:?} empty"
            );
            let res = (query_results.into_iter())
                .map(|res| MatchWithSnippets::new(res.oid))
                .collect::<Vec<_>>();
            return Ok(res);
        };

        let object_repo = object_store_pool.to_repository(bucket);

        let mut res = Vec::with_capacity(query_results.len());

        let preprocessor = Preprocessor {
            // NOTE: This is important to get accurate tf and add padding to snippets.
            filter_stopwords: false,
            ..Preprocessor::from_app_conf(&self.app_conf)
        };

        let weights_per_term = query_candidates
            .into_iter()
            .map(|candidate| {
                (
                    candidate.term,
                    snippet_matching::TermWeights {
                        weight: candidate.weight,
                        idf: candidate.idf,
                    },
                )
            })
            .collect();

        for QueryResult { oid, .. } in query_results {
            let oid_parsed = GenericOid::from_str(&oid)?;

            let todo = "Match fragments";
            let original = object_repo.get_object_original(oid_parsed, None);

            // Push a new entry no matter if we find the orignal object or even
            // any snippet. All matching objects must be returned, snippets list
            // will just be empty.
            let match_entry = res.push_mut(MatchWithSnippets::new(oid));

            match original {
                Ok(Some(bytes)) => {
                    // TODO(perf): Avoid parsing to string here? Match on raw bytes?
                    let Ok(doc) = String::from_utf8(bytes).inspect_err(|error| {
                        tracing::warn!(
                            ?collection,
                            ?bucket,
                            oid = match_entry.oid,
                            "Invalid byte sequence found in object store: {error}"
                        )
                    }) else {
                        continue;
                    };

                    match_entry.snippets = get_best_snippets(
                        &doc,
                        lang,
                        &preprocessor,
                        *snippets_config,
                        &weights_per_term,
                    );
                }
                Ok(None) => continue,
                Err(error) => {
                    tracing::error!("Error getting original object: {error}");
                    continue;
                }
            }
        }

        Ok(res)
    }
}

#[derive(Debug)]
#[derive(serde::Serialize)]
pub struct MatchWithSnippets {
    pub oid: String,
    pub snippets: Vec<Snippet>,
}

impl MatchWithSnippets {
    #[inline]
    const fn new(oid: String) -> Self {
        Self {
            oid,
            snippets: vec![],
        }
    }
}

/// Df threshold under which a term becomes elligible for typo correction.
///
/// Idf doesn’t work with very large corpora, where a ton of valid words
/// inevitably end under any threshold. Using a constant df wouldn’t scale well
/// either, so this function returns a logarithmic value with a constant floor.
///
/// Examples:
///
/// |     n      | ln(n) | 3 * ln(n) |
/// | ----------:| -----:| ---------:|
/// |      3_000 |   8.0 |       ~24 |
/// |    100_000 |  11.5 |       ~35 |
/// |    500_000 |  13.1 |       ~39 |
/// | 10_000_000 |  16.1 |       ~48 |
fn trusted_df(n_docs: u64) -> u64 {
    const C: f64 = 3.0;
    const FLOOR: u64 = 5;
    ((C * (n_docs.max(2) as f64).ln()) as u64).max(FLOOR)
}

/// Whether or not a typo correction candidate should be considered a valid
/// correction.
///
/// If a correction isn’t far more frequent than the original, it’s not worth
/// using it. It avoids completing `the` as `they` or `them` for example.
fn should_correct(original_df: u64, candidate_df: u64, trusted_df: u64) -> bool {
    const RATIO: u64 = 10;

    if original_df >= trusted_df {
        return false; // Frequent enough to be a real word.
    }

    candidate_df >= original_df.max(1).saturating_mul(RATIO)
}

fn overall_score(
    scores: impl ExactSizeIterator<Item = Option<QueryMatchScore>>,
) -> QueryResultScore {
    let count = scores.len() as f32;
    let scores = scores.map(|opt| opt.unwrap_or(0f32));

    #[cfg(debug_assertions)]
    let scores = {
        use std::sync::atomic::{AtomicU8, Ordering};

        let scores = scores.collect::<Vec<_>>();

        // if !scores.iter().any(|v| *v == 0.) {
        static COUNTER: AtomicU8 = AtomicU8::new(0);
        if COUNTER
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .is_ok_and(|prev| prev < 200)
        {
            tracing::debug!("scores = {scores:?}");
        }
        // }

        scores.into_iter()
    };

    let total = scores.sum::<f32>();

    #[allow(clippy::let_and_return)]
    let average = total / count;

    average
}

#[cfg(test)]
#[test]
#[rustfmt::skip]
fn test_overall_score() {
    use crate::store::fst::{typo_score, prefix_score};

    const MISSING: Option<QueryMatchScore> = None;
    const EXACT_MATCH: Option<QueryMatchScore> = Some(1.);

    // Max score for exact matches.
    assert_eq!(overall_score([EXACT_MATCH; 1].into_iter()), 1.);
    assert_eq!(overall_score([EXACT_MATCH; 2].into_iter()), 1.);
    assert_eq!(overall_score([EXACT_MATCH; 3].into_iter()), 1.);
    assert_eq!(overall_score([EXACT_MATCH; 4].into_iter()), 1.);

    // Lowest score for missing matches.
    assert_eq!(overall_score([MISSING; 1].into_iter()), 0.);
    assert_eq!(overall_score([MISSING; 2].into_iter()), 0.);
    assert_eq!(overall_score([MISSING; 3].into_iter()), 0.);
    assert_eq!(overall_score([MISSING; 4].into_iter()), 0.);

    // Auto-complete > fuzzy matching (not always, but in most cases).
    assert!(
          overall_score([Some(prefix_score(5,  4))].into_iter())
        > overall_score([Some(  typo_score(1, 10))].into_iter())
    );

    // Missing one term.
    assert_eq!(overall_score([MISSING, EXACT_MATCH             ].into_iter()), 1. / 2.);
    assert_eq!(overall_score([MISSING, EXACT_MATCH, EXACT_MATCH].into_iter()), 2. / 3.);
    // Missing one term is better than missing all terms.
    assert!(
          overall_score([MISSING, EXACT_MATCH].into_iter())
        > overall_score([MISSING             ].into_iter())
    );

    // Term order has no meaning.
    assert_eq!(
        overall_score([
            EXACT_MATCH,
            Some(prefix_score(2, 3)),
            Some(typo_score(2, 7))
        ].into_iter()),
        overall_score([
            Some(typo_score(2, 7)),
            Some(prefix_score(2, 3)),
            EXACT_MATCH
        ].into_iter())
    );

    // All typos in one term is like the same total across multiple terms.
    // NOTE: This is not a requirement, it’s just a non-regression test.
    assert_eq!(
        overall_score([Some(typo_score(1, 7)); 2]          .into_iter()),
        overall_score([Some(typo_score(2, 7)), EXACT_MATCH].into_iter())
    );
    assert_eq!(
        overall_score([Some(typo_score(1, 7)); 3]                       .into_iter()),
        overall_score([Some(typo_score(3, 7)), EXACT_MATCH, EXACT_MATCH].into_iter())
    );

    // Examples for “The brown fox jumps over the lazy dog”:
    // “brown fox jumps”
    assert_eq!(overall_score([EXACT_MATCH, EXACT_MATCH, EXACT_MATCH].into_iter()), 1.);
    // “brown fox jum”
    assert_eq!(
        overall_score([EXACT_MATCH, EXACT_MATCH, Some(prefix_score(2, 3))].into_iter()),
        0.9892473
    );
    // “bron fox jum”
    assert_eq!(
        overall_score([
            Some(typo_score(1, 5)),
            EXACT_MATCH,
            Some(prefix_score(2, 3))
        ].into_iter()),
        0.92258066
    );
    // “brown fox”
    assert_eq!(overall_score([EXACT_MATCH, EXACT_MATCH].into_iter()), 1.);
    // “brown fox eats”
    assert_eq!(
        overall_score([EXACT_MATCH, EXACT_MATCH, MISSING].into_iter()),
        0.6666667
    ); // 2/3
}

// MARK: - Scoring matrix

/// Score per query term, per object.
struct ScoringMatrix {
    map: IndexMap<GenericIid, Vec<Option<QueryMatchScore>>>,
    term_count: usize,
}

impl ScoringMatrix {
    fn new(term_count: usize) -> Self {
        Self {
            map: IndexMap::new(),
            term_count,
        }
    }

    fn build<'input, 'candidates, I>(
        retain_word_objects: usize,
        query_candidates: &'candidates [QueryCandidate<'_>],
        get_term_to_iids: impl Fn(&'candidates StoreTermHash) -> Result<Option<I>, ()> + Copy,
    ) -> Self
    where
        I: ExactSizeIterator<Item = TermToIidsEntry> + 'candidates,
    {
        let mut scoring_matrix = ScoringMatrix::new(query_candidates.len());

        let mut accept_new_iids = true;

        // Populate the scoring matrix with IIDs.
        for (
            term_idx,
            QueryCandidate {
                term_hash,
                idf,
                weight,
                is_required,
                ..
            },
        ) in query_candidates.iter().enumerate()
        {
            for term_to_iids_entry in get_term_to_iids(&term_hash)
                .unwrap_or_else(|()| None)
                .into_iter()
                .flatten()
                .take(retain_word_objects)
            {
                let iid = term_to_iids_entry.iid();

                let tf = term_to_iids_entry.tf();
                let bm25_score = weight * bm25_lite(tf, *idf);

                scoring_matrix.set_score(iid, bm25_score, term_idx, accept_new_iids);
            }

            if *is_required {
                // PERF: Don’t store new IIDs in the scoring matrix from now on;
                //   it would be filtered out later when applying the AND logic.
                accept_new_iids = false;
            }
        }

        scoring_matrix.backfill(&query_candidates, get_term_to_iids);

        scoring_matrix
    }

    fn set_score(
        &mut self,
        iid: GenericIid,
        score: QueryMatchScore,
        term_idx: usize,
        accept_new_iids: bool,
    ) -> bool {
        match self.map.entry(iid) {
            // If entry already exists, set term score.
            indexmap::map::Entry::Occupied(mut occupied_entry) => {
                // SAFETY: We always initialize vecs with `term_count` entries.
                let entry_score_opt =
                    unsafe { occupied_entry.get_mut().get_unchecked_mut(term_idx) };

                debug_assert!(entry_score_opt.is_none());

                tracing::trace!(term_idx, ?iid, ?score, "Setting new score");
                *entry_score_opt = Some(score);

                false
            }
            // If entry does not exist, insert score.
            indexmap::map::Entry::Vacant(vacant_entry) if accept_new_iids => {
                let mut scores = vec![None; self.term_count];

                tracing::trace!(term_idx, ?iid, ?score, "Inserting new entry");
                // SAFETY: `scores` has `term_count` elements.
                unsafe { *scores.get_unchecked_mut(term_idx) = Some(score) };

                vacant_entry.insert(scores);

                true
            }
            indexmap::map::Entry::Vacant(_) => {
                tracing::trace!(term_idx, ?iid, ?score, "Not inserting new entry");

                false
            }
        }
    }

    fn backfill<'candidates, I>(
        &mut self,
        query_candidates: &'candidates [QueryCandidate],
        get_term_to_iids: impl Fn(&'candidates StoreTermHash) -> Result<Option<I>, ()> + Copy,
    ) where
        I: ExactSizeIterator<Item = TermToIidsEntry> + 'candidates,
    {
        let mut to_backfill = HashMap::<GenericIid, usize>::with_capacity(self.map.len());

        for term_idx in 0..query_candidates.len() {
            to_backfill.clear();

            for (iid_idx, (iid, score_per_term)) in self.map.iter().enumerate() {
                if score_per_term[term_idx].is_none() {
                    to_backfill.insert(*iid, iid_idx);
                }
            }

            if to_backfill.is_empty() {
                continue;
            }

            let token = &query_candidates[term_idx];
            let term_hash = &token.term_hash;

            let Some(term_to_iids_entries) = get_term_to_iids(term_hash).unwrap_or_else(|()| None)
            else {
                continue;
            };

            let weight = token.weight;
            let idf = token.idf;

            for entry in term_to_iids_entries {
                if let Some(&iid_idx) = to_backfill.get(&entry.iid()) {
                    let score: f32 = weight * bm25_lite(entry.tf(), idf);

                    self.map[iid_idx][term_idx] = Some(score);
                }
            }
        }
    }
}

impl std::ops::Deref for ScoringMatrix {
    type Target = IndexMap<GenericIid, Vec<Option<QueryMatchScore>>>;

    fn deref(&self) -> &Self::Target {
        &self.map
    }
}

impl std::fmt::Debug for ScoringMatrix {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ScoringMatrix")
            .field(&self.map)
            .finish_non_exhaustive()
    }
}

impl ScoringMatrix {
    #[inline]
    fn into_iter<'a>(
        self,
        query_candidates: &'a [QueryCandidate<'_>],
    ) -> impl Iterator<Item = (GenericIid, Vec<Option<QueryMatchScore>>)> + use<'a> {
        let required_terms = query_candidates
            .iter()
            .take_while(|qc| qc.is_required)
            .count();

        self.map.into_iter().filter(move |(_iid, term_scores)| {
            !term_scores[..required_terms].iter().any(Option::is_none)
        })
    }
}
