// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use indexmap::IndexMap;

use crate::lexer::{NormalizedToken, TokenLexer};
use crate::query::{
    QueryMatchScore, QueryResultScore, QuerySearchID, QuerySearchLimit, QuerySearchOffset,
};
use crate::store::StoreItem;
use crate::store::fst::{StoreFSTActionBuilder, typo_factor};
use crate::store::identifiers::{StoreObjectIID, StoreTermHash, StoreTermHashed};
use crate::store::kv::{StoreKVAcquireMode, StoreKVAction, StoreKVActionBuilder};

impl super::Executor {
    pub fn search(
        &self,
        item: StoreItem,
        _event_id: QuerySearchID,
        lexer: TokenLexer,
        limit: QuerySearchLimit,
        offset: QuerySearchOffset,
    ) -> Result<Vec<String>, ()> {
        if let StoreItem(collection, Some(bucket), None) = item {
            // Important: acquire database access read lock, and reference it in context. This \
            //   prevents the database from being erased while using it in this block.
            let _kv_read_guard = self.kv_pool.lock_read_access();
            let _fst_read_guard = self.fst_pool.lock_read_access();

            let (Ok(kv_store), Ok(fst_store)) = (
                self.kv_pool
                    .acquire(StoreKVAcquireMode::OpenOnly, collection),
                self.fst_pool.acquire(collection, bucket),
            ) else {
                return Err(());
            };

            let (higher_limit, mut alternates_try) = (
                self.app_conf.store.kv.retain_word_objects,
                self.app_conf.search.query_alternates_try,
            );

            let (prefix_matching_enabled, fuzzy_matching_enabled) = (
                self.fst_pool.fst_action_config.prefix_matching_enabled,
                self.fst_pool.fst_action_config.fuzzy_matching_enabled,
            );

            // Important: acquire bucket store read lock
            executor_kv_lock_read!(kv_store);

            let (kv_action, fst_action) = (
                StoreKVActionBuilder::access(bucket, kv_store),
                StoreFSTActionBuilder::access(fst_store),
            );

            // Collect all terms so we know the count right ahead.
            // PERF: This helps allocating the correct amounts of memory.
            let tokens: Vec<(NormalizedToken, StoreTermHashed, usize)> = lexer.collect();
            let term_count = tokens.len();

            // Store scores for each found IID. Results will then be sorted by
            // score before being returned. Scores are basically the sum of
            // Levenshtein distances for each term in the query. Lower score
            // means better result.
            // NOTE: We use `IndexMap` instead of `HashMap` to preserve
            //   insertion order, which correlates to reverse data ingestion
            //   order.
            // NOTE: `capacity = 24` to reduce initial grows.
            let mut scoring_matrix: IndexMap<StoreObjectIID, Vec<Option<QueryMatchScore>>> =
                IndexMap::with_capacity(24usize.min(usize::from(limit)));

            // Look for exact matches.
            'matches: for (idx, (token, term_hash, _)) in tokens.iter().enumerate() {
                let mut iids = kv_action
                    .get_term_to_iids(*term_hash)
                    .unwrap_or(None)
                    .unwrap_or_default();

                // Look for exact matches normalized differently if the Sonic
                // index isn’t normalized.
                if !token.is_special()
                    && self.app_conf.normalization.unicode_normalization.is_none()
                {
                    use unicode_normalization::UnicodeNormalization as _;

                    let mut nfc = kv_action
                        .get_term_to_iids(StoreTermHash::from(
                            token.as_str().nfc().to_string().as_str(),
                        ))
                        .unwrap_or(None)
                        .unwrap_or_default();
                    iids.append(&mut nfc);

                    let mut nfd = kv_action
                        .get_term_to_iids(StoreTermHash::from(
                            token.as_str().nfd().to_string().as_str(),
                        ))
                        .unwrap_or(None)
                        .unwrap_or_default();
                    iids.append(&mut nfd);
                };

                tracing::debug!("got exact search executor iids: {iids:?} for term: {token:?}");

                for iid in iids.into_iter() {
                    // Assign a score of `1` as those are exact matches.
                    let inserted = update_score(&mut scoring_matrix, iid, 1., idx, term_count);

                    if inserted {
                        // Higher limit now reached?
                        // Stop acquiring new suggested IIDs now.
                        if scoring_matrix.len() >= higher_limit {
                            tracing::trace!(?token, "got enough completed results for term");

                            break 'matches;
                        }
                    }
                }
            }

            #[cfg(debug_assertions)]
            tracing::debug!(?scoring_matrix);

            // Look for words containing `term` as prefix.
            if scoring_matrix.len() < higher_limit && alternates_try > 0 && prefix_matching_enabled
            {
                tracing::debug!(
                    "not enough iids were found ({}/{higher_limit}), looking for prefixes",
                    scoring_matrix.len(),
                );

                'terms: for (idx, (token, _, original_len)) in tokens.iter().enumerate() {
                    let Some(suggestions) = fst_action.lookup_begins(token, *original_len) else {
                        tracing::trace!("did not get any completed word for term {token:?}");
                        continue 'terms;
                    };

                    merge_suggestions(
                        suggestions.map(|(w, distance)| (w, prefix_score(distance, *original_len))),
                        &mut scoring_matrix,
                        token,
                        idx,
                        term_count,
                        &kv_action,
                        &mut alternates_try,
                        higher_limit,
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

                'terms: for (idx, (token, _, original_word_len)) in tokens.iter().enumerate() {
                    let term = match token {
                        NormalizedToken::Word(term) => term,
                        // Skip term if it’s special (we want exact matches only).
                        NormalizedToken::Special(term) => {
                            tracing::debug!("skipping fuzzy search for {term:?}: term is special");
                            continue 'terms;
                        }
                    };

                    let max_typo_factor = typo_factor(*original_word_len);
                    let mut typo_factor = 1u32;

                    // TODO: Rework the Levenshtein query feature to avoid repeating
                    //   the same query over and over again. Maybe try to see if
                    //   `fst_levenshtein` can return distances in its response.
                    while alternates_try > 0 && typo_factor <= max_typo_factor {
                        let Some(suggestions) = fst_action.lookup_typos(term, typo_factor) else {
                            tracing::trace!("did not get any completed word for term {term:?}");
                            continue 'terms;
                        };

                        merge_suggestions(
                            suggestions
                                .map(|(w, distance)| (w, typo_score(distance, *original_word_len))),
                            &mut scoring_matrix,
                            term,
                            idx,
                            term_count,
                            &kv_action,
                            &mut alternates_try,
                            higher_limit,
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
            let one_term_is_special = tokens.iter().any(|(token, _, _)| token.is_special());
            if one_term_is_special {
                let mut to_remove = Vec::<StoreObjectIID>::new();

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
            let found_iids = scoring_matrix
                .into_iter()
                .map(|(iid, scores)| (iid, overall_score(&scores)));

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
                if let Ok(Some(oid)) = kv_action.get_iid_to_oid(found_iid) {
                    result_oids.push(oid);
                } else {
                    tracing::error!("failed getting search executor iid-to-oid");
                }
            }

            tracing::info!("got search executor final oids: {:?}", result_oids);

            return Ok(result_oids);
        }

        Err(())
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

// TODO: Use idf to weigh terms.
fn overall_score(scores: &[Option<QueryMatchScore>]) -> QueryResultScore {
    let total = scores
        .into_iter()
        .map(|opt| opt.unwrap_or(0f32))
        .sum::<f32>();
    let count = scores.len() as f32;

    let average = total / count;

    average
}

#[cfg(test)]
#[test]
fn test_overall_score() {
    const MISSING: Option<QueryMatchScore> = None;
    const EXACT_MATCH: Option<QueryMatchScore> = Some(1.);

    // Max score for exact matches.
    assert_eq!(overall_score(&[EXACT_MATCH; 1]), 1.);
    assert_eq!(overall_score(&[EXACT_MATCH; 2]), 1.);
    assert_eq!(overall_score(&[EXACT_MATCH; 3]), 1.);
    assert_eq!(overall_score(&[EXACT_MATCH; 4]), 1.);

    // Lowest score for missing matches.
    assert_eq!(overall_score(&[MISSING; 1]), 0.);
    assert_eq!(overall_score(&[MISSING; 2]), 0.);
    assert_eq!(overall_score(&[MISSING; 3]), 0.);
    assert_eq!(overall_score(&[MISSING; 4]), 0.);

    // Auto-complete > fuzzy matching (not always, but in most cases).
    assert!(overall_score(&[Some(prefix_score(5, 4))]) > overall_score(&[Some(typo_score(1, 10))]));

    // Missing one term.
    assert_eq!(overall_score(&[MISSING, EXACT_MATCH]), 1. / 2.);
    assert_eq!(overall_score(&[MISSING, EXACT_MATCH, EXACT_MATCH]), 2. / 3.);
    // Missing one term is better than missing all terms.
    assert!(overall_score(&[MISSING, EXACT_MATCH]) > overall_score(&[MISSING]));

    // Term order has no meaning.
    assert_eq!(
        overall_score(&[
            EXACT_MATCH,
            Some(prefix_score(2, 3)),
            Some(typo_score(2, 7))
        ]),
        overall_score(&[
            Some(typo_score(2, 7)),
            Some(prefix_score(2, 3)),
            EXACT_MATCH
        ])
    );

    // All typos in one term is like the same total across multiple terms.
    // NOTE: This is not a requirement, it’s just a non-regression test.
    assert_eq!(
        overall_score(&[Some(typo_score(1, 7)); 2]),
        overall_score(&[Some(typo_score(2, 7)), EXACT_MATCH])
    );
    assert_eq!(
        overall_score(&[Some(typo_score(1, 7)); 3]),
        overall_score(&[Some(typo_score(3, 7)), EXACT_MATCH, EXACT_MATCH])
    );

    // Examples for “The brown fox jumps over the lazy dog”:
    // “brown fox jumps”
    assert_eq!(overall_score(&[EXACT_MATCH, EXACT_MATCH, EXACT_MATCH]), 1.);
    // “brown fox jum”
    assert_eq!(
        overall_score(&[EXACT_MATCH, EXACT_MATCH, Some(prefix_score(2, 3))]),
        0.9892473
    );
    // “bron fox jum”
    assert_eq!(
        overall_score(&[
            Some(typo_score(1, 5)),
            EXACT_MATCH,
            Some(prefix_score(2, 3))
        ]),
        0.92258066
    );
    // “brown fox”
    assert_eq!(overall_score(&[EXACT_MATCH, EXACT_MATCH,]), 1.);
    // “brown fox eats”
    assert_eq!(
        overall_score(&[EXACT_MATCH, EXACT_MATCH, MISSING]),
        0.6666667
    ); // 2/3
}

#[allow(clippy::too_many_arguments)] // We’ll refactor this someday, and it’ not public anyway.
fn merge_suggestions(
    suggestions: impl Iterator<Item = (String, QueryMatchScore)>,
    scoring_matrix: &mut IndexMap<StoreObjectIID, Vec<Option<QueryMatchScore>>>,
    term: &String,
    term_idx: usize,
    term_count: usize,
    kv_action: &StoreKVAction<'_>,
    alternates_try: &mut usize,
    higher_limit: usize,
) {
    'suggestions: for (suggested_word, suggestion_score) in suggestions {
        // Do not load base results twice for same term as base term
        if suggested_word.eq(term) {
            continue;
        }

        tracing::trace!(?term, ?suggested_word, "got completed word for term");

        let suggested_term_hash = StoreTermHash::from(&suggested_word);
        let suggested_iids = match kv_action.get_term_to_iids(suggested_term_hash) {
            Ok(Some(suggested_iids)) => suggested_iids,
            Ok(None) => continue,
            Err(_) => continue,
        };

        for suggested_iid in suggested_iids.into_iter().take(*alternates_try) {
            // SAFETY: We can reach at most `alternates_try`.
            *alternates_try = unsafe { alternates_try.unchecked_sub(1) };

            let inserted = update_score(
                scoring_matrix,
                suggested_iid,
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
    scoring_matrix: &mut IndexMap<StoreObjectIID, Vec<Option<QueryMatchScore>>>,
    iid: StoreObjectIID,
    score: QueryMatchScore,
    term_idx: usize,
    term_count: usize,
) -> bool {
    match scoring_matrix.entry(iid) {
        // If entry already exists, use lowest score.
        indexmap::map::Entry::Occupied(mut occupied_entry) => {
            // SAFETY: We always initialize vecs with `term_count` entries.
            let entry_score = unsafe { occupied_entry.get_mut().get_unchecked_mut(term_idx) };

            let new_score = entry_score.map_or(score, |entry_score| score.min(entry_score));

            tracing::trace!(entry_score, new_score, "Updating to min score");
            *entry_score = Some(new_score);

            false
        }
        // If entry does not exist, insert score.
        indexmap::map::Entry::Vacant(vacant_entry) => {
            let mut scores = vec![None; term_count];

            tracing::trace!(new_score = score, "Inserting new score");
            // SAFETY: `scores` has `term_count` elements.
            unsafe { *scores.get_unchecked_mut(term_idx) = Some(score) };

            vacant_entry.insert(scores);

            true
        }
    }
}
