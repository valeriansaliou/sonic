// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::lexer::token::TokenLexer;
use crate::store::item::StoreItem;
use crate::store::kv::StoreKVPool;

pub struct ExecutorPush;

impl ExecutorPush {
    pub fn execute<'a>(store: StoreItem<'a>, lexer: TokenLexer<'a>) -> Result<(), ()> {
        if let StoreItem(collection, Some(bucket), Some(object)) = store {
            if let Ok(kv_store) = StoreKVPool::acquire(collection.as_str()) {
                // Try to resolve existing OID to IID, otherwise initialize IID (store \
                //   the bi-directional relationship)
                // TODO

                // Add IID in first position in list for terms, with sliding window if too many \
                //   of them
                // TODO

                // Link IID to 'term', with sliding window if too many of them
                // TODO

                return Ok(());
            }
        }

        Err(())
    }
}
