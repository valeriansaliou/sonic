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
    pub fn push(&self, item: StoreItem, lexer: TokenLexer) -> Result<(), ()> {
        if let StoreItem(collection, Some(bucket), Some(object)) = item {
            // Important: acquire database access read lock, and reference it in context. This \
            //   prevents the database from being erased while using it in this block.
            let _kv_read_guard = self.kv_pool.lock_read_access();
            let _fst_read_guard = self.fst_pool.lock_read_access();

            if let Ok(kv_store) = self.kv_pool.acquire(StoreKVAcquireMode::Any, collection) {
                // Important: acquire bucket store write lock
                executor_kv_lock_write!(kv_store);

                let kv_action = StoreKVActionBuilder::access_or_create(bucket, kv_store);
                let Some(bucket_id) = kv_action.bucket_id() else {
                    return Err(());
                };
                let fst_store = self.fst_pool.acquire(collection, bucket_id)?;
                let fst_action = StoreFSTActionBuilder::access(fst_store);

                let oid = object.as_str();
                if let Ok(iid) = kv_action.get_or_create_iid(oid) {
                    if kv_action.get_document_by_iid(iid)?.is_some() {
                        tracing::error!("cannot PUSH an OID managed by UPSERT");
                        return Err(());
                    }
                    // Acquire list of terms for IID
                    let iid_terms: LinkedHashSet<String> = LinkedHashSet::from_iter(
                        kv_action
                            .get_iid_to_terms(iid)
                            .unwrap_or(None)
                            .unwrap_or_default(),
                    );

                    tracing::debug!("got push executor stored iid-to-terms: {:?}", iid_terms);

                    let mut new_terms = Vec::<String>::new();
                    for (token, _) in lexer {
                        let term = token.as_str().to_owned();
                        if !iid_terms.contains(&term) && !new_terms.contains(&term) {
                            new_terms.push(term);
                        }
                    }

                    let existing_terms = iid_terms.into_iter().collect::<Vec<_>>();
                    let frequencies =
                        kv_action.batch_insert_iid_terms(iid, &existing_terms, &new_terms)?;
                    for (term, frequency) in frequencies {
                        if fst_action.push_word(&term, frequency, &self.app_conf.store.fst) {
                            tracing::debug!("push term committed to graph: {}", term);
                        }
                    }

                    return Ok(());
                }
            }
        }

        Err(())
    }
}
