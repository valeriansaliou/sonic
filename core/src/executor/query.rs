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
use crate::store::kv::KvRepositoryReadOnly;
use crate::store::{Bucket, StoreItemPart, StoreObjectIid, StoreObjectOid, StoreTermHash};
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
    score_per_term: Vec<Option<IidTermScore>>,
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
    fn query_<'input>(
        &self,
        collection: StoreItemPart,
        bucket: Bucket,
        input: &'input PreprocessorOutput<'input>,
        options: QueryOptions,
    ) -> Result<(Vec<QueryResult>, Vec<QueryCandidate<'input>>), ()> {
        // eprintln!("q: {}", input.original_text());
        let QueryOptions { limit, offset } = options;

        let alternates_try = self.app_conf.search.query_alternates_try;
        let mut retain_word_objects = self.app_conf.search.query_retain_word_objects;
        retain_word_objects = retain_word_objects.max(u32_to_usize(offset) + usize::from(limit));
        let min_idf = (self.app_conf.search).query_minimum_term_idf_default;
        let min_doc_count = (self.app_conf.search).query_minimum_term_idf_minimum_object_count;

        // ------------

        // SAFETY: Acquire database access read lock, and reference it in context.
        //   This prevents the database from being erased while using it in this block.
        let _kv_read_guard = self.kv_pool.lock_read_access();
        let _fst_read_guard = self.fst_pool.lock_read_access();

        let Some(kv_store) = self.kv_pool.acquire(false, collection, None)? else {
            tracing::debug!(
                "collection store does not exist, consider {bucket:?} from {collection:?} empty"
            );
            return Ok((vec![], vec![]));
        };
        let fst_store = self.fst_pool.acquire(collection, bucket)?;

        // SAFETY: Acquire bucket store read lock.
        executor_kv_lock_read!(kv_store);

        let kv_repo = kv_store.to_repository_read_only(bucket);
        let fst_repo = fst_store.to_repository();

        // ------------

        // PERF: Get document count once (used to compute idfs).
        let document_count = kv_repo
            .get_object_count()
            .map_err(|error| tracing::warn!("Error getting object count: {error}"))?;

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

            let Some(suggestions) = fst_repo.suggest_words(term, alternates_try, None) else {
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

        let mut scoring_matrix =
            ScoringMatrix::with_capacity(retain_word_objects, query_candidates.len());

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
            for term_to_iids_entry in kv_repo
                .get_term_to_iids(&term_hash)
                .unwrap_or_else(|()| None)
                .into_iter()
                .flatten()
                .take(retain_word_objects)
            {
                let iid = term_to_iids_entry.iid();

                let tf = term_to_iids_entry.tf();
                let bm25_score = weight * bm25_lite(tf, *idf);

                let score_entry = IidTermScore {
                    score: bm25_score,
                    idf: *idf,
                    weight: *weight,
                };

                scoring_matrix.set_score(iid, score_entry, term_idx, accept_new_iids);
            }

            if *is_required {
                // PERF: Don’t store new IIDs in the scoring matrix from now on;
                //   it would be filtered out later when applying the AND logic.
                accept_new_iids = false;
            }
        }

        scoring_matrix.backfill(&kv_repo, &query_candidates);

        let mut all_results = Vec::with_capacity(scoring_matrix.len());
        all_results.extend(
            (scoring_matrix.into_iter()).filter_map(|(iid, term_scores)| {
                let required_terms = query_candidates
                    .iter()
                    .take_while(|qc| qc.is_required)
                    .count();

                if term_scores[..required_terms].iter().any(Option::is_none) {
                    return None;
                };

                // Flatten scores, taking into account missing matches (thanks to `None`).
                let score =
                    overall_score(term_scores.iter().map(|opt| opt.as_ref().map(|v| v.score)));

                Some((iid, score, term_scores))
            }),
        );

        // Sort results.
        all_results.sort_by(|a, b| a.1.total_cmp(&b.1).reverse());

        let page_results: Vec<QueryResult> = all_results
            .into_iter()
            // Perform paging.
            .skip(u32_to_usize(offset))
            .take(usize::from(limit))
            // Resolve IIDs to OIDs.
            .flat_map(|(iid, score, score_per_term)| {
                if let Ok(Some(oid)) = kv_repo.get_iid_to_oid(iid) {
                    Some(QueryResult {
                        oid,
                        score,
                        score_per_term,
                    })
                } else {
                    tracing::error!("failed getting search executor iid-to-oid");
                    None
                }
            })
            .collect();

        tracing::info!("got search executor final results: {page_results:?}");

        Ok((page_results, query_candidates))
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
            .map(|(results, _)| results.into_iter().map(|res| res.oid))
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
        let (query_results, query_candidates) = self.query_(collection, bucket, &input, options)?;

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

        for QueryResult {
            oid,
            score_per_term,
            ..
        } in query_results
        {
            let oid_parsed = StoreObjectOid::from_str(&oid)?;

            let original = object_repo.get_object_original(oid_parsed);

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

                    let score_per_term = score_per_term
                        .into_iter()
                        .enumerate()
                        .flat_map(|(term_idx, opt)| {
                            opt.map(|score| {
                                let term =
                                    Cow::clone(&query_candidates[term_idx].term).into_owned();

                                (
                                    term,
                                    snippet_matching::TermScore {
                                        weight: score.weight,
                                        idf: score.idf,
                                    },
                                )
                            })
                        })
                        .collect();

                    match_entry.snippets = get_best_snippets(
                        &doc,
                        lang,
                        &preprocessor,
                        *snippets_config,
                        score_per_term,
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

#[derive(Debug)]
struct IidTermScore {
    score: f32,
    idf: f32,
    weight: f32,
}

/// Score per query term, per object.
struct ScoringMatrix {
    map: IndexMap<StoreObjectIid, Vec<Option<IidTermScore>>>,
    term_count: usize,
}

impl ScoringMatrix {
    fn with_capacity(n: usize, term_count: usize) -> Self {
        Self {
            map: IndexMap::with_capacity(n),
            term_count,
        }
    }

    fn set_score(
        &mut self,
        iid: StoreObjectIid,
        score: IidTermScore,
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

    fn backfill(&mut self, index: &KvRepositoryReadOnly, query_candidates: &[QueryCandidate]) {
        let mut to_backfill =
            vec![HashMap::<StoreObjectIid, usize>::with_capacity(self.map.len()); self.term_count];

        for (iid_idx, (iid, score_per_term)) in self.map.iter_mut().enumerate() {
            for (token_idx, score_opt) in score_per_term.iter_mut().enumerate() {
                if score_opt.is_none() {
                    to_backfill[token_idx].insert(*iid, iid_idx);
                }
            }
        }

        for (token_idx, iids) in to_backfill.into_iter().enumerate() {
            let token = &query_candidates[token_idx];
            let term_hash = token.term_hash;
            let weight = token.weight;
            let idf = token.idf;

            let Some(term_to_iids_entries) =
                index.get_term_to_iids(&term_hash).unwrap_or_else(|()| None)
            else {
                continue;
            };

            for entry in term_to_iids_entries {
                if let Some(&iid_idx) = iids.get(&entry.iid()) {
                    let score: f32 = weight * bm25_lite(entry.tf(), idf);

                    self.map[iid_idx][token_idx] = Some(IidTermScore { score, idf, weight });
                }
            }
        }
    }
}

impl std::ops::Deref for ScoringMatrix {
    type Target = IndexMap<StoreObjectIid, Vec<Option<IidTermScore>>>;

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

impl IntoIterator for ScoringMatrix {
    type Item = (StoreObjectIid, Vec<Option<IidTermScore>>);
    type IntoIter = indexmap::map::IntoIter<StoreObjectIid, Vec<Option<IidTermScore>>>;

    fn into_iter(self) -> Self::IntoIter {
        self.map.into_iter()
    }
}

impl Clone for IidTermScore {
    fn clone(&self) -> Self {
        unreachable!("Only `Option::<IidTermScore>::None` should ever be cloned.")
    }
}
