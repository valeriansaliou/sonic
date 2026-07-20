// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::lexer::TokenLexer;
use crate::store::StoreItem;
use crate::store::document::StoreDocument;
use crate::store::fst::StoreFSTActionBuilder;
use crate::store::kv::{StoreKVAcquireMode, StoreKVActionBuilder};

impl super::Executor {
    pub fn push(&self, item: StoreItem, lexer: TokenLexer, text: String) -> Result<(), ()> {
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
                if let Ok((iid, is_new_iid)) = kv_action.resolve_or_reserve_iid(oid) {
                    let existing_document = kv_action.get_document_by_iid(iid)?;
                    let old_terms = if existing_document.is_some() {
                        self.indexed_terms_for_iid(&kv_action, iid)?
                    } else {
                        Vec::new()
                    };
                    let mut document = existing_document.unwrap_or(StoreDocument::new(
                        oid,
                        0,
                        "",
                        serde_json::json!({}),
                    )?);
                    if !document.text.is_empty() && !text.is_empty() {
                        document.text.push(' ');
                    }
                    document.text.push_str(&text);

                    let mut new_terms = old_terms.clone();
                    for (token, _) in lexer {
                        let term = token.as_str().to_owned();
                        if !new_terms.contains(&term) {
                            new_terms.push(term);
                        }
                    }

                    let frequencies = kv_action.batch_upsert_document(
                        iid,
                        oid,
                        is_new_iid,
                        &old_terms,
                        &new_terms,
                        &document,
                    )?;
                    for (term, frequency) in frequencies {
                        if frequency == 0 {
                            fst_action.pop_word(&term);
                        } else {
                            fst_action.push_word(&term, frequency, &self.app_conf.store.fst);
                        }
                    }

                    return Ok(());
                }
            }
        }

        Err(())
    }
}
