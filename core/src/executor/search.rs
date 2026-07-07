// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use indexmap::IndexMap;
use std::collections::BTreeMap;

use crate::lexer::TokenLexer;
use crate::query::{QueryMatchScore, QuerySearchID, QuerySearchLimit, QuerySearchOffset};
use crate::store::StoreItem;
use crate::store::fst::{StoreFSTActionBuilder, typo_factor};
use crate::store::identifiers::{StoreObjectIID, StoreTermHash, StoreTermHashed};
use crate::store::kv::{StoreKVAcquireMode, StoreKVAction, StoreKVActionBuilder};

const MISSING_MATCH_SCORE: u16 = 100;

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
            let terms: Vec<(String, StoreTermHashed, usize)> = lexer.collect();
            let term_count = terms.len();

            // Store scores for each found IID. Results will then be sorted by
            // score before being returned. Scores are basically the sum of
            // Levenshtein distances for each term in the query. Lower score
            // means better result.
            // NOTE: We use `IndexMap` instead of `HashMap` to preserve
            //   insertion order, which correlates to reverse data ingestion
            //   order.
            // NOTE: `capacity = 24` to reduce initial grows.
            let mut scoring_matrix: IndexMap<StoreObjectIID, Vec<QueryMatchScore>> =
                IndexMap::with_capacity(24usize.min(usize::from(limit)));

            // Look for exact matches.
            'matches: for (idx, (term, term_hash, _)) in terms.iter().enumerate() {
                let iids = kv_action
                    .get_term_to_iids(*term_hash)
                    .unwrap_or(None)
                    .unwrap_or_default();

                tracing::debug!("got exact search executor iids: {iids:?} for term: {term:?}");

                for iid in iids.into_iter() {
                    // Assign a score of `0` as those are exact matches.
                    let inserted = update_score(&mut scoring_matrix, iid, 0, idx, term_count);

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
            if scoring_matrix.len() < higher_limit && alternates_try > 0 && prefix_matching_enabled
            {
                tracing::debug!(
                    "not enough iids were found ({}/{higher_limit}), looking for prefixes",
                    scoring_matrix.len(),
                );

                'terms: for (idx, (term, _, original_len)) in terms.iter().enumerate() {
                    // Skip term if it’s an ID (we want exact matches only).
                    // FIXME: We’d like to enable prefix matching for IDs, but
                    //   the way the tokenizer works causes false positives.
                    if is_considered_id(&term) {
                        tracing::debug!(
                            "skipping prefix search for {term:?}: term is considered an ID"
                        );
                        continue 'terms;
                    };

                    let Some(suggestions) = fst_action.lookup_begins(term, *original_len) else {
                        tracing::trace!("did not get any completed word for term {term:?}");
                        continue 'terms;
                    };

                    merge_suggestions(
                        suggestions,
                        &mut scoring_matrix,
                        term,
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

                'terms: for (idx, (term, _, original_word_len)) in terms.iter().enumerate() {
                    // Skip term if it’s an ID (we want exact matches only).
                    if is_considered_id(&term) {
                        tracing::debug!(
                            "skipping fuzzy matching for {term:?}: term is considered an ID"
                        );
                        continue 'terms;
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
                            suggestions,
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

            // Switch to implicit `AND` if query contains an ID.
            // NOTE: When a user queries for an ID (e.g. UUID), they expect only
            //   exact matches to be returned. If one term is considered an ID,
            //   we drop all results missing at least one term. It’s not the
            //   most efficient (compared to not storing the result in the first
            //   place) but it’s an edge case and the cost is negligible.
            let one_term_is_an_id = terms.iter().any(|(term, _, _)| is_considered_id(term));
            if one_term_is_an_id {
                let mut to_remove = Vec::<StoreObjectIID>::new();

                for (&iid, scores) in scoring_matrix.iter() {
                    if scores.contains(&MISSING_MATCH_SCORE) {
                        to_remove.push(iid);
                    }
                }

                for iid in to_remove {
                    scoring_matrix.swap_remove(&iid);
                }
            }

            // Flatten scores, taking into account missing matches (thanks to
            // `MISSING_MATCH_SCORE`).
            let found_iids = scoring_matrix
                .into_iter()
                .map(|(iid, scores)| (iid, scores.into_iter().sum()));

            // Sort found IIDs, then flatten.
            let all_iids = sorted_groups(found_iids).flat_map(|(_, v)| v);

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

fn is_considered_id(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_digit() || matches!(c, '@'))
        || s.chars()
            // Skip first char.
            .skip(1)
            // Skip last char.
            .take(s.len().saturating_sub(2))
            .any(is_non_prose_char)
}

fn is_non_prose_char(c: char) -> bool {
    matches!(c, '.' | '_' | ':' | '\\' | '+' | '=' | '&')
}

/// Note that the tokenizer might split the tested strings in ways that make
/// Sonic as a whole behave differently! This is just a simple unit test.
#[cfg(test)]
#[test]
fn test_is_considered_id() {
    // Sanity check.
    assert!(!is_considered_id("Hello"));

    // Phone number like.
    assert!(is_considered_id("1234-567890-12"));
    assert!(is_considered_id("0123456789"));

    // UUID like.
    assert!(is_considered_id("6db14cb4-b82e-4e49-8016-ef76c4290a2f"));

    // Hash like.
    assert!(is_considered_id("b244423d417369795292e9f4530d0c0e6fa07625"));
    assert!(is_considered_id("b244423"));

    // Code like.
    assert!(is_considered_id("is_considered_id"));

    // Domain name.
    assert!(is_considered_id("example.org"));
    assert!(!is_considered_id("endofsentence."));
    assert!(!is_considered_id(".startofsentence"));

    // URL.
    assert!(is_considered_id("https://example.org/foo?id=123"));

    // Email address.
    assert!(is_considered_id("alice@example.org"));
    assert!(is_considered_id("alice+foo@example.org"));

    // IP addresses.
    assert!(is_considered_id("192.168.1.0"));
    assert!(is_considered_id("0.0.0.0"));
    assert!(is_considered_id("2606:4700::6812:1c68"));
    assert!(is_considered_id("::1"));
    assert!(!is_considered_id("example:"));

    // Username.
    assert!(is_considered_id("@example"));
}

#[allow(clippy::too_many_arguments)] // We’ll refactor this someday, and it’ not public anyway.
fn merge_suggestions(
    suggestions: impl Iterator<Item = (String, QueryMatchScore)>,
    scoring_matrix: &mut IndexMap<StoreObjectIID, Vec<QueryMatchScore>>,
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
    scoring_matrix: &mut IndexMap<StoreObjectIID, Vec<QueryMatchScore>>,
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

            let new_score = score.min(*entry_score);

            tracing::trace!(entry_score, new_score, "Updating to min score");
            *entry_score = new_score;

            false
        }
        // If entry does not exist, insert score.
        indexmap::map::Entry::Vacant(vacant_entry) => {
            let mut scores = vec![MISSING_MATCH_SCORE; term_count];

            tracing::trace!(new_score = score, "Inserting new score");
            // SAFETY: `scores` has `term_count` elements.
            unsafe { *scores.get_unchecked_mut(term_idx) = score };

            vacant_entry.insert(scores);

            true
        }
    }
}

fn sorted_groups(
    map: impl ExactSizeIterator<Item = (StoreObjectIID, QueryMatchScore)>,
) -> impl ExactSizeIterator<Item = (QueryMatchScore, Vec<StoreObjectIID>)> {
    let mut btree: BTreeMap<QueryMatchScore, Vec<StoreObjectIID>> = BTreeMap::new();

    for (k, v) in map.into_iter() {
        btree.entry(v).or_default().push(k);
    }

    btree.into_iter()
}
