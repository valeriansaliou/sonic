// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

//! Feature: Language detection

mod common;

use crate::common::*;

/// Search should be language-aware.
#[test]
#[ignore = "Known issue (FIXME)"]
fn test_search_language_aware() {
    init_logging();

    #[rustfmt::skip]
    let examples = [
        ("J’ai envie de boire un thé", "FRA", "the", "ENG"),
    ];

    for (n, (message, lang, stopword, stopword_lang)) in examples.into_iter().enumerate() {
        let executor = make_test_executor_with_id(n);

        exec!(executor -> PUSH "messages" "user:1" "chat:1" message LANG(lang));
        exec!(executor -> TRIGGER consolidate);

        let response = exec!(executor -> QUERY "messages" "user:1" stopword LANG(lang));
        assert_eq!(response, ["chat:1"]);

        let response = exec!(executor -> QUERY "messages" "user:1" stopword LANG(stopword_lang));
        assert_eq!(response, [] as [&str; 0]);
    }
}

/// Language is case-insensitive.
#[test]
fn test_lang_is_case_insensitive() {
    init_logging();

    init_logging();

    #[rustfmt::skip]
    let examples = [
        ("Allons au cinéma", "FRA"),
        ("Allons au cinéma", "fra"),
    ];

    for (n, (message, lang)) in examples.into_iter().enumerate() {
        let executor = make_test_executor_with_id(n);

        exec!(executor -> PUSH "messages" "user:1" "chat:1" message LANG(lang));
        exec!(executor -> TRIGGER consolidate);

        let response = exec!(executor -> QUERY "messages" "user:1" message LANG(lang));
        assert_eq!(response, ["chat:1"]);
    }
}

/// “none” is a special language.
#[test]
#[ignore]
fn test_none_is_special_lang() {
    init_logging();

    unimplemented!()
}
