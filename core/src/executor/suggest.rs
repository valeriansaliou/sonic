// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::lexer::TokenLexer;
use crate::query::{QuerySearchID, QuerySearchLimit};
use crate::store::StoreItem;
use crate::store::fst::StoreFSTActionBuilder;

impl super::Executor {
    pub fn suggest(
        &self,
        item: StoreItem,
        _event_id: QuerySearchID,
        mut lexer: TokenLexer,
        limit: QuerySearchLimit,
    ) -> Result<Option<impl ExactSizeIterator<Item = String> + DoubleEndedIterator>, ()> {
        if let StoreItem(collection, Some(bucket), None) = item {
            // Important: acquire graph access read lock, and reference it in context. This \
            //   prevents the graph from being erased while using it in this block.
            let _fst_read_guard = self.fst_pool.lock_read_access();

            if let Ok(fst_store) = self.fst_pool.acquire(collection, bucket) {
                let fst_action = StoreFSTActionBuilder::access(fst_store);

                if let (Some(word), None) = (lexer.next(), lexer.next()) {
                    tracing::debug!("running suggest on word: {}", word.0);

                    return match fst_action.suggest_words(&word.0, word.2, limit as usize, None) {
                        Some(words) => Ok(Some(words.map(|(k, _)| k))),
                        None => Ok(None),
                    };
                }
            }
        }

        Err(())
    }
}
