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
        ("ΚΌΣΜΟΣ", "κόσμος"),
        ("привет", "ПРИВЕТ"),
    ];

    for (n, (message, term)) in examples.into_iter().enumerate() {
        let executor = make_test_executor_with_id(n);

        exec!(executor -> PUSH "messages" "user:1" "chat:1" message);
        exec!(executor -> TRIGGER consolidate);

        let response = exec!(executor -> QUERY "messages" "user:1" term);
        assert_eq!(response, ["chat:1"], "({message:?}, {term:?})");
    }
}

/// Search should be diacritics-insensitive.
///
/// NOTE: In this test, we disable matching via prefix or Levenstein distance
///   to avoid false positives. For example, `cinema` used to match `cinéma`,
///   but it was because of typo correction and not because normalization was
///   done properly. This is likely why it went under the radar that up until
///   v1.6.0 Sonic was storing non-normalized words in the FST.
#[test]
fn test_search_is_diacritics_insensitive() {
    #[rustfmt::skip]
    test_ingest_then_query!(
        normalization_config: { diacritic_folding_enabled: true },
        search_config: { fuzzy_matching_enabled: false, prefix_matching_enabled: false },
        push: "Cinéma",
        query: [
            ("cinema", true),
        ],
    );

    // Example from <https://github.com/valeriansaliou/sonic/issues/245>.
    #[rustfmt::skip]
    test_ingest_then_query!(
        normalization_config: { diacritic_folding_enabled: true },
        search_config: { fuzzy_matching_enabled: false, prefix_matching_enabled: true },
        push: "Veronika Šibanová",
        query: [
            ("Sibanova", true),
            ("Veronika S", true),
        ],
    );
}
