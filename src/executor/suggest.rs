// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::lexer::token::TokenLexer;
use crate::query::types::QuerySearchID;
use crate::store::item::StoreItem;
use crate::store::kv::{StoreKVActionBuilder, StoreKVPool, STORE_ACCESS_LOCK};

pub struct ExecutorSuggest;

impl ExecutorSuggest {
    pub fn execute<'a>(
        store: StoreItem<'a>,
        _event_id: QuerySearchID,
        _lexer: TokenLexer<'a>,
    ) -> Result<Option<Vec<String>>, ()> {
        if let StoreItem(collection, Some(bucket), None) = store {
            // Important: acquire database access read lock, and reference it in context. This \
            //   prevents the database from being erased while using it in this block.
            let _access = STORE_ACCESS_LOCK.read().unwrap();

            if let Ok(kv_store) = StoreKVPool::acquire(collection) {
                let _action = StoreKVActionBuilder::read(bucket, kv_store);

                // TODO
            }
        }

        Err(())
    }
}
