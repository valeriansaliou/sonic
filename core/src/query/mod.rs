// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod builder;
mod types;

use crate::lexer::TokenLexer;
use crate::store::StoreItem;
use crate::store::document::StoreDocument;

pub use self::types::*;

pub enum Query<'a> {
    Search(
        StoreItem<'a>,
        QuerySearchID<'a>,
        TokenLexer<'a>,
        QuerySearchLimit,
        QuerySearchOffset,
        Option<QueryTimeRange>,
    ),
    SearchDocuments(
        StoreItem<'a>,
        QuerySearchID<'a>,
        TokenLexer<'a>,
        QuerySearchLimit,
        QuerySearchOffset,
        Option<QueryTimeRange>,
    ),
    List(
        StoreItem<'a>,
        QuerySearchID<'a>,
        QuerySearchLimit,
        QuerySearchOffset,
    ),
    Push(StoreItem<'a>, TokenLexer<'a>, String),
    Upsert(StoreItem<'a>, TokenLexer<'a>, StoreDocument),
    Pop(StoreItem<'a>, String),
    Count(StoreItem<'a>),
    FlushC(StoreItem<'a>),
    FlushB(StoreItem<'a>),
    FlushO(StoreItem<'a>),
}
