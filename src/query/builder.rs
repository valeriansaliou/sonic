// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Sonic OSS License v1.0 (SOSSL v1.0)

use super::query::Query;
use crate::lexer::token::{TokenLexerBuilder, TokenLexerMode};
use crate::store::item::StoreItemBuilder;

pub struct QueryBuilder;

pub type QueryBuilderResult<'a> = Result<Query<'a>, ()>;

impl QueryBuilder {
    pub fn search<'a>(
        query_id: &'a str,
        collection: &'a str,
        bucket: &'a str,
        terms: &'a str,
        limit: u16,
        offset: u32,
    ) -> QueryBuilderResult<'a> {
        match (
            StoreItemBuilder::from_depth_2(collection, bucket),
            TokenLexerBuilder::from(TokenLexerMode::NormalizeAndCleanup, terms),
        ) {
            (Ok(store), Ok(text_lexed)) => {
                Ok(Query::Search(store, query_id, text_lexed, limit, offset))
            }
            _ => Err(()),
        }
    }

    pub fn suggest<'a>(
        query_id: &'a str,
        collection: &'a str,
        bucket: &'a str,
        terms: &'a str,
        limit: u16,
    ) -> QueryBuilderResult<'a> {
        match (
            StoreItemBuilder::from_depth_2(collection, bucket),
            TokenLexerBuilder::from(TokenLexerMode::NormalizeOnly, terms),
        ) {
            (Ok(store), Ok(text_lexed)) => Ok(Query::Suggest(store, query_id, text_lexed, limit)),
            _ => Err(()),
        }
    }

    pub fn push<'a>(
        collection: &'a str,
        bucket: &'a str,
        object: &'a str,
        text: &'a str,
    ) -> QueryBuilderResult<'a> {
        match (
            StoreItemBuilder::from_depth_3(collection, bucket, object),
            TokenLexerBuilder::from(TokenLexerMode::NormalizeAndCleanup, text),
        ) {
            (Ok(store), Ok(text_lexed)) => Ok(Query::Push(store, text_lexed)),
            _ => Err(()),
        }
    }

    pub fn pop<'a>(
        collection: &'a str,
        bucket: &'a str,
        object: &'a str,
        text: &'a str,
    ) -> QueryBuilderResult<'a> {
        match (
            StoreItemBuilder::from_depth_3(collection, bucket, object),
            TokenLexerBuilder::from(TokenLexerMode::NormalizeOnly, text),
        ) {
            (Ok(store), Ok(text_lexed)) => Ok(Query::Pop(store, text_lexed)),
            _ => Err(()),
        }
    }

    pub fn count<'a>(
        collection: &'a str,
        bucket: Option<&'a str>,
        object: Option<&'a str>,
    ) -> QueryBuilderResult<'a> {
        let store_result = match (bucket, object) {
            (Some(bucket_inner), Some(object_inner)) => {
                StoreItemBuilder::from_depth_3(collection, bucket_inner, object_inner)
            }
            (Some(bucket_inner), None) => StoreItemBuilder::from_depth_2(collection, bucket_inner),
            _ => StoreItemBuilder::from_depth_1(collection),
        };

        match store_result {
            Ok(store) => Ok(Query::Count(store)),
            _ => Err(()),
        }
    }

    pub fn flushc<'a>(collection: &'a str) -> QueryBuilderResult<'a> {
        match StoreItemBuilder::from_depth_1(collection) {
            Ok(store) => Ok(Query::FlushC(store)),
            _ => Err(()),
        }
    }

    pub fn flushb<'a>(collection: &'a str, bucket: &'a str) -> QueryBuilderResult<'a> {
        match StoreItemBuilder::from_depth_2(collection, bucket) {
            Ok(store) => Ok(Query::FlushB(store)),
            _ => Err(()),
        }
    }

    pub fn flusho<'a>(
        collection: &'a str,
        bucket: &'a str,
        object: &'a str,
    ) -> QueryBuilderResult<'a> {
        match StoreItemBuilder::from_depth_3(collection, bucket, object) {
            Ok(store) => Ok(Query::FlushO(store)),
            _ => Err(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_builds_search_query() {
        assert!(
            QueryBuilder::search("id1", "c:test:1", "b:test:1", "Michael Dake", 10, 20).is_ok()
        );
        assert!(QueryBuilder::search("id2", "c:test:1", "", "Michael Dake", 1, 0).is_err());
    }

    #[test]
    fn it_builds_suggest_query() {
        assert!(QueryBuilder::suggest("id1", "c:test:2", "b:test:2", "Micha", 5).is_ok());
        assert!(QueryBuilder::suggest("id2", "c:test:2", "", "Micha", 1).is_err());
    }

    #[test]
    fn it_builds_push_query() {
        assert!(QueryBuilder::push(
            "c:test:3",
            "b:test:3",
            "o:test:3",
            "My name is Michael Dake. I'm ordering in the US."
        )
        .is_ok());
        assert!(
            QueryBuilder::push("c:test:3", "", "o:test:3", "My name is Michael Dake.").is_err()
        );
    }

    #[test]
    fn it_builds_pop_query() {
        assert!(QueryBuilder::pop("c:test:4", "b:test:4", "o:test:4", "ordering US").is_ok());
        assert!(QueryBuilder::pop("c:test:4", "", "o:test:4", "ordering US").is_err());
    }

    #[test]
    fn it_builds_count_query() {
        assert!(QueryBuilder::count("c:test:5", None, None).is_ok());
        assert!(QueryBuilder::count("c:test:5", Some("b:test:5"), None).is_ok());
        assert!(QueryBuilder::count("c:test:5", Some("b:test:5"), Some("o:test:5")).is_ok());
        assert!(QueryBuilder::count("c:test:5", Some(""), Some("o:test:5")).is_err());
    }

    #[test]
    fn it_builds_flushc_query() {
        assert!(QueryBuilder::flushc("c:test:6").is_ok());
        assert!(QueryBuilder::flushc("").is_err());
    }

    #[test]
    fn it_builds_flushb_query() {
        assert!(QueryBuilder::flushb("c:test:7", "b:test:7").is_ok());
        assert!(QueryBuilder::flushb("c:test:7", "").is_err());
    }

    #[test]
    fn it_builds_flusho_query() {
        assert!(QueryBuilder::flusho("c:test:8", "b:test:8", "o:test:8").is_ok());
        assert!(QueryBuilder::flusho("c:test:8", "b:test:8", "").is_err());
    }
}
