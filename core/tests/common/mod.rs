// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

#![allow(unused)]

pub mod config;
mod executor;
mod logging;
mod meta_tests;
pub mod util;

pub use std::sync::Arc;

pub use self::executor::*;
pub(crate) use self::item_ref::*;
pub use self::logging::*;
pub(crate) use self::meta_tests::*;
pub(crate) use self::util::assert_contains;

// NOTE: Using macros instead of functions so `unwrap`s point to the call site
//   (helps debugging).
pub(crate) mod item_ref {
    macro_rules! collection_ref {
        ($collection:expr) => {
            sonic::store::StoreItemBuilder::from_depth_1($collection).unwrap()
        };
    }
    pub(crate) use collection_ref;

    macro_rules! bucket_ref {
        ($collection:expr, $bucket:expr) => {
            sonic::store::StoreItemBuilder::from_depth_2($collection, $bucket).unwrap()
        };
    }
    pub(crate) use bucket_ref;

    macro_rules! object_ref {
        ($collection:expr, $bucket:expr, $object:expr) => {
            sonic::store::StoreItemBuilder::from_depth_3($collection, $bucket, $object).unwrap()
        };
    }
    pub(crate) use object_ref;

    macro_rules! fragment_ref {
        ($collection:expr, $bucket:expr, $object:expr, $fragment:expr) => {
            sonic::store::StoreItemBuilder::from_depth_3_or_4(
                $collection,
                $bucket,
                $object,
                $fragment,
            )
            .unwrap()
        };
    }
    pub(crate) use fragment_ref;
}

