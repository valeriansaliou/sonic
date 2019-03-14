// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use linked_hash_set::LinkedHashSet;
use std::iter::FromIterator;

use crate::lexer::token::TokenLexer;
use crate::query::types::{QuerySearchID, QuerySearchLimit, QuerySearchOffset};
use crate::store::fst::{StoreFSTActionBuilder, StoreFSTPool, GRAPH_ACCESS_LOCK};
use crate::store::identifiers::{StoreObjectIID, StoreTermHash};
use crate::store::item::StoreItem;
use crate::store::kv::{StoreKVActionBuilder, StoreKVPool, STORE_ACCESS_LOCK};

pub struct ExecutorSearch;

impl ExecutorSearch {
    pub fn execute<'a>(
        store: StoreItem<'a>,
        _event_id: QuerySearchID,
        mut lexer: TokenLexer<'a>,
        limit: QuerySearchLimit,
        offset: QuerySearchOffset,
    ) -> Result<Option<Vec<String>>, ()> {
        if let StoreItem(collection, Some(bucket), None) = store {
            // Important: acquire database access read lock, and reference it in context. This \
            //   prevents the database from being erased while using it in this block.
            let (_kv_access, _fst_access) = (
                STORE_ACCESS_LOCK.read().unwrap(),
                GRAPH_ACCESS_LOCK.read().unwrap(),
            );

            if let (Ok(kv_store), Ok(fst_store)) = (
                StoreKVPool::acquire(collection),
                StoreFSTPool::acquire(collection, bucket),
            ) {
                let (kv_action, fst_action) = (
                    StoreKVActionBuilder::read(bucket, kv_store),
                    StoreFSTActionBuilder::read(fst_store),
                );

                // Try to resolve existing search terms to IIDs, and perform an algebraic AND on \
                //   all resulting IIDs for each given term.
                let mut found_iids: LinkedHashSet<StoreObjectIID> = LinkedHashSet::new();

                while let Some((term, term_hashed)) = lexer.next() {
                    let mut iids = kv_action
                        .get_term_to_iids(term_hashed)
                        .unwrap_or(None)
                        .unwrap_or(Vec::new());

                    // No IIDs? Try to complete with a suggested alternate word
                    if iids.is_empty() == true {
                        debug!("no iid was found, completing for term: {}", term);

                        if let Some(suggested_words) = fst_action.suggest_words(&term, 1) {
                            if let Some(suggested_word) = suggested_words.first() {
                                debug!("got completed word: {} for term: {}", suggested_word, term);

                                if let Some(suggested_iids) = kv_action
                                    .get_term_to_iids(StoreTermHash::from(&suggested_word))
                                    .unwrap_or(None)
                                {
                                    iids.extend(suggested_iids);
                                }
                            }
                        } else {
                            debug!("did not get any completed word for term: {}", term);
                        }
                    }

                    debug!("got search executor iids: {:?} for term: {}", iids, term);

                    // Intersect found IIDs with previous batch
                    let iids_set: LinkedHashSet<StoreObjectIID> =
                        LinkedHashSet::from_iter(iids.into_iter());

                    if found_iids.is_empty() == true {
                        found_iids = iids_set;
                    } else {
                        found_iids = found_iids
                            .intersection(&iids_set)
                            .map(|value| *value)
                            .collect();
                    }

                    debug!(
                        "got search executor iid intersection: {:?} for term: {}",
                        found_iids, term
                    );

                    // No IID found? (stop there)
                    if found_iids.is_empty() == true {
                        info!(
                            "stop search executor as no iid was found in common for term: {}",
                            term
                        );

                        break;
                    }
                }

                // Resolve OIDs from IIDs
                // Notice: we also proceed paging from there
                let mut result_oids = Vec::new();
                let (limit_usize, offset_usize) = (limit as usize, offset as usize);

                for (index, found_iid) in found_iids.iter().skip(offset_usize).enumerate() {
                    // Stop there?
                    if index >= limit_usize {
                        break;
                    }

                    // Read IID-to-OID for this found IID
                    if let Ok(Some(oid)) = kv_action.get_iid_to_oid(*found_iid) {
                        result_oids.push(oid);
                    }
                }

                info!("got search executor final oids: {:?}", result_oids);

                return Ok(if result_oids.is_empty() == false {
                    Some(result_oids)
                } else {
                    None
                });
            }
        }

        Err(())
    }
}
