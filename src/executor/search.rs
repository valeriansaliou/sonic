// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use linked_hash_set::LinkedHashSet;
use std::iter::FromIterator;

use crate::lexer::token::TokenLexer;
use crate::query::types::{QuerySearchID, QuerySearchLimit, QuerySearchOffset};
use crate::store::fst::{StoreFSTActionBuilder, StoreFSTPool};
use crate::store::identifiers::{StoreObjectIID, StoreTermHash};
use crate::store::item::StoreItem;
use crate::store::kv::{StoreKVAcquireMode, StoreKVActionBuilder, StoreKVPool};
use crate::APP_CONF;

pub struct ExecutorSearch;

impl ExecutorSearch {
    pub fn execute<'a>(
        store: StoreItem<'a>,
        _event_id: QuerySearchID,
        lexer: TokenLexer<'a>,
        limit: QuerySearchLimit,
        offset: QuerySearchOffset,
    ) -> Result<Option<Vec<String>>, ()> {
        if let StoreItem(collection, Some(bucket), None) = store {
            // Important: acquire database access read lock, and reference it in context. This \
            //   prevents the database from being erased while using it in this block.
            general_kv_access_lock_read!();
            general_fst_access_lock_read!();

            if let (Ok(kv_store), Ok(fst_store)) = (
                StoreKVPool::acquire(StoreKVAcquireMode::OpenOnly, collection),
                StoreFSTPool::acquire(collection, bucket),
            ) {
                // Important: acquire bucket store read lock
                executor_kv_lock_read!(kv_store);

                let (kv_action, fst_action) = (
                    StoreKVActionBuilder::access(bucket, kv_store),
                    StoreFSTActionBuilder::access(fst_store),
                );

                // Try to resolve existing search terms to IIDs, and perform an algebraic AND on \
                //   all resulting IIDs for each given term.
                let mut found_iids: LinkedHashSet<StoreObjectIID> = LinkedHashSet::new();

                'lexing: for (term, term_hashed) in lexer {
                    let mut iids = LinkedHashSet::from_iter(
                        kv_action
                            .get_term_to_iids(term_hashed)
                            .unwrap_or(None)
                            .unwrap_or_default()
                            .into_iter(),
                    );

                    // No IIDs? Try to complete with a suggested alternate word
                    // Notice: this may sound dirty to try generating as many results as the \
                    //   'retain_word_objects' value, but as we do not know if another lexed word \
                    //   comes next we need to exhaust all search space as to intersect it with \
                    //   the (likely) upcoming word.
                    let (higher_limit, alternates_try) = (
                        APP_CONF.store.kv.retain_word_objects,
                        APP_CONF.channel.search.query_alternates_try,
                    );

                    if iids.len() < higher_limit && alternates_try > 0 {
                        debug!(
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
                            fst_action.suggest_words(&term, alternates_try + 1, Some(1))
                        {
                            let mut iids_new_len = iids.len();

                            // This loop will be broken early if we get enough results at some \
                            //   iteration
                            'suggestions: for suggested_word in suggested_words {
                                // Do not load base results twice for same term as base term
                                if suggested_word == term {
                                    continue 'suggestions;
                                }

                                debug!("got completed word: {} for term: {}", suggested_word, term);

                                if let Some(suggested_iids) = kv_action
                                    .get_term_to_iids(StoreTermHash::from(&suggested_word))
                                    .unwrap_or(None)
                                {
                                    for suggested_iid in suggested_iids {
                                        // Do not append the same IID twice (can happen a lot \
                                        //   when completing from suggested results that point \
                                        //   to the same end-OID)
                                        if !iids.contains(&suggested_iid) {
                                            iids.insert(suggested_iid);

                                            iids_new_len += 1;

                                            // Higher limit now reached? Stop acquiring new \
                                            //   suggested IIDs now.
                                            if iids_new_len >= higher_limit {
                                                debug!(
                                                    "got enough completed results for term: {}",
                                                    term
                                                );

                                                break 'suggestions;
                                            }
                                        }
                                    }
                                }
                            }

                            debug!(
                                "done completing results for term: {}, now {} results",
                                term, iids_new_len
                            );
                        } else {
                            debug!("did not get any completed word for term: {}", term);
                        }
                    }

                    debug!("got search executor iids: {:?} for term: {}", iids, term);

                    // Intersect found IIDs with previous batch
                    if found_iids.is_empty() {
                        found_iids = iids;
                    } else {
                        found_iids = found_iids.intersection(&iids).copied().collect();
                    }

                    debug!(
                        "got search executor iid intersection: {:?} for term: {}",
                        found_iids, term
                    );

                    // No IID found? (stop there)
                    if found_iids.is_empty() {
                        info!(
                            "stop search executor as no iid was found in common for term: {}",
                            term
                        );

                        break 'lexing;
                    }
                }

                // Resolve OIDs from IIDs
                // Notice: we also proceed paging from there
                let (limit_usize, offset_usize) = (limit as usize, offset as usize);
                let mut result_oids = Vec::with_capacity(limit_usize);

                'paging: for (index, found_iid) in found_iids.iter().skip(offset_usize).enumerate()
                {
                    // Stop there?
                    if index >= limit_usize {
                        break 'paging;
                    }

                    // Read IID-to-OID for this found IID
                    if let Ok(Some(oid)) = kv_action.get_iid_to_oid(*found_iid) {
                        result_oids.push(oid);
                    } else {
                        error!("failed getting search executor iid-to-oid");
                    }
                }

                info!("got search executor final oids: {:?}", result_oids);

                return Ok(if !result_oids.is_empty() {
                    Some(result_oids)
                } else {
                    None
                });
            }
        }

        Err(())
    }
}
