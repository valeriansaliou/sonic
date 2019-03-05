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
    pub fn execute<'a>(store: StoreItem<'a>, lexer: TokenLexer<'a>) -> Result<(), ()> {
        if let StoreItem(collection, Some(bucket), Some(object)) = store {
            if let Ok(kv_store) = StoreKVPool::acquire(collection.as_str()) {
                let action = StoreKVActionBuilder::new(bucket, kv_store);

                // Try to resolve existing OID to IID, otherwise initialize IID (store the \
                //   bi-directional relationship)
                let oid = object.as_str().to_owned();
                let iid = action.get_oid_to_iid(&oid).or_else(|| {
                    // TODO: for initializer, must implement a locking mechanism if it is to be shared

                    if let Some(iid_incr) = action.get_meta_to_value(StoreMetaKey::IIDIncr) {
                        let iid_incr = match iid_incr {
                            StoreMetaValue::IIDIncr(iid_incr) => iid_incr + 1,
                            _ => 0,
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
                            action.set_oid_to_iid(&oid, iid_incr);
                            action.set_iid_to_oid(iid_incr, &oid);

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
                    let mut iid_terms: HashSet<String> =
                        HashSet::from_iter(action.get_iid_to_terms(iid).unwrap_or(Vec::new()));

                    for term in lexer {
                        // Check that term is not already linked to IID
                        if iid_terms.contains(&term) == false {
                            has_commits = true;

                            // Add IID in first position in list for terms, with sliding window if too many \
                            //   of them
                            let mut term_iids =
                                action.get_term_to_iids(&term).unwrap_or(Vec::new());

                            if term_iids.contains(&iid) == false {
                                term_iids.insert(0, iid);

                                action.set_term_to_iids(&term, &term_iids);
                            }

                            // Insert term into IID to terms map
                            iid_terms.insert(term);
                        }
                    }

                    // Commit updated list of terms for IID? (if any commit made)
                    if has_commits == true {
                        let collected_iids: Vec<String> = iid_terms.into_iter().collect();

                        action.set_iid_to_terms(iid, &collected_iids);
                    }

                    return Ok(());
                }
            }
        }

        Err(())
    }
}
