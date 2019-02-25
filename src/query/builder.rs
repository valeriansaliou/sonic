// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use super::query::Query;
use crate::lexer::token::LexedTokensBuilder;
use crate::store::item::StoreItemBuilder;

pub struct QueryBuilder;

pub type QueryBuilderResult<'a> = Result<Query<'a>, ()>;

impl QueryBuilder {
    pub fn search<'a>(
        query_id: String,
        collection: &'a str,
        bucket: &'a str,
        terms: &'a str,
        limit: u16,
        offset: u32,
    ) -> QueryBuilderResult<'a> {
        match (
            StoreItemBuilder::from_depth_2(collection, bucket),
            LexedTokensBuilder::from(terms),
        ) {
            (Ok(store), Ok(text_lexed)) => {
                Ok(Query::Search(store, query_id, text_lexed, limit, offset))
            }
            _ => Err(()),
        }
    }

    pub fn suggest<'a>(
        query_id: String,
        collection: &'a str,
        bucket: &'a str,
        terms: &'a str,
    ) -> QueryBuilderResult<'a> {
        match (
            StoreItemBuilder::from_depth_2(collection, bucket),
            LexedTokensBuilder::from(terms),
        ) {
            (Ok(store), Ok(text_lexed)) => Ok(Query::Suggest(store, query_id, text_lexed)),
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
            LexedTokensBuilder::from(text),
        ) {
            (Ok(store), Ok(text_lexed)) => Ok(Query::Push(store, text_lexed)),
            _ => Err(()),
        }
    }

    pub fn pop<'a>(
        collection: &'a str,
        bucket: &'a str,
        object: &'a str,
    ) -> QueryBuilderResult<'a> {
        match StoreItemBuilder::from_depth_3(collection, bucket, object) {
            Ok(store) => Ok(Query::Pop(store)),
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
