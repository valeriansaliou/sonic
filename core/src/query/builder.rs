// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use super::Query;
use super::types::{QueryGenericLang, QuerySearchLimit, QuerySearchOffset, QueryTimeRange};
use crate::config::{ConfigNormalization, ConfigTokenization};
use crate::lexer::{TokenLexerBuilder, TokenLexerMode};
use crate::store::StoreItemBuilder;
use crate::store::document::StoreDocument;

impl<'a> Query<'a> {
    #[allow(clippy::too_many_arguments)] // This will be reworked at some point.
    pub fn search(
        query_id: &'a str,
        collection: &'a str,
        bucket: &'a str,
        terms: &'a str,
        limit: QuerySearchLimit,
        offset: QuerySearchOffset,
        lang: Option<QueryGenericLang>,
        normalization_config: ConfigNormalization,
        tokenization_config: ConfigTokenization,
    ) -> Result<Self, ()> {
        Self::search_with_range(
            query_id,
            collection,
            bucket,
            terms,
            limit,
            offset,
            lang,
            None,
            normalization_config,
            tokenization_config,
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn search_with_range(
        query_id: &'a str,
        collection: &'a str,
        bucket: &'a str,
        terms: &'a str,
        limit: QuerySearchLimit,
        offset: QuerySearchOffset,
        lang: Option<QueryGenericLang>,
        time_range: Option<QueryTimeRange>,
        normalization_config: ConfigNormalization,
        tokenization_config: ConfigTokenization,
        documents: bool,
    ) -> Result<Self, ()> {
        match (
            StoreItemBuilder::from_depth_2(collection, bucket),
            TokenLexerBuilder::from(
                TokenLexerMode::from_query_lang(&lang),
                lang.and_then(QueryGenericLang::into_lang_opt),
                terms,
                normalization_config,
                tokenization_config,
            ),
        ) {
            (Ok(store), Ok(text_lexed)) if documents => Ok(Query::SearchDocuments(
                store, query_id, text_lexed, limit, offset, time_range,
            )),
            (Ok(store), Ok(text_lexed)) => Ok(Query::Search(
                store, query_id, text_lexed, limit, offset, time_range,
            )),
            _ => Err(()),
        }
    }

    pub fn list(
        query_id: &'a str,
        collection: &'a str,
        bucket: &'a str,
        limit: QuerySearchLimit,
        offset: QuerySearchOffset,
    ) -> Result<Self, ()> {
        match StoreItemBuilder::from_depth_2(collection, bucket) {
            Ok(store) => Ok(Query::List(store, query_id, limit, offset)),
            _ => Err(()),
        }
    }

    pub fn push(
        collection: &'a str,
        bucket: &'a str,
        object: &'a str,
        text: &'a str,
        lang: Option<QueryGenericLang>,
        normalization_config: ConfigNormalization,
        tokenization_config: ConfigTokenization,
    ) -> Result<Self, ()> {
        match (
            StoreItemBuilder::from_depth_3(collection, bucket, object),
            TokenLexerBuilder::from(
                TokenLexerMode::from_query_lang(&lang),
                lang.and_then(QueryGenericLang::into_lang_opt),
                text,
                normalization_config,
                tokenization_config,
            ),
        ) {
            (Ok(store), Ok(text_lexed)) => Ok(Query::Push(store, text_lexed)),
            _ => Err(()),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert(
        collection: &'a str,
        bucket: &'a str,
        object: &'a str,
        text: &'a str,
        timestamp_ms: u64,
        metadata: serde_json::Value,
        lang: Option<QueryGenericLang>,
        normalization_config: ConfigNormalization,
        tokenization_config: ConfigTokenization,
    ) -> Result<Self, ()> {
        let document = StoreDocument::new(object, timestamp_ms, text, metadata)?;
        match (
            StoreItemBuilder::from_depth_3(collection, bucket, object),
            TokenLexerBuilder::from(
                TokenLexerMode::from_query_lang(&lang),
                lang.and_then(QueryGenericLang::into_lang_opt),
                text,
                normalization_config,
                tokenization_config,
            ),
        ) {
            (Ok(store), Ok(text_lexed)) => Ok(Query::Upsert(store, text_lexed, document)),
            _ => Err(()),
        }
    }

    pub fn pop(
        collection: &'a str,
        bucket: &'a str,
        object: &'a str,
        text: &'a str,
        normalization_config: ConfigNormalization,
        tokenization_config: ConfigTokenization,
    ) -> Result<Self, ()> {
        match (
            StoreItemBuilder::from_depth_3(collection, bucket, object),
            TokenLexerBuilder::from(
                TokenLexerMode::NormalizeOnly,
                None,
                text,
                normalization_config,
                tokenization_config,
            ),
        ) {
            (Ok(store), Ok(text_lexed)) => Ok(Query::Pop(store, text_lexed)),
            _ => Err(()),
        }
    }

    pub fn count(
        collection: &'a str,
        bucket: Option<&'a str>,
        object: Option<&'a str>,
    ) -> Result<Self, ()> {
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

    pub fn flushc(collection: &'a str) -> Result<Self, ()> {
        match StoreItemBuilder::from_depth_1(collection) {
            Ok(store) => Ok(Query::FlushC(store)),
            _ => Err(()),
        }
    }

    pub fn flushb(collection: &'a str, bucket: &'a str) -> Result<Self, ()> {
        match StoreItemBuilder::from_depth_2(collection, bucket) {
            Ok(store) => Ok(Query::FlushB(store)),
            _ => Err(()),
        }
    }

    pub fn flusho(collection: &'a str, bucket: &'a str, object: &'a str) -> Result<Self, ()> {
        match StoreItemBuilder::from_depth_3(collection, bucket, object) {
            Ok(store) => Ok(Query::FlushO(store)),
            _ => Err(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NORMALIZATION_CONFIG: ConfigNormalization = ConfigNormalization {
        diacritic_folding_enabled: false,
        stemming_enabled: false,
    };
    const TOKENIZATION_CONFIG: ConfigTokenization = ConfigTokenization {
        detect_special_patterns: true,
        compat_split_special_patterns: false,
    };

    #[test]
    fn it_builds_search_query() {
        #[rustfmt::skip]
        assert!(Query::search(
            "id1", "c:test:1", "b:test:1", "Michael Dake", 10, 20, None,
            NORMALIZATION_CONFIG, TOKENIZATION_CONFIG,
        ).is_ok());

        #[rustfmt::skip]
        assert!(Query::search(
            "id2", "c:test:1", "", "Michael Dake", 1, 0, None,
            NORMALIZATION_CONFIG, TOKENIZATION_CONFIG,
        ).is_err());
    }

    #[test]
    fn it_builds_list_query() {
        assert!(Query::list("id1", "c:test:2", "b:test:2", 100, 0).is_ok());
        assert!(Query::list("id2", "c:test:2", "", 10, 0).is_err());
    }

    #[test]
    fn it_builds_push_query() {
        #[rustfmt::skip]
        assert!(Query::push(
            "c:test:3", "b:test:3", "o:test:3", "My name is Michael Dake. I'm ordering in the US.", None,
            NORMALIZATION_CONFIG, TOKENIZATION_CONFIG,
        ).is_ok());

        #[rustfmt::skip]
        assert!(Query::push(
            "c:test:3", "", "o:test:3", "My name is Michael Dake.", None,
            NORMALIZATION_CONFIG, TOKENIZATION_CONFIG,
        ).is_err());
    }

    #[test]
    fn it_builds_pop_query() {
        #[rustfmt::skip]
        assert!(Query::pop(
            "c:test:4", "b:test:4", "o:test:4", "ordering US",
            NORMALIZATION_CONFIG, TOKENIZATION_CONFIG,
        ).is_ok());

        #[rustfmt::skip]
        assert!(Query::pop(
            "c:test:4", "", "o:test:4", "ordering US",
            NORMALIZATION_CONFIG, TOKENIZATION_CONFIG,
        ).is_err());
    }

    #[test]
    fn it_builds_count_query() {
        assert!(Query::count("c:test:5", None, None).is_ok());
        assert!(Query::count("c:test:5", Some("b:test:5"), None).is_ok());
        assert!(Query::count("c:test:5", Some("b:test:5"), Some("o:test:5")).is_ok());
        assert!(Query::count("c:test:5", Some(""), Some("o:test:5")).is_err());
    }

    #[test]
    fn it_builds_flushc_query() {
        assert!(Query::flushc("c:test:6").is_ok());
        assert!(Query::flushc("").is_err());
    }

    #[test]
    fn it_builds_flushb_query() {
        assert!(Query::flushb("c:test:7", "b:test:7").is_ok());
        assert!(Query::flushb("c:test:7", "").is_err());
    }

    #[test]
    fn it_builds_flusho_query() {
        assert!(Query::flusho("c:test:8", "b:test:8", "o:test:8").is_ok());
        assert!(Query::flusho("c:test:8", "b:test:8", "").is_err());
    }
}
