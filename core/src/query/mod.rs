// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod builder;
mod types;

use crate::lexer::preprocessor::PreprocessorOutput;
use crate::store::StoreItemPart;

pub use self::types::*;

pub enum Query<'a> {
    Search(
        StoreItemPart<'a>,
        StoreItemPart<'a>,
        QuerySearchID<'a>,
        PreprocessorOutput<'a>,
        QuerySearchLimit,
        QuerySearchOffset,
    ),
    Suggest(
        StoreItemPart<'a>,
        StoreItemPart<'a>,
        QuerySearchID<'a>,
        PreprocessorOutput<'a>,
        QuerySearchLimit,
    ),
    List(
        StoreItemPart<'a>,
        StoreItemPart<'a>,
        QuerySearchID<'a>,
        QuerySearchLimit,
        QuerySearchOffset,
    ),
    Push(
        StoreItemPart<'a>,
        StoreItemPart<'a>,
        StoreItemPart<'a>,
        PreprocessorOutput<'a>,
        PushAssumeNew,
    ),
    Pop(
        StoreItemPart<'a>,
        StoreItemPart<'a>,
        StoreItemPart<'a>,
        PreprocessorOutput<'a>,
    ),
    Count(
        StoreItemPart<'a>,
        Option<StoreItemPart<'a>>,
        Option<StoreItemPart<'a>>,
    ),
    FlushC(StoreItemPart<'a>),
    FlushB(StoreItemPart<'a>, StoreItemPart<'a>),
    FlushO(StoreItemPart<'a>, StoreItemPart<'a>, StoreItemPart<'a>),
}
