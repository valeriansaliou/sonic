// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use sonic::config::{ConfigNormalization, ConfigStopwords, ConfigTokenization};
use sonic::executor::{QueryGenericLang, QuerySearchLimit, QuerySearchOffset};
use sonic::lexer::preprocessor::Preprocessor;
use sonic::lexer::to_rework::TokenLexerMode;
use sonic::store::StoreItemBuilder;

use super::Query;

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
        stopwords_config: &'a ConfigStopwords,
    ) -> Result<Self, ()> {
        let should_cleanup = TokenLexerMode::from_query_lang(&lang).should_cleanup();
        let preprocessor = Preprocessor::new(
            tokenization_config,
            normalization_config,
            stopwords_config.clone(),
            should_cleanup,
            should_cleanup,
        );

        match StoreItemBuilder::from_depth_2(collection, bucket) {
            Ok((c, b)) => {
                let text_lexed =
                    preprocessor.preprocess(terms, lang.and_then(QueryGenericLang::into_lang_opt));
                Ok(Query::Search(c, b, query_id, text_lexed, limit, offset))
            }
            Err(_err) => Err(()),
        }
    }

    #[allow(clippy::too_many_arguments)] // This will be reworked at some point.
    pub fn suggest(
        query_id: &'a str,
        collection: &'a str,
        bucket: &'a str,
        terms: &'a str,
        limit: QuerySearchLimit,
        normalization_config: ConfigNormalization,
        tokenization_config: ConfigTokenization,
        stopwords_config: &'a ConfigStopwords,
    ) -> Result<Self, ()> {
        let preprocessor = Preprocessor::new(
            tokenization_config,
            normalization_config,
            stopwords_config.clone(),
            false,
            false,
        );

        match StoreItemBuilder::from_depth_2(collection, bucket) {
            Ok((c, b)) => {
                let text_lexed = preprocessor.preprocess(terms, None);
                Ok(Query::Suggest(c, b, query_id, text_lexed, limit))
            }
            Err(_err) => Err(()),
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
            Ok((c, b)) => Ok(Query::List(c, b, query_id, limit, offset)),
            _ => Err(()),
        }
    }

    #[allow(clippy::too_many_arguments)] // This will be reworked at some point.
    pub fn push(
        collection: &'a str,
        bucket: &'a str,
        object: &'a str,
        text: &'a str,
        lang: Option<QueryGenericLang>,
        assume_new: bool,
        normalization_config: ConfigNormalization,
        tokenization_config: ConfigTokenization,
        stopwords_config: &'a ConfigStopwords,
    ) -> Result<Self, ()> {
        let should_cleanup = TokenLexerMode::from_query_lang(&lang).should_cleanup();
        let preprocessor = Preprocessor::new(
            tokenization_config,
            normalization_config,
            stopwords_config.clone(),
            should_cleanup,
            should_cleanup,
        );

        match StoreItemBuilder::from_depth_3(collection, bucket, object) {
            Ok((c, b, o)) => {
                let text_lexed =
                    preprocessor.preprocess(text, lang.and_then(QueryGenericLang::into_lang_opt));
                Ok(Query::Push(c, b, o, text_lexed, assume_new))
            }
            Err(_err) => Err(()),
        }
    }

    pub fn pop(
        collection: &'a str,
        bucket: &'a str,
        object: &'a str,
        text: &'a str,
        normalization_config: ConfigNormalization,
        tokenization_config: ConfigTokenization,
        stopwords_config: &'a ConfigStopwords,
    ) -> Result<Self, ()> {
        let preprocessor = Preprocessor::new(
            tokenization_config,
            normalization_config,
            stopwords_config.clone(),
            false,
            false,
        );

        match StoreItemBuilder::from_depth_3(collection, bucket, object) {
            Ok((c, b, o)) => {
                let text_lexed = preprocessor.preprocess(text, None);
                Ok(Query::Pop(c, b, o, text_lexed))
            }
            Err(_err) => Err(()),
        }
    }

    pub fn count(
        collection: &'a str,
        bucket: Option<&'a str>,
        object: Option<&'a str>,
    ) -> Result<Self, ()> {
        match (bucket, object) {
            (Some(bucket_inner), Some(object_inner)) => {
                StoreItemBuilder::from_depth_3(collection, bucket_inner, object_inner)
                    .map(|(c, b, o)| Query::Count(c, Some(b), Some(o)))
            }
            (Some(bucket_inner), None) => StoreItemBuilder::from_depth_2(collection, bucket_inner)
                .map(|(c, b)| Query::Count(c, Some(b), None)),
            _ => StoreItemBuilder::from_depth_1(collection).map(|c| Query::Count(c, None, None)),
        }
        .map_err(|error| tracing::warn!("Invalid count request: {error:?}"))
    }

    pub fn flushc(collection: &'a str) -> Result<Self, ()> {
        match StoreItemBuilder::from_depth_1(collection) {
            Ok(c) => Ok(Query::FlushC(c)),
            _ => Err(()),
        }
    }

    pub fn flushb(collection: &'a str, bucket: &'a str) -> Result<Self, ()> {
        match StoreItemBuilder::from_depth_2(collection, bucket) {
            Ok((c, b)) => Ok(Query::FlushB(c, b)),
            _ => Err(()),
        }
    }

    pub fn flusho(collection: &'a str, bucket: &'a str, object: &'a str) -> Result<Self, ()> {
        match StoreItemBuilder::from_depth_3(collection, bucket, object) {
            Ok((c, b, o)) => Ok(Query::FlushO(c, b, o)),
            _ => Err(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use super::*;

    const NORMALIZATION_CONFIG: ConfigNormalization = ConfigNormalization {
        unicode_normalization: None,
        diacritic_folding_enabled: false,
        stemming_enabled: false,
    };
    const TOKENIZATION_CONFIG: ConfigTokenization = ConfigTokenization {
        detect_special_patterns: true,
        compat_split_special_patterns: false,
    };
    static STOPWORDS_CONFIG: LazyLock<ConfigStopwords> = LazyLock::new(|| ConfigStopwords {
        allow: Default::default(),
        deny: Default::default(),
    });

    #[test]
    fn it_builds_search_query() {
        #[rustfmt::skip]
        assert!(Query::search(
            "id1", "c:test:1", "b:test:1", "Michael Dake", 10, 20, None,
            NORMALIZATION_CONFIG, TOKENIZATION_CONFIG, &STOPWORDS_CONFIG,
        ).is_ok());

        #[rustfmt::skip]
        assert!(Query::search(
            "id2", "c:test:1", "", "Michael Dake", 1, 0, None,
            NORMALIZATION_CONFIG, TOKENIZATION_CONFIG, &STOPWORDS_CONFIG,
        ).is_err());
    }

    #[test]
    fn it_builds_suggest_query() {
        #[rustfmt::skip]
        assert!(Query::suggest(
            "id1", "c:test:2", "b:test:2", "Micha", 5,
            NORMALIZATION_CONFIG, TOKENIZATION_CONFIG, &STOPWORDS_CONFIG,
        ).is_ok());

        #[rustfmt::skip]
        assert!(Query::suggest(
            "id2", "c:test:2", "", "Micha", 1,
            NORMALIZATION_CONFIG, TOKENIZATION_CONFIG, &STOPWORDS_CONFIG,
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
            "c:test:3", "b:test:3", "o:test:3", "My name is Michael Dake. I'm ordering in the US.", None, false,
            NORMALIZATION_CONFIG, TOKENIZATION_CONFIG, &STOPWORDS_CONFIG,
        ).is_ok());

        #[rustfmt::skip]
        assert!(Query::push(
            "c:test:3", "", "o:test:3", "My name is Michael Dake.", None, false,
            NORMALIZATION_CONFIG, TOKENIZATION_CONFIG, &STOPWORDS_CONFIG,
        ).is_err());
    }

    #[test]
    fn it_builds_pop_query() {
        #[rustfmt::skip]
        assert!(Query::pop(
            "c:test:4", "b:test:4", "o:test:4", "ordering US",
            NORMALIZATION_CONFIG, TOKENIZATION_CONFIG, &STOPWORDS_CONFIG,
        ).is_ok());

        #[rustfmt::skip]
        assert!(Query::pop(
            "c:test:4", "", "o:test:4", "ordering US",
            NORMALIZATION_CONFIG, TOKENIZATION_CONFIG, &STOPWORDS_CONFIG,
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
