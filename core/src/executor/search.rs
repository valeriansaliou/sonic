// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use indexmap::IndexMap;

use crate::lexer::{NormalizedToken, TokenLexer};
use crate::query::{
    QueryMatchScore, QuerySearchID, QuerySearchLimit, QuerySearchOffset, QueryTimeRange,
};
use crate::store::StoreItem;
use crate::store::document::StoreDocument;
use crate::store::fst::{StoreFSTActionBuilder, typo_factor};
use crate::store::identifiers::StoreObjectIID;
use crate::store::kv::{StoreKVAcquireMode, StoreKVAction, StoreKVActionBuilder};

const MISSING_MATCH_SCORE: u16 = 100;

impl super::Executor {
    pub fn search(
        &self,
        item: StoreItem,
        event_id: QuerySearchID,
        lexer: TokenLexer,
        limit: QuerySearchLimit,
        offset: QuerySearchOffset,
    ) -> Result<Vec<String>, ()> {
        self.search_with_range(item, event_id, lexer, limit, offset, None)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn search_with_range(
        &self,
        item: StoreItem,
        _event_id: QuerySearchID,
        lexer: TokenLexer,
        limit: QuerySearchLimit,
        offset: QuerySearchOffset,
        time_range: Option<QueryTimeRange>,
    ) -> Result<Vec<String>, ()> {
        if let StoreItem(collection, Some(bucket), None) = item {
            // Important: acquire database access read lock, and reference it in context. This \
            //   prevents the database from being erased while using it in this block.
            let _kv_read_guard = self.kv_pool.lock_read_access();
            let _fst_read_guard = self.fst_pool.lock_read_access();

            let Ok(kv_store) = self
                .kv_pool
                .acquire(StoreKVAcquireMode::OpenOnly, collection)
            else {
                return Err(());
            };

            let page_end = usize::try_from(offset)
                .unwrap_or(usize::MAX)
                .saturating_add(usize::from(limit));
            let (higher_limit, mut alternates_try) = (
                self.app_conf.search.query_candidates_maximum.max(page_end),
                self.app_conf.search.query_alternates_try,
            );

            let fuzzy_matching_enabled = self.fst_pool.fst_action_config.fuzzy_matching_enabled;

            // Important: acquire bucket store read lock
            executor_kv_lock_read!(kv_store);

            let kv_action = StoreKVActionBuilder::access(bucket, kv_store);
            let Some(bucket_id) = kv_action.bucket_id() else {
                return Ok(Vec::new());
            };
            let fst_store = self.fst_pool.acquire(collection, bucket_id)?;
            let fst_action = StoreFSTActionBuilder::access(fst_store);

            // Collect all terms so we know the count right ahead.
            // PERF: This helps allocating the correct amounts of memory.
            let tokens: Vec<(NormalizedToken, usize)> = lexer.collect();
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
            for (idx, (token, _)) in tokens.iter().enumerate() {
                let iids = if let Some(range) = time_range {
                    kv_action
                        .get_term_iids_in_time_range(
                            token,
                            range.from_ms,
                            range.to_ms,
                            higher_limit,
                        )
                        .unwrap_or_default()
                } else {
                    kv_action
                        .get_term_iids_desc(token, higher_limit)
                        .unwrap_or_default()
                };

                tracing::debug!("got exact search executor iids: {iids:?} for term: {token:?}");

                for iid in iids.into_iter() {
                    if scoring_matrix.len() >= higher_limit && !scoring_matrix.contains_key(&iid) {
                        continue;
                    }
                    // Assign a score of `0` as those are exact matches.
                    update_score(&mut scoring_matrix, iid, 0, idx, term_count);
                }
            }

            #[cfg(debug_assertions)]
            tracing::debug!(?scoring_matrix);

            // Complete partial terms from the adaptive corpus lexicon.
            if scoring_matrix.len() < higher_limit && alternates_try > 0 {
                'terms: for (idx, (token, original_len)) in tokens.iter().enumerate() {
                    let Some(completions) = fst_action.lookup_begins(token, *original_len) else {
                        continue 'terms;
                    };

                    merge_suggestions(
                        completions,
                        &mut scoring_matrix,
                        token,
                        idx,
                        term_count,
                        &kv_action,
                        &mut alternates_try,
                        higher_limit,
                        time_range,
                    );
                }
            }

            // Look for words like `term` (fuzzy matching).
            if scoring_matrix.len() < higher_limit && alternates_try > 0 && fuzzy_matching_enabled {
                tracing::debug!(
                    "not enough iids were found ({}/{higher_limit}), looking for fuzzy matches",
                    scoring_matrix.len(),
                );

                'terms: for (idx, (token, original_word_len)) in tokens.iter().enumerate() {
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
                            time_range,
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
            let one_term_is_special = tokens.iter().any(|(token, _)| token.is_special());
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
            let mut found_iids = scoring_matrix
                .into_iter()
                .map(|(iid, scores)| {
                    Ok((
                        iid,
                        scores.into_iter().sum::<QueryMatchScore>(),
                        if time_range.is_some() {
                            kv_action.get_iid_timestamp(iid)?.unwrap_or(0)
                        } else {
                            0
                        },
                    ))
                })
                .collect::<Result<Vec<_>, ()>>()?;
            found_iids.sort_by(|left, right| {
                left.1
                    .cmp(&right.1)
                    .then_with(|| right.2.cmp(&left.2))
                    .then_with(|| {
                        if time_range.is_some() {
                            right.0.cmp(&left.0)
                        } else {
                            std::cmp::Ordering::Equal
                        }
                    })
            });

            // Resolve OIDs from IIDs
            // Notice: we also proceed paging from there
            let (limit_usize, offset_usize) = (limit as usize, offset as usize);
            let mut result_oids = Vec::with_capacity(limit_usize);

            'paging: for (index, (found_iid, _, _)) in
                found_iids.into_iter().skip(offset_usize).enumerate()
            {
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

    #[allow(clippy::too_many_arguments)]
    pub fn search_documents(
        &self,
        item: StoreItem,
        event_id: QuerySearchID,
        lexer: TokenLexer,
        limit: QuerySearchLimit,
        offset: QuerySearchOffset,
        time_range: Option<QueryTimeRange>,
    ) -> Result<Vec<StoreDocument>, ()> {
        let collection = item.0;
        let bucket = item.1.ok_or(())?;
        let oids = self.search_with_range(item, event_id, lexer, limit, offset, time_range)?;
        let _guard = self.kv_pool.lock_read_access();
        let store = self
            .kv_pool
            .acquire(StoreKVAcquireMode::OpenOnly, collection)?;
        executor_kv_lock_read!(store);
        let action = StoreKVActionBuilder::access(bucket, store);
        let mut documents = Vec::with_capacity(oids.len());
        for oid in oids {
            if let Some(document) = action.get_document(&oid)? {
                documents.push(document);
            }
        }
        Ok(documents)
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
    time_range: Option<QueryTimeRange>,
) {
    'suggestions: for (suggested_word, suggestion_score) in suggestions {
        if *alternates_try == 0 {
            break;
        }

        // Do not load base results twice for same term as base term
        if suggested_word.eq(term) {
            continue;
        }

        tracing::trace!(?term, ?suggested_word, "got completed word for term");

        let suggested_iids = match time_range.map_or_else(
            || kv_action.get_term_iids_desc(&suggested_word, higher_limit),
            |range| {
                kv_action.get_term_iids_in_time_range(
                    &suggested_word,
                    range.from_ms,
                    range.to_ms,
                    higher_limit,
                )
            },
        ) {
            Ok(suggested_iids) => suggested_iids,
            Err(_) => continue,
        };
        *alternates_try -= 1;

        for suggested_iid in suggested_iids {
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
