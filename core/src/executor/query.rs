// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use indexmap::IndexMap;

use super::types::{QueryMatchScore, QueryResultScore, QuerySearchLimit, QuerySearchOffset};
use crate::lexer::preprocessor::{Preprocessor, PreprocessorOutput};
pub use crate::lexer::snippet_matching::QuerySnippetsConfig;
use crate::lexer::snippet_matching::{self, Snippet, get_best_snippets};
use crate::scoring::*;
use crate::store::fst::typo_factor;
use crate::store::kv::KvRepositoryReadOnly;
use crate::store::{Bucket, StoreItemPart, StoreObjectIid, StoreObjectOid, StoreTermHash};
use crate::util::u32_to_usize;

pub struct QueryOptions {
    pub limit: QuerySearchLimit,
    pub offset: QuerySearchOffset,
}

#[derive(Debug)]
pub struct QueryResult {
    pub oid: String,
    pub score: f32,
    pub score_per_term: Vec<Option<QueryTermScore>>,
}

impl super::Executor {
    fn query_(
        &self,
        collection: StoreItemPart,
        bucket: Bucket,
        input: PreprocessorOutput,
        options: QueryOptions,
    ) -> Result<Vec<QueryResult>, ()> {
        let QueryOptions { limit, offset } = options;

        // Important: acquire database access read lock, and reference it in context. This \
        //   prevents the database from being erased while using it in this block.
        let _kv_read_guard = self.kv_pool.lock_read_access();
        let _fst_read_guard = self.fst_pool.lock_read_access();

        let (Ok(kv_store), Ok(fst_store)) = (
            self.kv_pool.acquire(false, collection, None),
            self.fst_pool.acquire(collection, bucket),
        ) else {
            return Err(());
        };

        let Some(kv_store) = kv_store else {
            tracing::debug!(
                "collection store does not exist, consider {bucket:?} from {collection:?} empty"
            );
            return Ok(vec![]);
        };

        let (higher_limit, mut alternates_try) = (
            self.app_conf.search.query_retain_word_objects,
            self.app_conf.search.query_alternates_try,
        );

        let (mut minimum_idf, idf_min_doc_count) = (
            self.app_conf.search.query_minimum_term_idf_default,
            (self.app_conf.search).query_minimum_term_idf_minimum_object_count,
        );

        let (prefix_matching_enabled, fuzzy_matching_enabled) = (
            self.fst_pool.fst_repo_config.prefix_matching_enabled,
            self.fst_pool.fst_repo_config.fuzzy_matching_enabled,
        );

        // Important: acquire bucket store read lock
        executor_kv_lock_read!(kv_store);

        let kv_repo = kv_store.to_repository_read_only(bucket);
        let fst_repo = fst_store.to_repository();

        let document_count = kv_repo
            .get_object_count()
            .map_err(|error| tracing::warn!("{error:?}"))?;

        if document_count < idf_min_doc_count {
            tracing::debug!(
                "ignoring minimum_term_idf ({minimum_idf}) as document_count is too low ({document_count}<{idf_min_doc_count})"
            );
            minimum_idf = 0.;
        }

        let term_count = input.tokens().len();

        let mut iid_limit_per_token = vec![higher_limit / term_count; term_count];

        // Store scores for each found IID. Results will then be sorted by
        // score before being returned. Scores are basically the sum of
        // Levenshtein distances for each term in the query. Lower score
        // means better result.
        // NOTE: We use `IndexMap` instead of `HashMap` to preserve
        //   insertion order, which correlates to reverse data ingestion
        //   order.
        // NOTE: `capacity = 24` to reduce initial grows.
        let mut scoring_matrix: ScoringMatrix =
            ScoringMatrix::with_capacity(24usize.min(usize::from(limit)));

        // Look for exact matches.
        'matches: for (idx, token) in input.tokens().enumerate() {
            let term_hash = token.hash();
            let term = token.as_normalized();

            let idf = {
                let document_frequency = kv_repo.get_document_frequency(&term_hash);
                bm25_lite_idf(document_count, document_frequency)
            };

            // Filter out minimum IDF.
            // PERF: Filtering `minimum_idf > 0` to save some computation.
            if minimum_idf > 0. && idf < minimum_idf {
                tracing::debug!("skipping term {term:?} because idf too low ({idf}<{minimum_idf})");
                continue;
            }

            let mut iid_iters = Vec::with_capacity(3);

            if let Some(iter) = kv_repo
                .get_term_to_iids(&term_hash)
                .unwrap_or_else(|()| None)
            {
                iid_iters.push(iter)
            }

            // Look for exact matches normalized differently if the Sonic
            // index isn’t normalized.
            if !token.is_special() && self.app_conf.normalization.unicode_normalization.is_none() {
                use unicode_normalization::UnicodeNormalization as _;

                if let Some(iter) = kv_repo
                    .get_term_to_iids(&StoreTermHash::from(term.nfc().to_string().as_str()))
                    .unwrap_or_else(|()| None)
                {
                    iid_iters.push(iter)
                }

                if let Some(iter) = kv_repo
                    .get_term_to_iids(&StoreTermHash::from(term.nfd().to_string().as_str()))
                    .unwrap_or_else(|()| None)
                {
                    iid_iters.push(iter)
                }
            };

            // Assign a weight of `1` as those are exact matches.
            let weight = 1.;

            let mut term_to_iids_entries = iid_iters.into_iter().flatten().collect::<Vec<_>>();
            // Sort IIDs by highest tf, to get better results first.
            // NOTE: `sort_by` is stable, meaning ingestion order will still be
            //   used in case of ties.
            term_to_iids_entries.sort_by(|a, b| a.tf().cmp(&b.tf()).reverse());

            for term_to_iids_entry in term_to_iids_entries {
                let tf = term_to_iids_entry.tf();
                let score: f32 = weight * bm25_lite(tf, f64::from(idf));

                let inserted = scoring_matrix.update_score(
                    term_to_iids_entry.iid(),
                    QueryTermScore {
                        score,
                        idf,
                        weight,
                        term: str::to_owned(term),
                    },
                    idx,
                    term_count,
                );

                // Higher limit now reached?
                // Stop acquiring new suggested IIDs now.
                if inserted {
                    iid_limit_per_token[idx] -= 1;
                    if iid_limit_per_token[idx] == 0 {
                        tracing::debug!(?term, "got enough completed results for term");

                        continue 'matches;
                    }
                }
            }
        }

