// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::lexer::token::TokenLexer;
use crate::store::item::StoreItem;

pub struct ExecutorFlushO;

impl ExecutorFlushO {
    pub fn execute<'a>(store: StoreItem<'a>) -> u64 {
        // TODO
        0
    }
}
