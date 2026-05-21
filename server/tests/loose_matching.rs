// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

//! Feature: Loose matching

mod common;

use crate::common::*;

/// Search should be case-insensitive.
#[test]
fn test_search_is_case_insensitive() {
    init_logging();

    #[rustfmt::skip]
    let examples = [
        ("HeLLo", "hello"),
    ];

    for (n, (message, term)) in examples.into_iter().enumerate() {
        let executor = make_test_executor_with_id(n);

        exec!(executor -> PUSH "messages" "user:1" "chat:1" message);
        exec!(executor -> TRIGGER consolidate);

        let response = exec!(executor -> QUERY "messages" "user:1" term);
        assert_eq!(response, ["chat:1"]);
    }
}

/// Search should be diacritics-insensitive.
#[test]
fn test_search_is_diacritics_insensitive() {
    init_logging();

    #[rustfmt::skip]
    let examples = [
        ("Cinéma", "cinema"),
    ];

    for (n, (message, term)) in examples.into_iter().enumerate() {
        let executor = make_test_executor_with_id(n);

        exec!(executor -> PUSH "messages" "user:1" "chat:1" message);
        exec!(executor -> TRIGGER consolidate);

        let response = exec!(executor -> QUERY "messages" "user:1" term);
        assert_eq!(response, ["chat:1"]);
    }
}
