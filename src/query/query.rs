// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use super::types::*;
use crate::lexer::token::LexedTokens;
use crate::store::item::StoreItem;

pub enum Query<'a> {
    Search(
        StoreItem<'a>,
        QuerySearchID,
        LexedTokens,
        QuerySearchLimit,
        QuerySearchOffset,
    ),
    Suggest(StoreItem<'a>, QuerySearchID, LexedTokens),
    Push(StoreItem<'a>, LexedTokens),
    Pop(StoreItem<'a>),
    Count(StoreItem<'a>),
    FlushC(StoreItem<'a>),
    FlushB(StoreItem<'a>),
    FlushO(StoreItem<'a>),
}