macro_rules! exec {
    ($executor:ident -> PUSH $collection:tt $bucket:tt $oid:tt $fragment:tt $text:tt $(LANG($lang:expr))? $(; $($standalone_meta:ident)+)?) => {
        exec!(internal_ push $executor -> PUSH $collection $bucket $oid {Some($fragment)} $text $(LANG($lang))? $(; $($standalone_meta)+)?)
    };

    ($executor:ident -> PUSH $collection:tt $bucket:tt $oid:tt $text:tt $(LANG($lang:expr))? $(; $($standalone_meta:ident)+)?) => {
        exec!(internal_ push $executor -> PUSH $collection $bucket $oid None $text $(LANG($lang))? $(; $($standalone_meta)+)?)
    };

    (internal_ push $executor:ident -> PUSH $collection:tt $bucket:tt $oid:tt $fragment:tt $text:tt $(LANG($lang:expr))? $(; $($standalone_meta:ident)+)?) => {{
        let mut additional_meta = concat!($($(" ", stringify!($standalone_meta),)+)? " ");

        let is_incomplete = additional_meta.contains(" INCOMPLETE ");

        // Remove additional space added for search purposes.
        additional_meta = &additional_meta[..(additional_meta.len() - 1)];

        #[rustfmt::skip]
        $executor.log(format!(
            "PUSH {:?} {:?} {:?}{} {:?}{}{additional_meta}",
            $collection, $bucket, $oid, exec!(internal_ fragment_txt $fragment), $text, exec!(internal_ lang_txt $($lang)?),
        ));
        let (c, b, o, f) = crate::common::fragment_ref!($collection, $bucket, $oid, $fragment);
        let preprocessor = sonic::lexer::preprocessor::Preprocessor::new(
            $executor.app_conf.tokenization,
            $executor.app_conf.normalization,
            $executor.app_conf.stopwords.clone(),
            true,
            true,
        );
        let options = sonic::executor::PushOptions {
            is_incomplete,
            ..Default::default()
        };

        $executor
            .push(
                c, b, o, f,
                preprocessor.preprocess($text, exec!(internal_ lang $($lang)?)),
                options,
            )
            .unwrap()
    }};

    ($executor:ident -> TRIGGER consolidate) => {{
        $executor.log(format!("TRIGGER consolidate"));
        $executor.fst_pool.consolidate(true, |_| true)
    }};

    ($executor:ident -> COUNT $collection:tt) => {{
        $executor.log(format!("COUNT {:?}", $collection));
        let c = collection_ref!($collection);
        $executor.countc(c)
    }};

    ($executor:ident -> COUNT $collection:tt $bucket:tt) => {{
        $executor.log(format!("COUNT {:?} {:?}", $collection, $bucket));
        let (c, b) = bucket_ref!($collection, $bucket);
        $executor.legacy_countb(c, b)
    }};

    ($executor:ident -> COUNTB $collection:tt $bucket:tt) => {{
        $executor.log(format!("COUNTB {:?} {:?}", $collection, $bucket));
        let (c, b) = bucket_ref!($collection, $bucket);
        $executor.countb(c, b)
    }};

    ($executor:ident -> COUNT $collection:tt $bucket:tt $oid:tt) => {{
        $executor.log(format!("COUNT {:?} {:?} {:?}", $collection, $bucket, $oid));
        let (c, b, o) = object_ref!($collection, $bucket, $oid);
        $executor.counto(c, b, o)
    }};

    ($executor:ident -> QUERY $collection:tt $bucket:tt $term:tt $(LANG($lang:expr))? $(LIMIT($limit:expr))?) => {{
        #[rustfmt::skip]
        $executor.log(format!(
            "QUERY {:?} {:?} {:?}{}{}",
            $collection, $bucket, $term, exec!(internal_ lang_txt $($lang)?), exec!(internal_ limit_txt $($limit)?)
        ));
        let (c, b) = crate::common::bucket_ref!($collection, $bucket);
        let preprocessor = sonic::lexer::preprocessor::Preprocessor::new(
            $executor.app_conf.tokenization,
            $executor.app_conf.normalization,
            $executor.app_conf.stopwords.clone(),
            true,
            true,
        );

        let input = preprocessor.preprocess($term, exec!(internal_ lang $($lang)?));

        let query_options = sonic::executor::QueryOptions {
            limit: exec!(internal_ limit $($limit)?),
            offset: 0,
        };

        $executor
            .query(c, b, input, query_options)
            .expect("QUERY should succeed")
            .collect::<Vec<_>>()
    }};

    (internal_ set_snippets_ord SNIPPETS $cfg:ident) => {};
    (internal_ set_snippets_ord SNIPPETS_REV $cfg:ident) => { $cfg.ordering = sonic::store::kv::Ordering::IngestionOrder; };

    ($executor:ident -> WITH $snippets_keyword:ident$(($snip_wc:expr$(, $snip_ws:expr$(, $snip_wp:expr)?)?))? QUERY $collection:tt $bucket:tt $term:tt $(LANG($lang:expr))? $(LIMIT($limit:expr))?) => {{
        let additional_meta = concat!(" WITH ", stringify!($snippets_keyword)$(, "(", $snip_wc, $(",", $snip_ws, $(",", $snip_wp,)?)? ")")?);

        #[rustfmt::skip]
        $executor.log(format!(
            "QUERY {:?} {:?} {:?}{}{}{additional_meta}",
            $collection, $bucket, $term, exec!(internal_ lang_txt $($lang)?), exec!(internal_ limit_txt $($limit)?)
        ));
        let (c, b) = crate::common::bucket_ref!($collection, $bucket);
        let preprocessor = sonic::lexer::preprocessor::Preprocessor::new(
            $executor.app_conf.tokenization,
            $executor.app_conf.normalization,
            $executor.app_conf.stopwords.clone(),
            true,
            true,
        );

        let lang = exec!(internal_ lang $($lang)?);
        let input = preprocessor.preprocess($term, lang);

        let query_options = sonic::executor::QueryOptions {
            limit: exec!(internal_ limit $($limit)?),
            offset: 0,
        };

        #[allow(unused_mut)]
        let mut snippets_config = sonic::executor::QuerySnippetsConfig {
            $(window_count: $snip_wc,
            $(window_size: $snip_ws,
            $(max_window_padding: $snip_wp,)?)?)?
            ..Default::default()
        };
        exec!(internal_ set_snippets_ord $snippets_keyword snippets_config);

        $executor
            .query_with_snippets(c, b, input, query_options, lang, &snippets_config)
            .expect("QUERY should succeed")
    }};

    ($executor:ident -> SUGGEST $collection:tt $bucket:tt $word:tt $(LANG($lang:expr))?) => {{
        #[rustfmt::skip]
        $executor.log(format!(
            "SUGGEST {:?} {:?} {:?}{}",
            $collection, $bucket, $word, exec!(internal_ lang_txt $($lang)?)
        ));
        let (c, b) = crate::common::bucket_ref!($collection, $bucket);
        let preprocessor = sonic::lexer::preprocessor::Preprocessor::new(
            $executor.app_conf.tokenization,
            $executor.app_conf.normalization,
            $executor.app_conf.stopwords.clone(),
            false,
            false,
        );
        $executor
            .suggest(
                c, b,
                preprocessor.preprocess($word, exec!(internal_ lang $($lang)?)),
                $executor.app_conf.search.suggest_limit_default,
            )
            .expect("SUGGEST should succeed")
            .map(|words| words.collect::<Vec<String>>())
            .unwrap_or_default()
    }};

    ($executor:ident -> LIST $collection:tt $bucket:tt $(LIMIT($limit:expr))?) => {{
        $executor.log(format!("LIST {:?} {:?}", $collection, $bucket));
        let (c, b) = crate::common::bucket_ref!($collection, $bucket);
        $executor
            .list(
                c, b,
                exec!(internal_ limit $($limit)?),
                0,
            )
            .expect("LIST should succeed")
    }};

    ($executor:ident -> FLUSHC $collection:tt) => {{
        $executor.log(format!("FLUSHC {:?}", $collection));
        let c = crate::common::collection_ref!($collection);
        $executor.flushc(c).unwrap()
    }};

    ($executor:ident -> FLUSHB $collection:tt $bucket:tt) => {{
        $executor.log(format!("FLUSHB {:?} {:?}", $collection, $bucket));
        let (c, b) = crate::common::bucket_ref!($collection, $bucket);
        $executor.flushb(c, b).unwrap()
    }};

    ($executor:ident -> FLUSHO $collection:tt $bucket:tt $oid:tt) => {{
        $executor.log(format!("FLUSHO {:?} {:?} {:?}", $collection, $bucket, $oid));
        let (c, b, o) = crate::common::object_ref!($collection, $bucket, $oid);
        $executor.flusho(c, b, o).unwrap()
    }};

    (internal_ fragment_txt None) => { "" };
    (internal_ fragment_txt {Some($fragment:expr)}) => { format!(" {}", $fragment) };

    (internal_ lang) => { None };
    (internal_ lang $lang:expr) => { whatlang::Lang::from_code($lang) };

    (internal_ lang_txt) => { "" };
    (internal_ lang_txt $lang:expr) => { format!(" LANG({})", $lang) };

    (internal_ limit) => { sonic::executor::QuerySearchLimit::MAX };
    (internal_ limit $limit:expr) => { $limit };

    (internal_ limit_txt) => { "" };
    (internal_ limit_txt $limit:expr) => { format!(" LLIMIT({})", $limit) };
}
pub(crate) use exec;
