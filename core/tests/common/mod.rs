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
        let (c, b, o) = crate::common::object_ref!($collection, $bucket, $oid);
        let preprocessor = sonic::lexer::preprocessor::Preprocessor::new(
            $executor.app_conf.tokenization,
            $executor.app_conf.normalization,
            $executor.app_conf.stopwords.clone(),
            true,
            true,
        );
        $executor
            .push(
                c, b, o,
                preprocessor.preprocess($text, exec!(internal_ lang $($lang)?)),
                false,
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
        $executor
            .search(
                c, b,
                "",
                preprocessor.preprocess($term, exec!(internal_ lang $($lang)?)),
                exec!(internal_ limit $($limit)?),
                0,
            )
            .expect("QUERY should succeed")
    }};

    ($executor:ident -> LIST $collection:tt $bucket:tt $(LIMIT($limit:expr))?) => {{
        $executor.log(format!("LIST {:?} {:?}", $collection, $bucket));
        let (c, b) = crate::common::bucket_ref!($collection, $bucket);
        $executor
            .list(
                c, b,
                "",
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

    (internal_ lang) => { None };
    (internal_ lang $lang:expr) => { whatlang::Lang::from_code($lang) };

    (internal_ lang_txt) => { "" };
    (internal_ lang_txt $lang:expr) => { format!(" LANG({})", $lang) };

    (internal_ limit) => { sonic::query::QuerySearchLimit::MAX };
    (internal_ limit $limit:expr) => { $limit };

    (internal_ limit_txt) => { "" };
    (internal_ limit_txt $limit:expr) => { format!(" LLIMIT({})", $limit) };
}
pub(crate) use exec;
