// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::lexer::token::TokenLexer;
use crate::query::types::{QuerySearchID, QuerySearchLimit, QuerySearchOffset};
use crate::store::item::StoreItem;

pub struct ExecutorSearch;

impl ExecutorSearch {
    pub fn execute<'a>(
        store: StoreItem<'a>,
        event_id: QuerySearchID,
        lexer: TokenLexer<'a>,
        limit: QuerySearchLimit,
        offset: QuerySearchOffset,
    ) -> Option<Vec<String>> {
        // TODO
        Some(vec![
            "session_71f3d63b-57c4-40fb-8557-e11309170edd".to_string(),
            "session_6501e83a-b778-474f-b60c-7bcad54d755f".to_string(),
            "session_8ab1dcdd-eb53-4294-a7d1-080a7245622d".to_string(),
        ])
    }
}
