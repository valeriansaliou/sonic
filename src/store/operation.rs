// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::executor::count::ExecutorCount;
use crate::executor::flushb::ExecutorFlushB;
use crate::executor::flushc::ExecutorFlushC;
use crate::executor::flusho::ExecutorFlushO;
use crate::executor::pop::ExecutorPop;
use crate::executor::push::ExecutorPush;
use crate::executor::search::ExecutorSearch;
use crate::query::query::Query;

pub struct StoreOperationDispatch;

impl StoreOperationDispatch {
    pub fn dispatch(query: Query) -> Result<Option<String>, ()> {
        // Dispatch de-constructed query to its target executor
        match query {
            Query::Search(store, query_id, lexer, limit, offset) => {
                ExecutorSearch::execute(store, query_id, lexer, limit, offset)
                    .map(|results| results.map(|results| results.join(" ")))
            }
            Query::Push(store, lexer) => ExecutorPush::execute(store, lexer).map(|_| None),
            Query::Pop(store) => ExecutorPop::execute(store).map(|count| Some(count.to_string())),
            Query::Count(store) => {
                ExecutorCount::execute(store).map(|count| Some(count.to_string()))
            }
            Query::FlushC(store) => {
                ExecutorFlushC::execute(store).map(|count| Some(count.to_string()))
            }
            Query::FlushB(store) => {
                ExecutorFlushB::execute(store).map(|count| Some(count.to_string()))
            }
            Query::FlushO(store) => {
                ExecutorFlushO::execute(store).map(|count| Some(count.to_string()))
            }
        }
    }
}
