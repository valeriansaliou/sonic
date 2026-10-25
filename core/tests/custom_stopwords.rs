// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

//! Feature: Custom stopwords

mod common;

use crate::common::*;

/// See <https://github.com/valeriansaliou/sonic/issues/300>.
#[test]
fn test_config_stopwords_deny() {
    init_logging();
    let executor = make_test_executor(|app_conf| {
        app_conf.stopwords.deny = Arc::new(["foobar"].into_iter().map(ToOwned::to_owned).collect());
    });

    exec!(
        executor -> PUSH "articles" "default" "article:1"
        "You can also prevent foobar from being indexed, if you really don’t \
        like the word or it comes up too frequently."
    );

    exec!(executor -> TRIGGER consolidate);

    {
        let response = exec!(executor -> QUERY "articles" "default" "foobar");
        assert_eq!(response, [] as [&str; 0]);
    }
}

/// See <https://github.com/valeriansaliou/sonic/issues/383>.
#[test]
fn test_minimum_term_idf() {
    init_logging();

    // NOTE: This is an implementation detail.
    fn bm25_idf(document_count: u64, document_frequency: u64) -> f32 {
        debug_assert!(
            document_frequency <= document_count,
            "{document_frequency} > {document_count}"
        );

        let n = document_count.max(document_frequency) as f32;
        let df = document_frequency as f32;

        (1.0 + (n - df + 0.5) / (df + 0.5)).ln()
    }

    let executor = make_test_executor(|app_conf| {
        app_conf.search.query_minimum_term_idf_default = 0.6931472;
        app_conf.search.query_minimum_term_idf_minimum_object_count = 4;

        // Disable stemming to make results more predictable.
        app_conf.normalization.stemming_enabled = false;
    });

    assert_eq!(bm25_idf(4, 3), 0.35667497); // foo
    assert_eq!(bm25_idf(4, 2), 0.6931472); // bar
    assert_eq!(bm25_idf(4, 1), 1.2039728); // baz

    exec!(executor -> PUSH "articles" "default" "article:1" "example");
    exec!(executor -> PUSH "articles" "default" "article:2" "foo");
    exec!(executor -> PUSH "articles" "default" "article:3" "foo bar");
    exec!(executor -> PUSH "articles" "default" "article:4" "foo bar baz");

    exec!(executor -> TRIGGER consolidate);

    {
        let response = exec!(executor -> QUERY "articles" "default" "foo bar baz");
        assert_eq!(response, ["article:4", "article:3"]);
    }
}