        #[cfg(debug_assertions)]
        tracing::debug!(?scoring_matrix);

        // Look for words containing `term` as prefix.
        if scoring_matrix.len() < higher_limit && alternates_try > 0 && prefix_matching_enabled {
            tracing::debug!(
                "not enough iids were found ({}/{higher_limit}), looking for prefixes",
                scoring_matrix.len(),
            );

            'terms: for (idx, token) in input.tokens().enumerate() {
                let original_len = token.as_original().len();
                let term = token.as_normalized();

                let Some(suggestions) = fst_repo.lookup_begins(term, original_len) else {
                    tracing::trace!("did not get any completed word for term {term:?}");
                    continue 'terms;
                };

                merge_suggestions(
                    suggestions.map(|(w, distance)| (w, prefix_score(distance, original_len))),
                    &mut scoring_matrix,
                    term,
                    idx,
                    term_count,
                    &kv_repo,
                    &mut alternates_try,
                    &mut iid_limit_per_token[idx],
                    document_count,
                    minimum_idf,
                );
            }
        }

        #[cfg(debug_assertions)]
        tracing::debug!(?scoring_matrix);

        // Look for words like `term` (fuzzy matching).
        if scoring_matrix.len() < higher_limit && alternates_try > 0 && fuzzy_matching_enabled {
            tracing::debug!(
                "not enough iids were found ({}/{higher_limit}), looking for fuzzy matches",
                scoring_matrix.len(),
            );

            'terms: for (idx, token) in input.tokens().enumerate() {
                let original_word_len = token.as_original().len();
                let term = token.as_normalized();

                // Skip term if it’s special (we’d want exact matches only).
                if token.is_special() {
                    tracing::debug!("skipping fuzzy search for {term:?}: term is special");
                    continue 'terms;
                }

                let max_typo_factor = typo_factor(original_word_len);
                let mut typo_factor = 1u32;

                // TODO: Rework the Levenshtein query feature to avoid repeating
                //   the same query over and over again. Maybe try to see if
                //   `fst_levenshtein` can return distances in its response.
                while alternates_try > 0 && typo_factor <= max_typo_factor {
                    let Some(suggestions) = fst_repo.lookup_typos(term, typo_factor) else {
                        tracing::trace!("did not get any completed word for term {term:?}");
                        continue 'terms;
                    };

                    merge_suggestions(
                        suggestions
                            .map(|(w, distance)| (w, typo_score(distance, original_word_len))),
                        &mut scoring_matrix,
                        term,
                        idx,
                        term_count,
                        &kv_repo,
                        &mut alternates_try,
                        &mut iid_limit_per_token[idx],
                        document_count,
                        minimum_idf,
                    );

                    typo_factor += 1;
                }
            }
        }

        #[cfg(debug_assertions)]
        tracing::debug!(?scoring_matrix);

        // Switch to implicit `AND` if query contains a special token.
        // NOTE: When a user queries for a special token (e.g. UUID),
        //   they expect only exact matches to be returned. If one term is
        //   considered special, we drop all results missing at least one
        //   term. It’s not the most efficient (compared to not storing the
        //   result in the first place) but it’s an edge case and the cost
        //   is negligible.
        let one_term_is_special = input.tokens().any(|t| t.is_special());
        if one_term_is_special {
            let mut to_remove = Vec::<StoreObjectIid>::new();

            for (&iid, scores) in scoring_matrix.iter() {
                // FIXME: Only remove if it’s a special token that’s missing.
                if scores.iter().any(Option::is_none) {
                    to_remove.push(iid);
                }
            }

            for iid in to_remove {
                scoring_matrix.swap_remove(&iid);
            }
        }

        let mut all_results = (scoring_matrix.into_iter())
            .map(|(iid, term_scores)| {
                // Flatten scores, taking into account missing matches (thanks to `None`).
                let score =
                    overall_score(term_scores.iter().map(|opt| opt.as_ref().map(|v| v.score)));
                (iid, score, term_scores)
            })
            .collect::<Vec<_>>();

        // Sort results.
        all_results.sort_by(|a, b| a.1.total_cmp(&b.1).reverse());

        let page_results = all_results
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

        Ok(page_results)
    }

    #[inline]
    pub fn query(
        &self,
        collection: StoreItemPart,
        bucket: Bucket,
        input: PreprocessorOutput,
        options: QueryOptions,
    ) -> Result<impl ExactSizeIterator<Item = String>, ()> {
        self.query_(collection, bucket, input, options)
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
        let query_results = self.query_(collection, bucket, input, options)?;

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

                    let score_per_term = (score_per_term.into_iter())
                        .flatten()
                        .map(<(String, snippet_matching::TermScore)>::from)
                        .collect::<IndexMap<_, _>>();

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

/// Inversely proportional to `lev_distance / word_len`, decreasing slowly
/// towards `f(20) = 0.5`. Will never reach `0`.
fn prefix_score(lev_distance: u16, word_len: usize) -> f32 {
    // NOTE: Will be `> 1` in practice.
    let lev_ratio = lev_distance as f32 / word_len as f32;

    // NOTE: `20` means that auto-completed words 20 times longer than the
    //   original word get a score of `0.5`. It’s just a magic number, it has
    //   no further meaning. It just feels ok.
    20. / (20. + lev_ratio)
}

#[cfg(test)]
#[test]
fn test_prefix_score() {
    // Auto-complete 2 times longer.
    for n in [2, 4, 8] {
        assert_eq!(prefix_score(n, n as usize), 0.95238096);
    }

    // Auto-complete 4 times longer.
    for n in [2, 4, 8] {
        assert_eq!(prefix_score(3 * n, n as usize), 0.8695652);
    }

    // Auto-complete 1 character.
    assert_eq!(prefix_score(1, 3), 0.9836065);
    assert_eq!(prefix_score(1, 4), 0.9876543);
    assert_eq!(prefix_score(1, 5), 0.99009895);
    assert_eq!(prefix_score(1, 6), 0.9917356);

    // Auto-complete 2 characters.
    assert_eq!(prefix_score(2, 3), 0.96774197);
    assert_eq!(prefix_score(2, 4), 0.9756098);
    assert_eq!(prefix_score(2, 5), 0.98039216);
    assert_eq!(prefix_score(2, 6), 0.9836065);

    // More auto-complete means lower score.
    for n in [1, 2, 4, 8] {
        for word_len in [2, 4, 8, 10] {
            assert!(prefix_score(n + 1, word_len as usize) < prefix_score(n, word_len as usize));
        }
    }
}

/// Levenshtein distance proportional to word length.
fn typo_score(lev_distance: u16, word_len: usize) -> f32 {
    debug_assert!(
        (lev_distance as usize) < word_len,
        "{lev_distance} >= {word_len}"
    );

    // SAFETY: `.min(1)` isn’t strictly necessary as `lev_distance` should
    //   always be `< word_len`, but it’s there as a safety precaution.
    let lev_ratio = (lev_distance as f32 / word_len as f32).min(1.);

    1. - lev_ratio
}

#[cfg(test)]
#[test]
fn test_typo_score() {
    // No typo.
    assert_eq!(typo_score(0, 1), 1.);
    assert_eq!(typo_score(0, 2), 1.);

    // 1 typo.
    assert_eq!(typo_score(1, 3), 0.6666666);
    assert_eq!(typo_score(1, 4), 0.75);
    assert_eq!(typo_score(1, 5), 0.8);
    // 1 typo always scores lower than 0.
    for n in 2..=8 {
        assert!(typo_score(1, n) < typo_score(0, n), "n={n}");
    }

    // 2 typos.
    assert_eq!(typo_score(2, 5), 0.6);
    assert_eq!(typo_score(2, 6), 0.6666666);
    assert_eq!(typo_score(2, 7), 0.71428573);
    // 2 typos always scores lower than 1.
    for n in 3..=8 {
        assert!(typo_score(2, n) < typo_score(1, n), "n={n}");
    }

    // A lot of typos (length has no impact, proportion has).
    assert_eq!(typo_score(1 * 20, 2 * 20), typo_score(1, 2));
    assert_eq!(typo_score(3 * 20, 7 * 20), typo_score(3, 7));
}

fn overall_score(
    scores: impl ExactSizeIterator<Item = Option<QueryMatchScore>>,
) -> QueryResultScore {
    let count = scores.len() as f32;
    let total = scores.map(|opt| opt.unwrap_or(0f32)).sum::<f32>();

    #[allow(clippy::let_and_return)]
    let average = total / count;

    average
}

#[cfg(test)]
#[test]
#[rustfmt::skip]
fn test_overall_score() {
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

#[allow(clippy::too_many_arguments)] // We’ll refactor this someday, and it’ not public anyway.
fn merge_suggestions(
    suggestions: impl Iterator<Item = (String, QueryMatchScore)>,
    scoring_matrix: &mut ScoringMatrix,
    term: &str,
    term_idx: usize,
    term_count: usize,
    kv_repo: &KvRepositoryReadOnly<'_>,
    alternates_try: &mut usize,
    iid_limit: &mut usize,
    document_count: u64,
    minimum_idf: f32,
) {
    // PERF: Abort early.
    if *iid_limit == 0 {
        return;
    }

    'suggestions: for (suggested_word, weight) in suggestions {
        // Do not load base results twice for same term as base term
        if suggested_word.eq(term) {
            continue;
        }

        tracing::trace!(?term, ?suggested_word, "got completed word for term");

        let suggested_term_hash = StoreTermHash::from(suggested_word.as_str());
        let suggested_iids = match kv_repo.get_term_to_iids(&suggested_term_hash) {
            Ok(Some(suggested_iids)) => suggested_iids,
            Ok(None) => continue,
            Err(_) => continue,
        };

        let idf = {
            let document_frequency = kv_repo.get_document_frequency(&suggested_term_hash);
            bm25_lite_idf(document_count, document_frequency)
        };

        // Filter out minimum IDF.
        // PERF: Filtering `minimum_idf > 0` to save some computation.
        if minimum_idf > 0. && idf < minimum_idf {
            tracing::debug!(
                "skipping term {suggested_word:?} because idf too low ({idf}<{minimum_idf})"
            );
            continue;
        }

        for term_to_iids_entry in suggested_iids.into_iter().take(*alternates_try) {
            let tf = term_to_iids_entry.tf();
            let score: f32 = weight * bm25_lite(tf, f64::from(idf));

            // SAFETY: We can reach at most `alternates_try`.
            *alternates_try = unsafe { alternates_try.unchecked_sub(1) };

            let inserted = scoring_matrix.update_score(
                term_to_iids_entry.iid(),
                QueryTermScore {
                    score,
                    idf,
                    weight,
                    term: String::clone(&suggested_word),
                },
                term_idx,
                term_count,
            );

            // Higher limit now reached?
            // Stop acquiring new suggested IIDs now.
            if inserted {
                // SAFETY: `iid_limit` starts `> 0`.
                *iid_limit -= 1;
                if *iid_limit == 0 {
                    tracing::trace!(?term, "got enough completed results for term");

                    break 'suggestions;
                }
            }
        }
    }

    tracing::trace!(
        ?term,
        "done completing results for term, now {} total results",
        scoring_matrix.len()
    );
}

// MARK: - Scoring matrix

/// Score per query term, per object.
#[derive(Debug)]
#[repr(transparent)]
struct ScoringMatrix(IndexMap<StoreObjectIid, Vec<Option<QueryTermScore>>>);

/// Score for a query term, also indicating which term matched (when loose
/// matching).
#[derive(Debug, Clone)]
pub struct QueryTermScore {
    pub score: f32,
    pub idf: f32,
    pub weight: f32,
    pub term: String,
}

impl From<QueryTermScore> for (String, snippet_matching::TermScore) {
    fn from(value: QueryTermScore) -> Self {
        (
            value.term,
            snippet_matching::TermScore {
                weight: value.weight,
                idf: value.idf,
            },
        )
    }
}

impl ScoringMatrix {
    fn with_capacity(n: usize) -> Self {
        Self(IndexMap::with_capacity(n))
    }

    fn swap_remove(&mut self, key: &StoreObjectIid) {
        self.0.swap_remove(key);
    }

    fn update_score(
        &mut self,
        iid: StoreObjectIid,
        new_term_score: QueryTermScore,
        term_idx: usize,
        term_count: usize,
    ) -> bool {
        let new_score = new_term_score.score;

        match self.0.entry(iid) {
            // If entry already exists, use lowest score.
            indexmap::map::Entry::Occupied(mut occupied_entry) => {
                // SAFETY: We always initialize vecs with `term_count` entries.
                let entry_score_opt =
                    unsafe { occupied_entry.get_mut().get_unchecked_mut(term_idx) };

                match entry_score_opt {
                    Some(QueryTermScore {
                        score: current_score,
                        idf,
                        weight,
                        term,
                    }) if *current_score < new_score => {
                        tracing::trace!(
                            current_score,
                            new_score,
                            term_idx,
                            "Updating to max score"
                        );
                        *entry_score_opt = Some(new_term_score);
                    }
                    Some(_) => {}
                    None => {
                        tracing::trace!(score = new_score, term_idx, "Setting max score");
                        *entry_score_opt = Some(new_term_score);
                    }
                }

                false
            }
            // If entry does not exist, insert score.
            indexmap::map::Entry::Vacant(vacant_entry) => {
                let mut scores = vec![None; term_count];

                tracing::trace!(score = new_score, term_idx, "Inserting new score");
                // SAFETY: `scores` has `term_count` elements.
                unsafe { *scores.get_unchecked_mut(term_idx) = Some(new_term_score) };

                vacant_entry.insert(scores);

                true
            }
        }
    }
}

impl std::ops::Deref for ScoringMatrix {
    type Target = IndexMap<StoreObjectIid, Vec<Option<QueryTermScore>>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl IntoIterator for ScoringMatrix {
    type Item = (StoreObjectIid, Vec<Option<QueryTermScore>>);
    type IntoIter = indexmap::map::IntoIter<StoreObjectIid, Vec<Option<QueryTermScore>>>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}
