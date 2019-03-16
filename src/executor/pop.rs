// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use linked_hash_set::LinkedHashSet;
use std::iter::FromIterator;

use crate::lexer::token::TokenLexer;
use crate::store::fst::{StoreFSTActionBuilder, StoreFSTPool};
use crate::store::identifiers::StoreTermHashed;
use crate::store::item::StoreItem;
use crate::store::kv::{StoreKVAcquireMode, StoreKVActionBuilder, StoreKVPool};

pub struct ExecutorPop;

impl ExecutorPop {
    pub fn execute<'a>(store: StoreItem<'a>, lexer: TokenLexer<'a>) -> Result<u32, ()> {
        if let StoreItem(collection, Some(bucket), Some(object)) = store {
            // Important: acquire database access read lock, and reference it in context. This \
            //   prevents the database from being erased while using it in this block.
            general_kv_access_lock_read!();
            general_fst_access_lock_read!();

            if let (Ok(kv_store), Ok(fst_store)) = (
                StoreKVPool::acquire(StoreKVAcquireMode::OpenOnly, collection, bucket),
                StoreFSTPool::acquire(collection, bucket),
            ) {
                // Important: acquire bucket store write lock
                executor_kv_lock_write!(kv_store);

                let (kv_action, fst_action) = (
                    StoreKVActionBuilder::access(kv_store),
                    StoreFSTActionBuilder::access(fst_store),
                );

                // Try to resolve existing OID to IID (if it does not exist, there is nothing to \
                //   be flushed)
                let oid = object.as_str().to_owned();

                if let Ok(iid_value) = kv_action.get_oid_to_iid(&oid) {
                    let mut count_popped = 0;

                    if let Some(iid) = iid_value {
                        // Try to resolve existing search terms from IID, and perform an algebraic \
                        //   AND on all popped terms to generate a list of terms to be cleaned up.
                        if let Ok(Some(iid_terms_hashed_vec)) = kv_action.get_iid_to_terms(iid) {
                            info!(
                                "got pop executor stored iid-to-terms: {:?}",
                                iid_terms_hashed_vec
                            );

                            let pop_terms: Vec<(String, StoreTermHashed)> = lexer.collect();

                            let iid_terms_hashed: LinkedHashSet<StoreTermHashed> =
                                LinkedHashSet::from_iter(
                                    iid_terms_hashed_vec.iter().map(|value| *value),
                                );

                            let remaining_terms: LinkedHashSet<StoreTermHashed> = iid_terms_hashed
                                .difference(&LinkedHashSet::from_iter(
                                    pop_terms.iter().map(|item| item.1),
                                ))
                                .map(|value| *value)
                                .collect();

                            debug!(
                                "got pop executor terms remaining terms: {:?} for iid: {}",
                                remaining_terms, iid
                            );

                            count_popped = (iid_terms_hashed.len() - remaining_terms.len()) as u32;

                            if count_popped > 0 {
                                if remaining_terms.len() == 0 {
                                    info!("nuke whole bucket for pop executor");

                                    // Flush bucket (batch operation, as it is shared w/ other \
                                    //   executors)
                                    executor_ensure_op!(kv_action.batch_flush_bucket(
                                        iid,
                                        &oid,
                                        &iid_terms_hashed_vec
                                    ));
                                } else {
                                    info!("nuke only certain terms for pop executor");

                                    // Nuke IID in Term-to-IIDs list
                                    for (pop_term, pop_term_hashed) in &pop_terms {
                                        // Check that term is linked to IID (and should be removed)
                                        if iid_terms_hashed.contains(pop_term_hashed) == true {
                                            if let Ok(Some(mut pop_term_iids)) =
                                                kv_action.get_term_to_iids(*pop_term_hashed)
                                            {
                                                // Remove IID from list of IIDs to be popped
                                                pop_term_iids.retain(|cur_iid| cur_iid != &iid);

                                                if pop_term_iids.is_empty() == true {
                                                    // IIDs list was empty, delete whole key
                                                    executor_ensure_op!(kv_action
                                                        .delete_term_to_iids(*pop_term_hashed));

                                                    // Pop from FST graph (does not exist anymore)
                                                    if fst_action.pop_word(pop_term) == true {
                                                        debug!(
                                                            "pop term hash nuked from graph: {}",
                                                            pop_term_hashed
                                                        );
                                                    }
                                                } else {
                                                    // Re-build IIDs list w/o current IID
                                                    executor_ensure_op!(kv_action
                                                        .set_term_to_iids(
                                                            *pop_term_hashed,
                                                            &pop_term_iids,
                                                        ));
                                                }
                                            } else {
                                                error!(
                                                    "failed getting term-to-iids in pop executor"
                                                );
                                            }
                                        }
                                    }

                                    // Bump IID-to-Terms list
                                    let remaining_terms_vec: Vec<StoreTermHashed> =
                                        Vec::from_iter(remaining_terms.into_iter());

                                    executor_ensure_op!(
                                        kv_action.set_iid_to_terms(iid, &remaining_terms_vec)
                                    );
                                }
                            }
                        } else {
                            error!("failed getting iid-to-terms in pop executor");
                        }
                    }

                    return Ok(count_popped);
                }
            }
        }

        Err(())
    }
}
