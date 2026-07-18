// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

//! Feature: Language detection

mod common;

use crate::common::*;

/// Language hints should not remove searchable terms.
#[test]
fn test_language_hints_preserve_terms() {
    let sentence = "J’ai envie de boire un thé";

    #[rustfmt::skip]
    test_ingest_then_query!(
        search_config: { fuzzy_matching_enabled: false, prefix_matching_enabled: false },
        push: sentence LANG("fra"),
        query: [
            ("the", true),
        ] LANG("fra"),
    );

    #[rustfmt::skip]
    test_ingest_then_query!(
        search_config: { fuzzy_matching_enabled: false, prefix_matching_enabled: false },
        push: sentence LANG("fra"),
        query: [
            ("the", true),
        ] LANG("eng"),
    );
}

/// Language is case-insensitive.
#[test]
fn test_lang_is_case_insensitive() {
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
