// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use indexmap::IndexMap;
use std::collections::BTreeMap;
use std::iter::FromIterator;

use crate::lexer::TokenLexer;
use crate::query::{QueryMatchScore, QuerySearchID, QuerySearchLimit, QuerySearchOffset};
use crate::store::StoreItem;
use crate::store::fst::StoreFSTActionBuilder;
use crate::store::identifiers::{StoreObjectIID, StoreTermHash};
use crate::store::kv::{StoreKVAcquireMode, StoreKVActionBuilder};

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

            // Important: acquire bucket store read lock
            executor_kv_lock_read!(kv_store);

            let (kv_action, fst_action) = (
                StoreKVActionBuilder::access(bucket, kv_store),
                StoreFSTActionBuilder::access(fst_store),
            );

            // Store scores for each found IID. Results will then be sorted by
            // score before being returned. Scores are basically the sum of
            // Levenshtein distances for each term in the query. Lower score
            // means better result.
            // NOTE: We use `IndexMap` instead of `HashMap` to preserve
            //   insertion order, which correlates to reverse data ingestion
            //   order.
            // NOTE: `capacity = 24` to avoid initial grows.
            let mut found_iids: IndexMap<StoreObjectIID, (u16, QueryMatchScore)> =
                IndexMap::with_capacity(24.min(usize::from(limit)));

            let mut term_count = 0u16;

            for (term, term_hashed, original_len) in lexer {
                // SAFETY: We’ll never reach 2^32…
                term_count += 1;

                let mut iids: IndexMap<StoreObjectIID, QueryMatchScore> = IndexMap::from_iter(
                    kv_action
                        .get_term_to_iids(term_hashed)
                        .unwrap_or(None)
                        .unwrap_or_default()
                        .into_iter()
                        // Assign a score of `0` as those are exact matches.
                        .map(|k| (k, 0)),
                );

                tracing::debug!(
                    "got exact search executor iids: {:?} for term: {}",
                    iids,
                    term
                );

                // No IIDs? Try to complete with a suggested alternate word
                // Notice: this may sound dirty to try generating as many results as the \
                //   'retain_word_objects' value, but as we do not know if another lexed word \
                //   comes next we need to exhaust all search space as to intersect it with \
                //   the (likely) upcoming word.
                let (higher_limit, alternates_try) = (
                    self.app_conf.store.kv.retain_word_objects,
                    self.app_conf.search.query_alternates_try,
                );

                if iids.len() < higher_limit && alternates_try > 0 {
                    tracing::debug!(
                        "not enough iids were found ({}/{}), completing for term: {}",
                        iids.len(),
                        higher_limit,
                        term
                    );

                    // Suggest N words, in case the first one is found in FST as an exact \
                    //   match of term, we can pick next ones to complete search even further.
                    // Notice: we add '1' to the 'alternates_try' number as to account for \
                    //   exact match suggestion that comes as first result and is to be ignored.
                    if let Some(suggested_words) =
                        fst_action.suggest_words(&term, original_len, alternates_try + 1, None)
                    {
                        let mut iids_new_len = iids.len();

                        // This loop will be broken early if we get enough results at some \
                        //   iteration
                        'suggestions: for (suggested_word, word_score) in suggested_words {
                            // Do not load base results twice for same term as base term
                            if suggested_word == term {
                                continue 'suggestions;
                            }

                            tracing::debug!(
                                "got completed word: {} for term: {}",
                                suggested_word,
                                term
                            );

                            if let Some(suggested_iids) = kv_action
                                .get_term_to_iids(StoreTermHash::from(&suggested_word))
                                .unwrap_or(None)
                            {
                                for suggested_iid in suggested_iids {
                                    // Do not append the same IID twice (can happen a lot \
                                    //   when completing from suggested results that point \
                                    //   to the same end-OID)
                                    if !iids.contains_key(&suggested_iid) {
                                        iids.insert(suggested_iid, word_score);

                                        iids_new_len += 1;

                                        // Higher limit now reached? Stop acquiring new \
                                        //   suggested IIDs now.
                                        if iids_new_len >= higher_limit {
                                            tracing::debug!(
                                                "got enough completed results for term: {}",
                                                term
                                            );

                                            break 'suggestions;
                                        }
                                    }
                                }
                            }
                        }

                        tracing::debug!(
                            "done completing results for term: {}, now {} results",
                            term,
                            iids_new_len
                        );
                    } else {
                        tracing::debug!("did not get any completed word for term: {}", term);
                    }
                }

                tracing::debug!("got search executor iids: {:?} for term: {}", iids, term);

                // Intersect found IIDs with previous batch
                for (iid, new_score) in iids.into_iter() {
                    let (count, score) = found_iids.entry(iid).or_insert((0, 0));
                    *count = count.saturating_add(1);
                    *score = score.saturating_add(new_score);
                }

                tracing::debug!(
                    "got search executor iid intersection: {:?} for term: {}",
                    found_iids,
                    term
                );
            }

            // Update scores to take into account missing matches.
            let found_iids = found_iids.into_iter().map(|(iid, (count, score))| {
                (iid, score + MISSING_MATCH_SCORE * (term_count - count))
            });

            // Sort found IIDs, then flatten.
            let found_iids = sorted_groups(found_iids).flat_map(|(_, v)| v);

            // Resolve OIDs from IIDs
            // Notice: we also proceed paging from there
            let (limit_usize, offset_usize) = (limit as usize, offset as usize);
            let mut result_oids = Vec::with_capacity(limit_usize);

            'paging: for (index, found_iid) in found_iids.skip(offset_usize).enumerate() {
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

fn sorted_groups(
    map: impl ExactSizeIterator<Item = (StoreObjectIID, QueryMatchScore)>,
) -> impl ExactSizeIterator<Item = (QueryMatchScore, Vec<StoreObjectIID>)> {
    let mut btree: BTreeMap<QueryMatchScore, Vec<StoreObjectIID>> = BTreeMap::new();

    for (k, v) in map.into_iter() {
        btree.entry(v).or_default().push(k);
    }

    btree.into_iter()
}
