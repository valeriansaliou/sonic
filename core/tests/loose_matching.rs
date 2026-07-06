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
fn test_search_is_case_insensitive_simple() {
    init_logging();

    let examples = [
        ("HeLLo", "hello"),
        ("ΚΌΣΜΟΣ", "κόσμος"),
        ("привет", "ПРИВЕТ"),
    ];

    // Make it work bidirectionally.
    let examples = examples.into_iter().flat_map(|(a, b)| [(a, b), (b, a)]);

    for (n, (message, term)) in examples.enumerate() {
        let executor = make_test_executor_with_id(n);

        exec!(executor -> PUSH "messages" "user:1" "chat:1" message);
        exec!(executor -> TRIGGER consolidate);

        let response = exec!(executor -> QUERY "messages" "user:1" term);
        assert_eq!(response, ["chat:1"], "({message:?}, {term:?})");
    }
}

/// Case-insensitivity isn’t just about lowercasing, as explained in
/// <https://www.w3.org/TR/charmod-norm/#definitionCaseFolding>.
/// This test ensures Sonic does proper case folding.
#[test]
fn test_search_is_case_insensitive_proper() {
    init_logging();

    let examples = [
        ("ΚΌΣΜΟΣ", "κόσμοσ"),
        ("ΚΌΣΜΟΣ", "κόσμος"),
        ("DİYARBAKIR", "diyarbakır"),
    ];

    // Make it work bidirectionally.
    let examples = examples.into_iter().flat_map(|(a, b)| [(a, b), (b, a)]);

    for (n, (message, term)) in examples.into_iter().enumerate() {
        let executor = make_test_executor_with_id(n);

        exec!(executor -> PUSH "messages" "user:1" "chat:1" message);
        exec!(executor -> TRIGGER consolidate);

        let response = exec!(executor -> QUERY "messages" "user:1" term);
        assert_eq!(response, ["chat:1"], "({message:?}, {term:?})");
    }
}

/// Unicode representation should not impact search results.
#[test]
#[ignore = "Not supported yet"]
fn test_search_is_unicode_normalized() {
    init_logging();

    let examples = [
        // Ligatures
        ("ﬃ", "ffi"),
        // Width folding
        ("ＡＢＣ", "ABC"),
        // Style
        ("①②③", "123"),
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
    test_ingest_then_query!(
        normalization_config: { diacritic_folding_enabled: true },
        search_config: { fuzzy_matching_enabled: false, prefix_matching_enabled: false },
        push: "Cinéma",
        query: [
            ("cinema", true),
        ],
    );

    // Example from <https://github.com/valeriansaliou/sonic/issues/245>.
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

#[test]
fn test_no_fuzzy_matching_for_ids() {
    init_logging();

    let examples = [
        // UUID.
        (
            vec![
                "My user ID is \"6db14cb4-b82e-4e49-8016-ef76c4290a2f\"",
                "My user ID is `6db14cb4-b82e-4e49-8016-ef76c4290a2e`",
            ],
            "6db14cb4-b82e-4e49-8016-ef76c4290a2f",
        ),
        // One term in the query (`abcd`) has only letters.
        (
            vec![
                "My user ID is 6db14cb4-abcd-4e49-8016-ef76c4290a2e",
                "My user ID is 6db14cb4-cdef-4e49-8016-ef76c4290a2e",
            ],
            "6db14cb4-abcd-4e49-8016-ef76c4290a2e",
        ),
        // Phone number like.
        (
            vec!["Here it is: 1234-567890-12", "Here it is: 1234-567890-13"],
            "1234-567890-12",
        ),
    ];

    for (example_idx, (messages, query)) in examples.into_iter().enumerate() {
        let executor = make_test_executor_with_id(example_idx);

        for (message_idx, &message) in messages.iter().enumerate() {
            let id = &format!("chat:{message_idx}");
            exec!(executor -> PUSH "messages" "user:1" id message);
        }
        exec!(executor -> TRIGGER consolidate);

        let response = exec!(executor -> QUERY "messages" "user:1" query);

        assert_eq!(response, ["chat:0"], "({messages:?}, {query:?})");
    }
}
