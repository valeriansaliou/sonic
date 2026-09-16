// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use linked_hash_set::LinkedHashSet;
use rocksdb::WriteBatch;
use std::iter::FromIterator;

use crate::lexer::itertools::UniqueBy;
use crate::lexer::preprocessor::{PreprocessorOutput, Token};
use crate::store::StoreItem;
use crate::store::identifiers::StoreTermHash;
use crate::store::kv::StoreKVAcquireMode;
use crate::util::hash::NoopU32HasherBuilder;

impl super::Executor {
    pub fn pop(&self, item: StoreItem, input: PreprocessorOutput) -> Result<u32, ()> {
        if let StoreItem(collection, Some(bucket), Some(object)) = item {
            // Important: acquire database access read lock, and reference it in context. This \
            //   prevents the database from being erased while using it in this block.
            let _kv_read_guard = self.kv_pool.lock_read_access();
            let _fst_read_guard = self.fst_pool.lock_read_access();

            if let (Ok(kv_store), Ok(fst_store)) = (
                self.kv_pool
                    .acquire(StoreKVAcquireMode::OpenOnly, collection, None, |_| {}),
                self.fst_pool.acquire(collection, bucket),
            ) {
                let Some(kv_store) = kv_store else {
                    tracing::debug!(
                        "collection store does not exist, consider {bucket:?} from {collection:?} empty"
                    );
                    return Ok(0);
                };

                // Important: acquire bucket store write lock
                executor_kv_lock_write!(kv_store);

                let kv_action = kv_store.access_read_write(bucket);

                // Try to resolve existing OID to IID (if it does not exist, there is nothing to \
                //   be flushed)
                let oid = object.as_str();

                if let Ok(iid_value) = kv_action.get_oid_to_iid(oid) {
                    let mut count_popped = 0;

                    if let Some(iid) = iid_value {
                        // Try to resolve existing search terms from IID, and perform an algebraic \
                        //   AND on all popped terms to generate a list of terms to be cleaned up.
                        if let Ok(Some(iid_terms_hashes_vec)) = kv_action.get_iid_to_terms(iid) {
                            tracing::info!(
                                "got pop executor stored iid-to-terms: {:?}",
                                iid_terms_hashes_vec
                            );

                            let iid_terms_hashes: LinkedHashSet<StoreTermHash> =
                                LinkedHashSet::from_iter(iid_terms_hashes_vec.iter().copied());

                            let remaining_terms: LinkedHashSet<StoreTermHash> = iid_terms_hashes
                                .difference(&LinkedHashSet::from_iter(
                                    input.tokens().map(Token::into_hash),
                                ))
                                .copied()
                                .collect();

                            tracing::debug!(
                                "got pop executor terms remaining terms: {:?} for iid: {}",
                                remaining_terms,
                                iid
                            );

                            count_popped = (iid_terms_hashes.len() - remaining_terms.len()) as u32;

                            if count_popped > 0 {
                                let mut batch = WriteBatch::default();

                                if remaining_terms.is_empty() {
                                    tracing::info!("nuke whole bucket for pop executor");

                                    // Flush bucket (batch operation, as it is shared w/ other \
                                    //   executors)
                                    kv_action.batch_flush_bucket(
                                        &mut batch,
                                        iid,
                                        oid,
                                        &iid_terms_hashes_vec,
                                    );
                                } else {
                                    tracing::info!("nuke only certain terms for pop executor");

                                    let tokens = UniqueBy::new_with_hasher(
                                        input.tokens(),
                                        Token::hash,
                                        NoopU32HasherBuilder,
                                    );

                                    // Nuke IID in Term-to-IIDs list
                                    for token in tokens {
                                        let (pop_term, pop_term_hash) =
                                            (token.as_normalized(), token.hash());

                                        // Check that term is linked to IID (and should be removed)
                                        if iid_terms_hashes.contains(&pop_term_hash) {
                                            if let Ok(Some(mut pop_term_iids)) =
                                                kv_action.get_term_to_iids(pop_term_hash)
                                            {
                                                // Remove IID from list of IIDs to be popped
                                                pop_term_iids.retain(|cur_iid| cur_iid != &iid);

                                                if pop_term_iids.is_empty() {
                                                    // IIDs list was empty, delete whole key
                                                    kv_action.delete_term_to_iids(
                                                        &mut batch,
                                                        pop_term_hash,
                                                    );

                                                    // Pop from FST graph (does not exist anymore)
                                                    if fst_store.pop_word(pop_term) {
                                                        tracing::debug!(
                                                            "pop term hash nuked from graph: {:?}",
                                                            pop_term_hash
                                                        );
                                                    }
                                                } else {
                                                    // Re-build IIDs list w/o current IID
                                                    kv_action.set_term_to_iids(
                                                        &mut batch,
                                                        pop_term_hash,
                                                        pop_term_iids.into_iter(),
                                                    );
                                                }
                                            } else {
                                                tracing::error!(
                                                    "failed getting term-to-iids in pop executor"
                                                );
                                            }
                                        }
                                    }

                                    // Bump IID-to-Terms list
                                    let remaining_terms_vec: Vec<StoreTermHash> =
                                        Vec::from_iter(remaining_terms);

                                    kv_action.set_iid_to_terms(
                                        &mut batch,
                                        iid,
                                        remaining_terms_vec.into_iter(),
                                    );
                                }

                                executor_ensure_op!(kv_action.write(batch));
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
