// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::collections::HashSet;
use std::iter::FromIterator;

use crate::lexer::token::TokenLexer;
use crate::store::identifiers::{StoreMetaKey, StoreMetaValue};
use crate::store::item::StoreItem;
use crate::store::kv::{StoreKVActionBuilder, StoreKVPool};

pub struct ExecutorPush;

impl ExecutorPush {
    pub fn execute<'a>(store: StoreItem<'a>, mut lexer: TokenLexer<'a>) -> Result<(), ()> {
        if let StoreItem(collection, Some(bucket), Some(object)) = store {
            if let Ok(kv_store) = StoreKVPool::acquire(collection.as_str()) {
                let action = StoreKVActionBuilder::new(bucket, kv_store);

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
                    let mut iid_terms: HashSet<String> = HashSet::from_iter(
                        action
                            .get_iid_to_terms(iid)
                            .unwrap_or(None)
                            .unwrap_or(Vec::new()),
                    );

                    info!("got push executor stored iid-to-terms: {:?}", iid_terms);

                    while let Some(term) = lexer.next() {
                        // Check that term is not already linked to IID
                        if iid_terms.contains(&term) == false {
                            if let Ok(term_iids) = action.get_term_to_iids(&term) {
                                has_commits = true;

                                // Add IID in first position in list for terms
                                let mut term_iids = term_iids.unwrap_or(Vec::new());

                                if term_iids.contains(&iid) == true {
                                    term_iids.remove_item(&iid);
                                }

                                info!("has push executor term-to-iids: {}", iid);

                                term_iids.insert(0, iid);

                                action.set_term_to_iids(&term, &term_iids).ok();

                                // Insert term into IID to terms map
                                iid_terms.insert(term);
                            }
                        }
                    }

                    // Commit updated list of terms for IID? (if any commit made)
                    if has_commits == true {
                        let collected_iids: Vec<String> = iid_terms.into_iter().collect();

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
