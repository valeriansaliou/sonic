// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::collections::HashSet;
use std::iter::FromIterator;

use crate::lexer::token::TokenLexer;
use crate::query::types::{QuerySearchID, QuerySearchLimit, QuerySearchOffset};
use crate::store::identifiers::StoreObjectIID;
use crate::store::item::StoreItem;
use crate::store::kv::{StoreKVActionBuilder, StoreKVPool};

pub struct ExecutorSearch;

impl ExecutorSearch {
    pub fn execute<'a>(
        store: StoreItem<'a>,
        _event_id: QuerySearchID,
        mut lexer: TokenLexer<'a>,
        _limit: QuerySearchLimit,
        _offset: QuerySearchOffset,
    ) -> Result<Option<Vec<String>>, ()> {
        if let StoreItem(collection, Some(bucket), None) = store {
            if let Ok(kv_store) = StoreKVPool::acquire(collection.as_str()) {
                let action = StoreKVActionBuilder::new(bucket, kv_store);

                // Try to resolve existing search terms to IIDs, and perform an algebraic AND on \
                //   all resulting IIDs for each given term.
                let mut found_iids: HashSet<StoreObjectIID> = HashSet::new();

                // TODO: support for multiple terms?
                while let Some(term) = lexer.next() {
                    if let Ok(iids_inner) = action.get_term_to_iids(&term) {
                        let iids = iids_inner.unwrap_or(Vec::new());

                        debug!(
                            "got search executor iids: {:?} for term: {}", iids, term
                        );

                        // Intersect found IIDs with previous batch
                        let iids_set: HashSet<StoreObjectIID> =
                            HashSet::from_iter(iids.iter().map(|value| *value));

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
                }

                // Resolve OIDs from IIDs
                let mut result_oids = Vec::new();

                for found_iid in found_iids {
                    if let Ok(Some(oid)) = action.get_iid_to_oid(found_iid) {
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
