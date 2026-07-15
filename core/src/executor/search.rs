// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use indexmap::IndexMap;
use std::collections::BTreeMap;

use crate::lexer::{NormalizedToken, TokenLexer};
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
            let mut scoring_matrix: IndexMap<StoreObjectIID, Vec<QueryMatchScore>> =
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
                    // Assign a score of `0` as those are exact matches.
                    let inserted = update_score(&mut scoring_matrix, iid, 0, idx, term_count);

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
                        suggestions,
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
