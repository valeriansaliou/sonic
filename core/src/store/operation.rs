// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::{Executor, Query};

pub struct StoreOperationDispatch;

impl StoreOperationDispatch {
    pub fn dispatch(query: Query, executor: &Executor) -> Result<Option<String>, ()> {
        // Dispatch de-constructed query to its target executor
        match query {
            Query::Search(store, query_id, lexer, limit, offset, time_range) => executor
                .search_with_range(store, query_id, lexer, limit, offset, time_range)
                .map(|results| {
                    if results.is_empty() {
                        None
                    } else {
                        Some(results.join(" "))
                    }
                }),
            Query::SearchDocuments(store, query_id, lexer, limit, offset, time_range) => executor
                .search_documents(store, query_id, lexer, limit, offset, time_range)
                .and_then(|documents| serde_json::to_string(&documents).map_err(|_| ()))
                .map(Some),
            Query::List(store, query_id, limit, offset) => executor
                .list(store, query_id, limit, offset)
                .map(|results| results.join(" "))
                .map(Some),
            Query::Push(store, lexer, text) => executor.push(store, lexer, text).map(|_| None),
            Query::Upsert(store, lexer, document) => {
                executor.upsert(store, lexer, document).map(|_| None)
            }
            Query::Pop(store, text) => executor
                .pop(store, text)
                .map(|count| Some(count.to_string())),
            Query::Count(store) => executor.count(store).map(|count| Some(count.to_string())),
            Query::FlushC(store) => executor.flushc(store).map(|count| Some(count.to_string())),
            Query::FlushB(store) => executor.flushb(store).map(|count| Some(count.to_string())),
            Query::FlushO(store) => executor.flusho(store).map(|count| Some(count.to_string())),
        }
    }
}
