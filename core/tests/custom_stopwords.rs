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
fn test_config_stopwords_allow() {
    init_logging();
    let executor = make_test_executor(|app_conf| {
        // NOTE: In Sonic `<= 1.7.4` (and possibly until `2.0.0`), `"microsoft"`
        //   was a hard-coded stopword in `ENG`. That wasn’t intended.
        app_conf.stopwords.allow = ["microsoft"].into_iter().map(ToOwned::to_owned).collect();
    });

    exec!(
        executor -> PUSH "articles" "default" "article:1"
        "Microsoft shouldn’t be a stopword."
    );
    exec!(
        executor -> PUSH "articles" "default" "article:2"
        "But now there’s a way to allow “Microsoft” to be indexed :)"
    );

    exec!(executor -> TRIGGER consolidate);

    {
        let response = exec!(executor -> QUERY "articles" "default" "Microsoft");
        assert_eq!(response, ["article:2", "article:1"]);
    }
}

/// See <https://github.com/valeriansaliou/sonic/issues/300>.
#[test]
fn test_config_stopwords_deny() {
    init_logging();
    let executor = make_test_executor(|app_conf| {
        app_conf.stopwords.deny = ["foobar"].into_iter().map(ToOwned::to_owned).collect();
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
    let executor = make_test_executor(|app_conf| {
        app_conf.search.query_minimum_term_idf_default = 0.5;
        app_conf.search.query_minimum_term_idf_minimum_object_count = 4;

        // Disable stemming to make results more predictable.
        app_conf.normalization.stemming_enabled = false;
    });

    // idf("foo") = 0.75, idf("bar") = 0.50, idf("baz") = 0.25
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
