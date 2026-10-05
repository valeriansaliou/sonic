// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use indexmap::IndexMap;

use super::types::{QueryMatchScore, QueryResultScore, QuerySearchLimit, QuerySearchOffset};
use crate::lexer::itertools::UniqueBy;
use crate::lexer::preprocessor::{Preprocessor, PreprocessorOutput, Token};
pub use crate::lexer::snippet_matching::QuerySnippetsConfig;
use crate::lexer::snippet_matching::{Snippet, get_best_snippets};
use crate::store::fst::typo_factor;
use crate::store::kv::KvRepositoryReadOnly;
use crate::store::{Bucket, StoreItemPart, StoreObjectIid, StoreObjectOid, StoreTermHash};
use crate::util::hash::NoopU32HasherBuilder;

type ScoringMatrix = IndexMap<StoreObjectIid, Vec<Option<(String, f32)>>>;

pub struct QueryOptions {
    pub limit: QuerySearchLimit,
    pub offset: QuerySearchOffset,
}

impl super::Executor {
    fn query_(
        &self,
        collection: StoreItemPart,
        bucket: Bucket,
        input: PreprocessorOutput,
        options: QueryOptions,
    ) -> Result<(Vec<(String, StoreObjectIid)>, ScoringMatrix), ()> {
        let QueryOptions { limit, offset } = options;

        // Important: acquire database access read lock, and reference it in context. This \
        //   prevents the database from being erased while using it in this block.
        let _kv_read_guard = self.kv_pool.lock_read_access();
        let _fst_read_guard = self.fst_pool.lock_read_access();

        let (Ok(kv_store), Ok(fst_store)) = (
            self.kv_pool.acquire(false, collection, None, |_| {}),
            self.fst_pool.acquire(collection, bucket),
        ) else {
            return Err(());
        };

        let Some(kv_store) = kv_store else {
            tracing::debug!(
                "collection store does not exist, consider {bucket:?} from {collection:?} empty"
            );
            return Ok((vec![], ScoringMatrix::new()));
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
            .map_err(|error| tracing::warn!("{error:?}"))? as u64;

        if document_count < idf_min_doc_count {
            tracing::debug!(
                "ignoring minimum_term_idf ({minimum_idf}) as document_count is too low ({document_count}<{idf_min_doc_count})"
            );
            minimum_idf = 0.;
        }

        // Collect all terms so we know the count right ahead.
        // PERF: This helps allocating the correct amounts of memory.
        let tokens: Vec<Token> =
            UniqueBy::new_with_hasher(input.tokens(), Token::hash, NoopU32HasherBuilder).collect();
        let term_count = tokens.len();

        // Store scores for each found IID. Results will then be sorted by
        // score before being returned. Scores are basically the sum of
        // Levenshtein distances for each term in the query. Lower score
        // means better result.
        // NOTE: We use `IndexMap` instead of `HashMap` to preserve
        //   insertion order, which correlates to reverse data ingestion
        //   order.
        // NOTE: `capacity = 24` to reduce initial grows.
        let mut scoring_matrix: ScoringMatrix =
            IndexMap::with_capacity(24usize.min(usize::from(limit)));

        // Look for exact matches.
        'matches: for (idx, token) in tokens.iter().enumerate() {
            let term_hash = token.hash();
            let term = token.as_normalized();

            let mut iids = kv_repo
                .get_term_to_iids(&term_hash)
                .unwrap_or(None)
                .unwrap_or_default();

            // Look for exact matches normalized differently if the Sonic
            // index isn’t normalized.
            if !token.is_special() && self.app_conf.normalization.unicode_normalization.is_none() {
                use unicode_normalization::UnicodeNormalization as _;

                let mut nfc = kv_repo
                    .get_term_to_iids(&StoreTermHash::from(term.nfc().to_string().as_str()))
                    .unwrap_or(None)
                    .unwrap_or_default();
                iids.append(&mut nfc);

                let mut nfd = kv_repo
                    .get_term_to_iids(&StoreTermHash::from(term.nfd().to_string().as_str()))
                    .unwrap_or(None)
                    .unwrap_or_default();
                iids.append(&mut nfd);
            };

            tracing::debug!("got exact search executor iids: {iids:?} for term: {term:?}");

            let document_frequency = document_frequency(&term_hash, &kv_repo);

            // Filter out minimum IDF.
            // PERF: Filtering `minimum_idf > 0` to save some computation.
            if minimum_idf > 0. {
                let idf = (document_count as f32 / document_frequency as f32).ln();
                if idf < minimum_idf {
                    tracing::debug!(
                        "skipping term {term:?} because idf too low ({idf}<{minimum_idf})"
                    );
                    continue;
                }
            }

            let bm25_score = bm25_lite_idf(document_count, document_frequency);

            for iid in iids.into_iter() {
                // Assign a base score of `1` as those are exact matches.
                let inserted = update_score(
                    &mut scoring_matrix,
                    iid,
                    term.to_owned(),
                    1. * bm25_score,
                    idx,
                    term_count,
                );

                if inserted {
                    // Higher limit now reached?
                    // Stop acquiring new suggested IIDs now.
                    if scoring_matrix.len() >= higher_limit {
                        tracing::trace!(?term, "got enough completed results for term");

                        break 'matches;
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

            'terms: for (idx, token) in tokens.iter().enumerate() {
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
                    higher_limit,
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

            'terms: for (idx, token) in tokens.iter().enumerate() {
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
                        higher_limit,
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
        let one_term_is_special = tokens.iter().any(Token::is_special);
        if one_term_is_special {
            let mut to_remove = Vec::<StoreObjectIid>::new();

            for (&iid, scores) in scoring_matrix.iter() {
                if scores.iter().any(Option::is_none) {
                    to_remove.push(iid);
                }
            }

            for iid in to_remove {
                scoring_matrix.swap_remove(&iid);
            }
        }

        // Flatten scores, taking into account missing matches (thanks to
        // `None`).
        let found_iids = scoring_matrix.iter().map(|(iid, scores)| {
            (
                iid,
                overall_score(scores.iter().map(|o| o.as_ref().map(|(_, s)| *s))),
            )
        });

        // Sort found IIDs.
        let all_iids = {
            let mut all_iids = found_iids.collect::<Vec<_>>();
            all_iids.sort_by(|a, b| a.1.total_cmp(&b.1).reverse());
            all_iids.into_iter().map(|(iid, _score)| iid)
        };

        // Resolve OIDs from IIDs
        // Notice: we also proceed paging from there
        let (limit_usize, offset_usize) = (limit as usize, offset as usize);
        let mut result_oids = Vec::with_capacity(limit_usize);

        'paging: for (index, found_iid) in all_iids.skip(offset_usize).enumerate() {
            // Stop there?
            if index >= limit_usize {
                break 'paging;
            }

            // Read IID-to-OID for this found IID
            if let Ok(Some(oid)) = kv_repo.get_iid_to_oid(*found_iid) {
                result_oids.push((oid, *found_iid));
            } else {
                tracing::error!("failed getting search executor iid-to-oid");
            }
        }

        tracing::info!("got search executor final oids: {:?}", result_oids);

        Ok((result_oids, scoring_matrix))
    }

    #[inline]
    pub fn query(
        &self,
        collection: StoreItemPart,
        bucket: Bucket,
        input: PreprocessorOutput,
        options: QueryOptions,
    ) -> Result<Vec<String>, ()> {
        self.query_(collection, bucket, input, options)
            .map(|res| res.0.into_iter().map(|(oid, _)| oid).collect())
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
        let (oids, mut scoring_matrix) = self.query_(collection, bucket, input, options)?;

        let object_store_pool = self
            .object_store_pool
            .acquire(false, collection, None, |_| {})?;

        let Some(object_store_pool) = object_store_pool else {
            tracing::debug!(
                "Object store does not exist, consider {bucket:?} from {collection:?} empty"
            );
            let res = (oids.into_iter())
                .map(|(oid, _)| MatchWithSnippets::new(oid))
                .collect::<Vec<_>>();
            return Ok(res);
        };

        let object_repo = object_store_pool.to_repository(bucket);

        let mut res = Vec::with_capacity(oids.len());

        let preprocessor = Preprocessor {
            // NOTE: This is important to get accurate tf and add padding to snippets.
            filter_stopwords: false,
            ..Preprocessor::from_app_conf(&self.app_conf)
        };

        for (oid, iid) in oids {
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

                    let indexmap::map::Entry::Occupied(mut scoring_matrix_entry) =
                        scoring_matrix.entry(iid)
                    else {
                        if cfg!(debug_assertions) {
                            panic!("IID missing from scoring matrix; this should not happen.");
                        } else {
                            continue;
                        }
                    };
                    let scoring_matrix_entry = std::mem::take(scoring_matrix_entry.get_mut());

                    let idf_by_token: std::collections::HashMap<String, (f32, usize)> =
                        (scoring_matrix_entry.into_iter())
                            .enumerate()
                            .flat_map(|(index, opt)| {
                                opt.map(|(term, score)| (term, (score, index)))
                            })
                            .collect();

                    match_entry.snippets = get_best_snippets(
                        &doc,
                        lang,
                        &preprocessor,
                        *snippets_config,
                        idf_by_token,
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

fn document_frequency(term_hash: &StoreTermHash, kv_repo: &KvRepositoryReadOnly<'_>) -> u64 {
    kv_repo
        .get_term_to_iids(term_hash)
        .inspect_err(|err| tracing::error!("{err:?}"))
        .unwrap_or(None)
        .map_or(0, |iids| iids.len()) as u64
}

fn bm25_lite_idf(document_count: u64, document_frequency: u64) -> f32 {
    debug_assert!(
        document_frequency <= document_count,
        "{document_frequency} > {document_count}"
    );

    let document_count = document_count.max(document_frequency) as f64;
    let df = document_frequency as f64;

    (1.0 + (document_count - df + 0.5) / (df + 0.5)).ln() as f32
}

#[allow(clippy::too_many_arguments)] // We’ll refactor this someday, and it’ not public anyway.
fn merge_suggestions(
    suggestions: impl Iterator<Item = (String, QueryMatchScore)>,
    scoring_matrix: &mut IndexMap<StoreObjectIid, Vec<Option<(String, QueryMatchScore)>>>,
    term: &str,
    term_idx: usize,
    term_count: usize,
    kv_repo: &KvRepositoryReadOnly<'_>,
    alternates_try: &mut usize,
    higher_limit: usize,
    document_count: u64,
    minimum_idf: f32,
) {
    'suggestions: for (suggested_word, base_score) in suggestions {
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

        let document_frequency = document_frequency(&suggested_term_hash, kv_repo);

        // Filter out minimum IDF.
        // PERF: Filtering `minimum_idf > 0` to save some computation.
        if minimum_idf > 0. {
            let idf = (document_count as f32 / document_frequency as f32).ln();
            if idf < minimum_idf {
                tracing::debug!(
                    "skipping term {suggested_word:?} because idf too low ({idf}<{minimum_idf})"
                );
                continue;
            }
        }

        let bm25_score = bm25_lite_idf(document_count, document_frequency);

        let suggestion_score = base_score * bm25_score;

        for suggested_iid in suggested_iids.into_iter().take(*alternates_try) {
            // SAFETY: We can reach at most `alternates_try`.
            *alternates_try = unsafe { alternates_try.unchecked_sub(1) };

            let inserted = update_score(
                scoring_matrix,
                suggested_iid,
                suggested_word.clone(),
                suggestion_score,
                term_idx,
                term_count,
            );

            if inserted {
                // Higher limit now reached?
                // Stop acquiring new suggested IIDs now.
                if scoring_matrix.len() >= higher_limit {
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

fn update_score(
    scoring_matrix: &mut IndexMap<StoreObjectIid, Vec<Option<(String, QueryMatchScore)>>>,
    iid: StoreObjectIid,
    term: String,
    score: QueryMatchScore,
    term_idx: usize,
    term_count: usize,
) -> bool {
    match scoring_matrix.entry(iid) {
        // If entry already exists, use lowest score.
        indexmap::map::Entry::Occupied(mut occupied_entry) => {
            // SAFETY: We always initialize vecs with `term_count` entries.
            let entry_score_opt = unsafe { occupied_entry.get_mut().get_unchecked_mut(term_idx) };

            match entry_score_opt {
                Some((_, entry_score)) if *entry_score > score => {
                    tracing::trace!(entry_score, score, "Updating to min score");
                    *entry_score_opt = Some((term, score));
                }
                Some(_) => {}
                None => {
                    tracing::trace!(score, "Setting min score");
                    *entry_score_opt = Some((term, score));
                }
            }

            false
        }
        // If entry does not exist, insert score.
        indexmap::map::Entry::Vacant(vacant_entry) => {
            let mut scores = vec![None; term_count];

            tracing::trace!(new_score = score, "Inserting new score");
            // SAFETY: `scores` has `term_count` elements.
            unsafe { *scores.get_unchecked_mut(term_idx) = Some((term, score)) };

            vacant_entry.insert(scores);

            true
        }
    }
}
