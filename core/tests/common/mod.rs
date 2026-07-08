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
}

macro_rules! exec {
    ($executor:ident -> PUSH $collection:tt $bucket:tt $oid:tt $text:tt $(LANG($lang:expr))?) => {{
        #[rustfmt::skip]
        $executor.log(format!(
            "PUSH {:?} {:?} {:?} {:?}{}",
            $collection, $bucket, $oid, $text, exec!(internal_ lang_txt $($lang)?)
        ));
        $executor
            .push(
                crate::common::object_ref!($collection, $bucket, $oid),
                sonic::lexer::TokenLexerBuilder::from(
                    sonic::lexer::TokenLexerMode::NormalizeAndCleanup,
                    exec!(internal_ lang $($lang)?),
                    $text,
                    $executor.app_conf.normalization,
                    $executor.app_conf.tokenization,
                )
                .unwrap(),
            )
            .unwrap()
    }};

    ($executor:ident -> TRIGGER consolidate) => {{
        $executor.log(format!("TRIGGER consolidate"));
        $executor.fst_pool.consolidate(true)
    }};

    ($executor:ident -> COUNT $collection:tt) => {{
        $executor.log(format!("COUNT {:?}", $collection));
        $executor.count(collection_ref!($collection))
    }};

    ($executor:ident -> COUNT $collection:tt $bucket:tt) => {{
        $executor.log(format!("COUNT {:?} {:?}", $collection, $bucket));
        $executor.count(bucket_ref!($collection, $bucket))
    }};

    ($executor:ident -> COUNT $collection:tt $bucket:tt $oid:tt) => {{
        $executor.log(format!("COUNT {:?} {:?} {:?}", $collection, $bucket, $oid));
        $executor.count(object_ref!($collection, $bucket, $oid))
    }};

    ($executor:ident -> QUERY $collection:tt $bucket:tt $term:tt $(LANG($lang:expr))? $(LIMIT($limit:expr))?) => {{
        #[rustfmt::skip]
        $executor.log(format!(
            "QUERY {:?} {:?} {:?}{}{}",
            $collection, $bucket, $term, exec!(internal_ lang_txt $($lang)?), exec!(internal_ limit_txt $($limit)?)
        ));
        $executor
            .search(
                crate::common::bucket_ref!($collection, $bucket),
                "",
                sonic::lexer::TokenLexerBuilder::from(
                    sonic::lexer::TokenLexerMode::NormalizeAndCleanup,
                    exec!(internal_ lang $($lang)?),
                    $term,
                    $executor.app_conf.normalization,
                    $executor.app_conf.tokenization,
                )
                .unwrap(),
                exec!(internal_ limit $($limit)?),
                0,
            )
            .expect("QUERY should succeed")
    }};

    ($executor:ident -> LIST $collection:tt $bucket:tt) => {{
        $executor.log(format!("LIST {:?} {:?}", $collection, $bucket));
        $executor
            .list(
                crate::common::bucket_ref!($collection, $bucket),
                "",
                sonic::query::QuerySearchLimit::MAX,
                0,
            )
            .expect("LIST should succeed")
    }};

    ($executor:ident -> FLUSHC $collection:tt) => {{
        $executor.log(format!("FLUSHC {:?}", $collection));
        $executor
            .flushc(crate::common::collection_ref!($collection))
            .unwrap()
    }};

    ($executor:ident -> FLUSHB $collection:tt $bucket:tt) => {{
        $executor.log(format!("FLUSHB {:?} {:?}", $collection, $bucket));
        $executor
            .flushb(crate::common::bucket_ref!($collection, $bucket))
            .unwrap()
    }};

    ($executor:ident -> FLUSHO $collection:tt $bucket:tt $oid:tt) => {{
        $executor.log(format!("FLUSHO {:?} {:?} {:?}", $collection, $bucket, $oid));
        $executor
            .flusho(crate::common::object_ref!($collection, $bucket, $oid))
            .unwrap()
    }};

    (internal_ lang) => { None };
    (internal_ lang $lang:expr) => { Some(whatlang::Lang::from_code($lang).unwrap()) };

    (internal_ lang_txt) => { "" };
    (internal_ lang_txt $lang:expr) => { format!(" LANG({})", $lang) };

    (internal_ limit) => { sonic::query::QuerySearchLimit::MAX };
    (internal_ limit $limit:expr) => { $limit };

    (internal_ limit_txt) => { "" };
    (internal_ limit_txt $limit:expr) => { format!(" LLIMIT({})", $limit) };
}
pub(crate) use exec;
