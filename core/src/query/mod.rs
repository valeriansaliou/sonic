// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod builder;
mod types;

use crate::lexer::preprocessor::PreprocessorOutput;
use crate::store::StoreItem;

pub use self::types::*;

pub enum Query<'a> {
    Search(
        StoreItem<'a>,
        QuerySearchID<'a>,
        PreprocessorOutput<'a>,
        QuerySearchLimit,
        QuerySearchOffset,
    ),
    Suggest(
        StoreItem<'a>,
        QuerySearchID<'a>,
        PreprocessorOutput<'a>,
        QuerySearchLimit,
    ),
    List(
        StoreItem<'a>,
        QuerySearchID<'a>,
        QuerySearchLimit,
        QuerySearchOffset,
    ),
    Push(StoreItem<'a>, PreprocessorOutput<'a>, PushAssumeNew),
    Pop(StoreItem<'a>, PreprocessorOutput<'a>),
    Count(StoreItem<'a>),
    FlushC(StoreItem<'a>),
    FlushB(StoreItem<'a>),
    FlushO(StoreItem<'a>),
}
