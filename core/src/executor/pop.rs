// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Crisp IM SAS
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::lexer::{TokenLexerBuilder, TokenLexerMode};
use crate::store::StoreItem;
use crate::store::fst::StoreFSTActionBuilder;
use crate::store::kv::{StoreKVAcquireMode, StoreKVActionBuilder};

impl super::Executor {
    pub fn pop(&self, item: StoreItem, text: String) -> Result<u32, ()> {
        let StoreItem(collection, Some(bucket), Some(object)) = item else {
            return Err(());
        };
        if text.is_empty() {
            return Ok(0);
        }

        let _kv_read_guard = self.kv_pool.lock_read_access();
        let _fst_read_guard = self.fst_pool.lock_read_access();
        let kv_store = self
            .kv_pool
            .acquire(StoreKVAcquireMode::OpenOnly, collection)?;
        executor_kv_lock_write!(kv_store);
        let kv_action = StoreKVActionBuilder::access(bucket, kv_store);
        let Some(bucket_id) = kv_action.bucket_id() else {
            return Ok(0);
        };
        let Some(iid) = kv_action.get_oid_to_iid(object.as_str())? else {
            return Ok(0);
        };
        let Some(mut document) = kv_action.get_document_by_iid(iid)? else {
            return Ok(0);
        };
        let Some(start) = document.text.find(&text) else {
            return Ok(0);
        };

        let old_terms = self.indexed_terms_for_iid(&kv_action, iid)?;
        document.text.replace_range(start..start + text.len(), "");
        document.text = document.text.trim().to_owned();
        let new_terms = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            None,
            &document.text,
            self.app_conf.normalization,
            self.app_conf.tokenization,
        )?
        .map(|(token, _)| token.into_inner())
        .collect::<Vec<_>>();
        let removed = old_terms
            .iter()
            .filter(|term| !new_terms.contains(term))
            .count() as u32;
        let frequencies = kv_action.batch_upsert_document(
            iid,
            object.as_str(),
            false,
            &old_terms,
            &new_terms,
            &document,
        )?;

        let fst_store = self.fst_pool.acquire(collection, bucket_id)?;
        let fst_action = StoreFSTActionBuilder::access(fst_store);
        for (term, frequency) in frequencies {
            if frequency == 0 {
                fst_action.pop_word(&term);
            } else {
                fst_action.push_word(&term, frequency, &self.app_conf.store.fst);
            }
        }

        Ok(removed)
    }
}
