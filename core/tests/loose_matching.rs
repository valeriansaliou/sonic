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

    struct TestMatches1st<'a> {
        messages: Vec<&'a str>,
        query: &'a str,
    }

    // In those examples, multiple messages are pushed into the index,
    // and only the first is supposed to match the given query.
    let examples = [
        // UUID like.
        TestMatches1st {
            messages: vec![
                "My user ID is \"6db14cb4-b82e-4e49-8016-ef76c4290a2f\"",
                "My user ID is `6db14cb4-b82e-4e49-8016-ef76c4290a2e`",
            ],
            query: "6db14cb4-b82e-4e49-8016-ef76c4290a2f",
        },
        TestMatches1st {
            // Note that one term in the query (`abcd`) has only letters.
            messages: vec![
                "My user ID is 6db14cb4-abcd-4e49-8016-ef76c4290a2e",
                "My user ID is 6db14cb4-cdef-4e49-8016-ef76c4290a2e",
            ],
            query: "6db14cb4-abcd-4e49-8016-ef76c4290a2e",
        },
        // Phone number like.
        TestMatches1st {
            messages: vec!["Here it is: 1234-567890-12", "Here it is: 1234-567890-13"],
            query: "1234-567890-12",
        },
        TestMatches1st {
            messages: vec!["Here it is: 0123456789", "Here it is: 0123456780"],
            query: "0123456789",
        },
        // Email address.
        TestMatches1st {
            messages: vec![
                "Contact me at alice@example.org",
                "Son e-mail: alice@exemple.org",
                "My name is Alice, I work at example.org.",
            ],
            query: "alice@example.org",
        },
        TestMatches1st {
            messages: vec![
                "Contact me at alice+foo@example.org",
                "Son e-mail: alice+foo@exemple.org",
            ],
            query: "alice+foo@example.org",
        },
        // Hash like.
        TestMatches1st {
            messages: vec![
                "It’s in b244423d417369795292e9f4530d0c0e6fa07625",
                "It’s in b244423d41736979529209f4530d0c0e6fa07625",
            ],
            query: "b244423d417369795292e9f4530d0c0e6fa07625",
        },
        TestMatches1st {
            messages: vec!["It’s in b244423", "It’s in b242423"],
            query: "b244423",
        },
        // Code like.
        TestMatches1st {
            messages: vec!["Check out ingest_flushc", "Look at ingest_flushb"],
            query: "ingest_flushc",
        },
        // Domain name.
        TestMatches1st {
            messages: vec!["The client is example.org.", "Their homepage: exemple.org."],
            query: "example.org",
        },
        // URL.
        TestMatches1st {
            messages: vec![
                "All data is in https://example.org/foo?id=123",
                "See https://example.org/foo?id=124",
            ],
            query: "https://example.org/foo?id=123",
        },
        // IP addresses.
        TestMatches1st {
            messages: vec!["192.168.1.0", "192.168.1.1"],
            query: "192.168.1.0",
        },
        TestMatches1st {
            messages: vec!["0.0.0.0", "0.0.0.1"],
            query: "0.0.0.0",
        },
        TestMatches1st {
            messages: vec!["2606:4700::6812:1c68", "2606:4700::6812:1c60"],
            query: "2606:4700::6812:1c68",
        },
        TestMatches1st {
            messages: vec!["::1", "::2"],
            query: "::1",
        },
        // Username.
        TestMatches1st {
            messages: vec!["@example1", "@example2"],
            query: "@example1",
        },
    ];

    for (example_idx, TestMatches1st { messages, query }) in examples.into_iter().enumerate() {
        let executor = make_test_executor_with_id(example_idx);

        for (message_idx, &message) in messages.iter().enumerate() {
            let id = &format!("chat:{message_idx}");
            exec!(executor -> PUSH "messages" "user:1" id message LANG("eng"));
        }
        exec!(executor -> TRIGGER consolidate);

        let response = exec!(executor -> QUERY "messages" "user:1" query LANG("eng"));

        assert_eq!(response, ["chat:0"], "({messages:?}, {query:?})");
    }
}
