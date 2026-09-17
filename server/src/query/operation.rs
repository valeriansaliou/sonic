// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use sonic::Executor;

use super::Query;
use crate::util::itertools::Itertools as _;

pub struct StoreOperationDispatch;

impl StoreOperationDispatch {
    pub fn dispatch(query: Query, executor: &Executor) -> Result<Option<String>, ()> {
        // Dispatch de-constructed query to its target executor
        match query {
            Query::Search(c, b, query_id, lexer, limit, offset) => executor
                .search(c, b, query_id, lexer, limit, offset)
                .map(|results| {
                    if results.is_empty() {
                        None
                    } else {
                        Some(results.join(" "))
                    }
                }),
            Query::Suggest(c, b, query_id, lexer, limit) => executor
                .suggest(c, b, query_id, lexer, limit)
                .map(|results| results.map(|mut results| results.join(" "))),
            Query::List(c, b, query_id, limit, offset) => executor
                .list(c, b, query_id, limit, offset)
                .map(|results| results.join(" "))
                .map(Some),
            Query::Push(c, b, o, lexer, assume_new) => {
                executor.push(c, b, o, lexer, assume_new).map(|_| None)
            }
            Query::Pop(c, b, o, lexer) => executor
                .pop(c, b, o, lexer)
                .map(|count| Some(count.to_string())),
            Query::Count(c, Some(b), Some(o)) => executor
                .counto(c, b, o)
                .map(|count| Some(count.to_string())),
            Query::Count(c, Some(b), None) => {
                executor.countb(c, b).map(|count| Some(count.to_string()))
            }
            Query::Count(c, None, None) => executor.countc(c).map(|count| Some(count.to_string())),
            Query::Count(_c, None, Some(_)) => unreachable!(),
            Query::FlushC(c) => executor.flushc(c).map(|count| Some(count.to_string())),
            Query::FlushB(c, b) => executor.flushb(c, b).map(|count| Some(count.to_string())),
            Query::FlushO(c, b, o) => executor
                .flusho(c, b, o)
                .map(|count| Some(count.to_string())),
        }
    }
}
