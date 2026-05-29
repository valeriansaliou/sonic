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
            Query::Search(store, query_id, lexer, limit, offset) => executor
                .search(store, query_id, lexer, limit, offset)
                .map(|results| {
                    if results.is_empty() {
                        None
                    } else {
                        Some(results.join(" "))
                    }
                }),
            Query::Suggest(store, query_id, lexer, limit) => executor
                .suggest(store, query_id, lexer, limit)
                .map(|results| results.map(|results| results.join(" "))),
            Query::List(store, query_id, limit, offset) => executor
                .list(store, query_id, limit, offset)
                .map(|results| results.join(" "))
                .map(Some),
            Query::Push(store, lexer) => executor.push(store, lexer).map(|_| None),
            Query::Pop(store, lexer) => executor
                .pop(store, lexer)
                .map(|count| Some(count.to_string())),
            Query::Count(store) => executor.count(store).map(|count| Some(count.to_string())),
            Query::FlushC(store) => executor.flushc(store).map(|count| Some(count.to_string())),
            Query::FlushB(store) => executor.flushb(store).map(|count| Some(count.to_string())),
            Query::FlushO(store) => executor.flusho(store).map(|count| Some(count.to_string())),
        }
    }
}
