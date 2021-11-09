// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use super::types::*;
use crate::lexer::token::TokenLexer;
use crate::store::item::StoreItem;

pub enum Query<'a> {
    Search(
        StoreItem<'a>,
        QuerySearchID<'a>,
        TokenLexer<'a>,
        QuerySearchLimit,
        QuerySearchOffset,
    ),
    Suggest(
        StoreItem<'a>,
        QuerySearchID<'a>,
        TokenLexer<'a>,
        QuerySearchLimit,
    ),
    Push(StoreItem<'a>, TokenLexer<'a>),
    Pop(StoreItem<'a>, TokenLexer<'a>),
    Count(StoreItem<'a>),
    FlushC(StoreItem<'a>),
    FlushB(StoreItem<'a>),
    FlushO(StoreItem<'a>),
}
