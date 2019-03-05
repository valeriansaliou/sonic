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
            Query::Pop(store) => {
                // TODO: return OK or ERR from execute()
                Ok(Some(ExecutorPop::execute(store).to_string()))
            }
            Query::Count(store) => {
                // TODO: return OK or ERR from execute()
                Ok(Some(ExecutorCount::execute(store).to_string()))
            }
            Query::FlushC(store) => {
                // TODO: return OK or ERR from execute()
                Ok(Some(ExecutorFlushC::execute(store).to_string()))
            }
            Query::FlushB(store) => {
                // TODO: return OK or ERR from execute()
                Ok(Some(ExecutorFlushB::execute(store).to_string()))
            }
            Query::FlushO(store) => {
                // TODO: return OK or ERR from execute()
                Ok(Some(ExecutorFlushO::execute(store).to_string()))
            }
        }
    }
}
