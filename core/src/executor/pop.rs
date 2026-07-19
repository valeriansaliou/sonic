// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use linked_hash_set::LinkedHashSet;
use std::iter::FromIterator;

use crate::lexer::TokenLexer;
use crate::store::StoreItem;
use crate::store::fst::StoreFSTActionBuilder;
use crate::store::kv::{StoreKVAcquireMode, StoreKVActionBuilder};

impl super::Executor {
    pub fn pop(&self, item: StoreItem, lexer: TokenLexer) -> Result<u32, ()> {
        if let StoreItem(collection, Some(bucket), Some(object)) = item {
            // Important: acquire database access read lock, and reference it in context. This \
            //   prevents the database from being erased while using it in this block.
            let _kv_read_guard = self.kv_pool.lock_read_access();
            let _fst_read_guard = self.fst_pool.lock_read_access();

            if let Ok(kv_store) = self
                .kv_pool
                .acquire(StoreKVAcquireMode::OpenOnly, collection)
            {
                // Important: acquire bucket store write lock
                executor_kv_lock_write!(kv_store);

                let kv_action = StoreKVActionBuilder::access(bucket, kv_store);
                let Some(bucket_id) = kv_action.bucket_id() else {
                    return Ok(0);
                };
                let fst_store = self.fst_pool.acquire(collection, bucket_id)?;
                let fst_action = StoreFSTActionBuilder::access(fst_store);

                // Try to resolve existing OID to IID (if it does not exist, there is nothing to \
                //   be flushed)
                let oid = object.as_str();

                if let Ok(iid_value) = kv_action.get_oid_to_iid(oid) {
                    let mut count_popped = 0;

                    if let Some(iid) = iid_value {
                        if kv_action.get_document_by_iid(iid)?.is_some() {
                            tracing::error!("cannot POP an OID managed by UPSERT");
                            return Err(());
                        }
                        // Try to resolve existing search terms from IID, and perform an algebraic \
                        //   AND on all popped terms to generate a list of terms to be cleaned up.
                        if let Ok(Some(iid_terms_hashed_vec)) = kv_action.get_iid_to_terms(iid) {
                            tracing::info!(
                                "got pop executor stored iid-to-terms: {:?}",
                                iid_terms_hashed_vec
                            );

                            let pop_terms = lexer
                                .map(|(token, _len)| token.into_inner())
                                .collect::<Vec<_>>();
                            let iid_terms: LinkedHashSet<String> =
                                LinkedHashSet::from_iter(iid_terms_hashed_vec.iter().cloned());
                            let pop_term_set: LinkedHashSet<String> =
                                LinkedHashSet::from_iter(pop_terms.iter().cloned());
                            let remaining_terms: LinkedHashSet<String> =
                                iid_terms.difference(&pop_term_set).cloned().collect();

                            tracing::debug!(
                                "got pop executor terms remaining terms: {:?} for iid: {}",
                                remaining_terms,
                                iid
                            );

                            count_popped = (iid_terms.len() - remaining_terms.len()) as u32;

                            if count_popped > 0 {
                                if remaining_terms.is_empty() {
                                    tracing::info!("nuke whole bucket for pop executor");

                                    // Flush bucket (batch operation, as it is shared w/ other \
                                    //   executors)
                                    executor_ensure_op!(kv_action.batch_flush_bucket(
                                        iid,
                                        oid,
                                        &iid_terms_hashed_vec
                                    ));
                                    for term in &pop_terms {
                                        match kv_action.get_term_frequency(term) {
                                            Ok(0) => {
                                                fst_action.pop_word(term);
                                            }
                                            Ok(frequency) => {
                                                fst_action.push_word(
                                                    term,
                                                    frequency,
                                                    &self.app_conf.store.fst,
                                                );
                                            }
                                            Err(()) => return Err(()),
                                        }
                                    }
                                } else {
                                    tracing::info!("nuke only certain terms for pop executor");

                                    let remaining_terms_vec = Vec::from_iter(remaining_terms);
                                    let removed_terms = pop_terms
                                        .iter()
                                        .filter(|term| iid_terms.contains(*term))
                                        .cloned()
                                        .collect::<Vec<_>>();
                                    let frequencies = kv_action.batch_remove_iid_terms(
                                        iid,
                                        &remaining_terms_vec,
                                        &removed_terms,
                                    )?;
                                    for (term, frequency) in frequencies {
                                        if frequency == 0 {
                                            fst_action.pop_word(&term);
                                        } else {
                                            fst_action.push_word(
                                                &term,
                                                frequency,
                                                &self.app_conf.store.fst,
                                            );
                                        }
                                    }
                                }
                            }
                        } else {
                            tracing::error!("failed getting iid-to-terms in pop executor");
                        }
                    }

                    return Ok(count_popped);
                }
            }
        }

        Err(())
    }
}
