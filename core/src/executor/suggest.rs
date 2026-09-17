// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use super::types::{QuerySearchID, QuerySearchLimit};
use crate::lexer::preprocessor::PreprocessorOutput;
use crate::store::StoreItemPart;

impl super::Executor {
    pub fn suggest(
        &self,
        collection: StoreItemPart,
        bucket: StoreItemPart,
        _event_id: QuerySearchID,
        input: PreprocessorOutput,
        limit: QuerySearchLimit,
    ) -> Result<Option<impl ExactSizeIterator<Item = String> + DoubleEndedIterator>, ()> {
        // Important: acquire graph access read lock, and reference it in context. This \
        //   prevents the graph from being erased while using it in this block.
        let _fst_read_guard = self.fst_pool.lock_read_access();

        if let Ok(fst_store) = self.fst_pool.acquire(collection, bucket) {
            let mut tokens = input.tokens();

            if let (Some(token), None) = (tokens.next(), tokens.next()) {
                let len = token.as_original().len();
                let term = token.into_normalized();

                tracing::debug!("running suggest on word: {term:?}");

                return match fst_store.suggest_words(term, len, limit as usize, None) {
                    Some(words) => Ok(Some(words.map(|(k, _)| k))),
                    None => Ok(None),
                };
            }
        }

        Err(())
    }
}
