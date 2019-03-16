// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use linked_hash_set::LinkedHashSet;
use std::iter::FromIterator;

use crate::lexer::token::TokenLexer;
use crate::store::fst::{StoreFSTActionBuilder, StoreFSTPool};
use crate::store::identifiers::{StoreMetaKey, StoreMetaValue, StoreTermHashed};
use crate::store::item::StoreItem;
use crate::store::kv::{StoreKVAcquireMode, StoreKVActionBuilder, StoreKVPool};

pub struct ExecutorPush;

impl ExecutorPush {
    pub fn execute<'a>(store: StoreItem<'a>, mut lexer: TokenLexer<'a>) -> Result<(), ()> {
        if let StoreItem(collection, Some(bucket), Some(object)) = store {
            // Important: acquire database access read lock, and reference it in context. This \
            //   prevents the database from being erased while using it in this block.
            general_kv_access_lock_read!();
            general_fst_access_lock_read!();

            if let (Ok(kv_store), Ok(fst_store)) = (
                StoreKVPool::acquire(StoreKVAcquireMode::Any, collection, bucket),
                StoreFSTPool::acquire(collection, bucket),
            ) {
                // Important: acquire bucket store write lock
                executor_kv_lock_write!(kv_store);

                let (kv_action, fst_action) = (
                    StoreKVActionBuilder::access(kv_store),
                    StoreFSTActionBuilder::access(fst_store),
                );

                // Try to resolve existing OID to IID, otherwise initialize IID (store the \
                //   bi-directional relationship)
                let oid = object.as_str().to_owned();
                let iid = kv_action.get_oid_to_iid(&oid).unwrap_or(None).or_else(|| {
                    info!("must initialize push executor oid-to-iid and iid-to-oid");

                    if let Ok(iid_incr) = kv_action.get_meta_to_value(StoreMetaKey::IIDIncr) {
                        let iid_incr = if let Some(iid_incr) = iid_incr {
                            match iid_incr {
                                StoreMetaValue::IIDIncr(iid_incr) => iid_incr + 1,
                            }
                        } else {
                            0
                        };

                        // Bump last stored increment
                        if kv_action
                            .set_meta_to_value(
                                StoreMetaKey::IIDIncr,
                                StoreMetaValue::IIDIncr(iid_incr),
                            )
                            .is_ok()
                            == true
                        {
                            // Associate OID <> IID (bidirectional)
                            kv_action.set_oid_to_iid(&oid, iid_incr).ok();
                            kv_action.set_iid_to_oid(iid_incr, &oid).ok();

                            Some(iid_incr)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                });

                if let Some(iid) = iid {
                    let mut has_commits = false;

                    // Acquire list of terms for IID
                    let mut iid_terms_hashed: LinkedHashSet<StoreTermHashed> =
                        LinkedHashSet::from_iter(
                            kv_action
                                .get_iid_to_terms(iid)
                                .unwrap_or(None)
                                .unwrap_or(Vec::new()),
                        );

                    info!(
                        "got push executor stored iid-to-terms: {:?}",
                        iid_terms_hashed
                    );

                    while let Some((term, term_hashed)) = lexer.next() {
                        // Check that term is not already linked to IID
                        if iid_terms_hashed.contains(&term_hashed) == false {
                            if let Ok(term_iids) = kv_action.get_term_to_iids(term_hashed) {
                                has_commits = true;

                                // Add IID in first position in list for terms
                                let mut term_iids = term_iids.unwrap_or(Vec::new());

                                // Remove IID from list of IIDs to be popped before inserting in \
                                //   first position?
                                if term_iids.contains(&iid) == true {
                                    term_iids.retain(|cur_iid| cur_iid != &iid);
                                }

                                info!("has push executor term-to-iids: {}", iid);

                                term_iids.insert(0, iid);

                                kv_action.set_term_to_iids(term_hashed, &term_iids).ok();

                                // Insert term into IID to terms map
                                iid_terms_hashed.insert(term_hashed);
                            }
                        }

                        // Push to FST graph? (this consumes the term; to avoid sub-clones)
                        if fst_action.push_word(&term) == true {
                            debug!("push term commited to graph: {}", term);
                        }
                    }

                    // Commit updated list of terms for IID? (if any commit made)
                    if has_commits == true {
                        let collected_iids: Vec<StoreTermHashed> =
                            iid_terms_hashed.into_iter().collect();

                        info!(
                            "has push executor iid-to-terms commits: {:?}",
                            collected_iids
                        );

                        kv_action.set_iid_to_terms(iid, &collected_iids).ok();
                    }

                    return Ok(());
                }
            }
        }

        Err(())
    }
}
