// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

#![cfg_attr(feature = "benchmark", feature(test))]
#![deny(unstable_features, unused_imports, unused_qualifications, clippy::all)]

#[macro_use]
extern crate log;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate serde_derive;

pub mod config;
mod executor;
mod lexer;
pub mod query;
mod stopwords;
pub mod store;

use std::ops::Deref;

use config::options::Config;
use config::reader::ConfigReader;
use query::actions::Query;
use store::fst::StoreFSTPool;
use store::kv::StoreKVPool;
use store::operation::StoreOperationDispatch;

struct AppArgs {
    config: String,
}

#[cfg(unix)]
#[cfg(feature = "allocator-jemalloc")]
#[global_allocator]
static ALLOC: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

lazy_static! {
    static ref APP_ARGS: AppArgs = AppArgs{config: "".to_string()};
    static ref APP_CONF: Config = ConfigReader::make();
}

/// called when startup
pub fn sonic_init(config_path: &str) {
    // Ensure all statics are valid (a `deref` is enough to lazily initialize them)
    let app_args: &AppArgs =  APP_ARGS.deref();
    let p = app_args as *const AppArgs as *mut AppArgs;
    unsafe {
        (*p).config = config_path.to_string();
    }

    let _ = APP_CONF.deref();
}

/// called when exit
pub fn sonic_exit() {
    // Perform a KV flush (ensures all in-memory changes are synced on-disk before shutdown)
    StoreKVPool::flush(true);

    // Perform a FST consolidation (ensures all in-memory items are synced on-disk before \
    //   shutdown; otherwise we would lose all non-consolidated FST changes)
    StoreFSTPool::consolidate(true);
}

/// called every 10 seconds
pub fn sonic_tick() {
    // #1: Janitors
    StoreKVPool::janitor();
    StoreFSTPool::janitor();

    // #2: Others
    StoreKVPool::flush(false);
    StoreFSTPool::consolidate(false);
}

/// execute a query
/// ```ignore
/// let query = QueryBuilder::push("home", "book", "3-body", "hello 3-body world!", None).unwrap();
/// let ret = execute_query(query).unwrap();
/// ```
pub fn execute_query(query: Query) -> Result<Option<String>, ()> {
    StoreOperationDispatch::dispatch(query)
}

#[cfg(test)]
mod tests {
    use crate::query::builder::QueryBuilder;

    use super::*;

    #[test]
    fn test_lib() {
        // init
        sonic_init("./config.cfg");

        // push
        let query = QueryBuilder::push("home", "book", "3-body", "hello 3-body world!", None).unwrap();
        let ret = execute_query(query).unwrap();
        println!("push return: {:?}", ret);
        let query = QueryBuilder::push("home", "book", "sonic-inside", "hello sonic!", None).unwrap();
        let ret = execute_query(query).unwrap();
        println!("push return: {:?}", ret);
        sonic_tick();

        // pop
        let query = QueryBuilder::pop("home", "book", "sonic inside", "hello sonic!").unwrap();
        let ret = execute_query(query).unwrap();
        println!("pop return: {:?}", ret);

        // query
        let query = QueryBuilder::search("query_id", "home", "book", "hello", 10, 0, None).unwrap();
        let ret = execute_query(query).unwrap();
        println!("search return: {:?}", ret);
        sonic_tick();

        // list
        let query = QueryBuilder::list("query_id", "home", "book", 10, 0).unwrap();
        let ret = execute_query(query).unwrap();
        println!("list return: {:?}", ret);
        sonic_tick();

        // exit
        sonic_exit();
    }
}