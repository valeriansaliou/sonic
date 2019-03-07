// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use linked_hash_set::LinkedHashSet;
use std::iter::FromIterator;

use crate::lexer::token::TokenLexer;
use crate::store::identifiers::{StoreMetaKey, StoreMetaValue, StoreTermHashed};
use crate::store::item::StoreItem;
use crate::store::kv::{StoreKVActionBuilder, StoreKVPool, STORE_ACCESS_LOCK};

pub struct ExecutorPush;

impl ExecutorPush {
    pub fn execute<'a>(store: StoreItem<'a>, mut lexer: TokenLexer<'a>) -> Result<(), ()> {
        if let StoreItem(collection, Some(bucket), Some(object)) = store {
            // Important: acquire database access read lock, and reference it in context. This \
            //   prevents the database from being erased while using it in this block.
            let _access = STORE_ACCESS_LOCK.read().unwrap();

            if let Ok(kv_store) = StoreKVPool::acquire(collection) {
                let action = StoreKVActionBuilder::write(bucket, kv_store);

                // TODO: when pushing anything to a list, prevent DOS by limiting the list length
                // TODO: when poping items to prevent DOS, also nuke IID from term-to-IIDs mapping
                // TODO: handle errors on all action.set() method and return a general ERR if one \
                //   fails (with a proper error log).

                // Try to resolve existing OID to IID, otherwise initialize IID (store the \
                //   bi-directional relationship)
                let oid = object.as_str().to_owned();
                let iid = action.get_oid_to_iid(&oid).unwrap_or(None).or_else(|| {
                    // TODO: for initializer, must implement a per-bucket mutex as multiple \
                    //   channel threads pushing at the same time may conflict.

                    info!("must initialize push executor oid-to-iid and iid-to-oid");

                    if let Ok(iid_incr) = action.get_meta_to_value(StoreMetaKey::IIDIncr) {
                        let iid_incr = if let Some(iid_incr) = iid_incr {
                            match iid_incr {
                                StoreMetaValue::IIDIncr(iid_incr) => iid_incr + 1,
                            }
                        } else {
                            0
                        };

                        // Bump last stored increment
                        if action
                            .set_meta_to_value(
                                StoreMetaKey::IIDIncr,
                                StoreMetaValue::IIDIncr(iid_incr),
                            )
                            .is_ok()
                            == true
                        {
                            // Associate OID <> IID (bidirectional)
                            action.set_oid_to_iid(&oid, iid_incr).ok();
                            action.set_iid_to_oid(iid_incr, &oid).ok();

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
                            action
                                .get_iid_to_terms(iid)
                                .unwrap_or(None)
                                .unwrap_or(Vec::new()),
                        );

                    info!(
                        "got push executor stored iid-to-terms: {:?}",
                        iid_terms_hashed
                    );

                    while let Some((_, term_hashed)) = lexer.next() {
                        // Check that term is not already linked to IID
                        if iid_terms_hashed.contains(&term_hashed) == false {
                            if let Ok(term_iids) = action.get_term_to_iids(term_hashed) {
                                has_commits = true;

                                // Add IID in first position in list for terms
                                let mut term_iids = term_iids.unwrap_or(Vec::new());

                                if term_iids.contains(&iid) == true {
                                    term_iids.remove_item(&iid);
                                }

                                info!("has push executor term-to-iids: {}", iid);

                                term_iids.insert(0, iid);

                                action.set_term_to_iids(term_hashed, &term_iids).ok();

                                // Insert term into IID to terms map
                                iid_terms_hashed.insert(term_hashed);
                            }
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

                        action.set_iid_to_terms(iid, &collected_iids).ok();
                    }

                    return Ok(());
                }
            }
        }

        Err(())
    }
}
