// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

//! Feature: Typo correction
//!
//! A “typo factor” is chosen based on the token’s length. Those tests ensure
//! typo correction works, and ensure no regression in the quality of the
//! results.
//! The mapping is configured by `search.typo_factor_word_lengths`. With the
//! default value (`[4, 7, 10]`), it is the following:
//!
//! ```
//! let mut typo_factor = match word.len() {
//!     1..=3 => 0,
//!     4..=6 => 1,
//!     7..=9 => 2,
//!     _ => 3,
//! };
//! ```

mod common;

use crate::common::*;

/// This test sentence contains words that are 3–10 characters-long, while
/// not being stopwords. It’s not realistic but not having stopwords avoids
/// false positives in tests.
const ASTRONOMY_WORDS: &str = "sun moon comet nebula pulsars asteroid satellite spacecraft";

/// Search should allow a certain number of typos (depending on token length).
///
/// This covers missing letters, inverted letters and additional letters.
/// The underlying algorithm used is the Levenshtein distance, so this test is
/// based on that knowledge.
#[test]
fn test_search_allows_typos() {
    // NOTE: Need to make language explicit because of
    //   <https://github.com/valeriansaliou/sonic/issues/322#issuecomment-4638688602>.
    //   Will be fixed separately.
    #[rustfmt::skip]
    test_ingest_then_query!(push: ASTRONOMY_WORDS [ensure_no_stopword] LANG("eng"), query: [
        // 3-letter word, distance = 1.
        ("sum", false),
        // 4-letter word, distance = 1.
        ("ssun", true), // sun
        ("noon", true), // moon
        // 4-letter word, distance = 2.
        ("commit", false),
        // 6-letter word, distance = 1.
        ("nzbula", true), // nebula
        // 6-letter word, distance = 2.
        ("nzbala", false),
        // 7-letter word, distance = 2.
        ("plusars", true), // pulsars
        // 9-letter word, distance = 2.
        ("saetllite", true), // satellite
        // 9-letter word, distance = 3.
        ("satemmote", false),
        // 10-letter word, distance = 3.
        ("sapcecrzft", true), // spacecraft
        // 10-letter word, distance = 4.
        ("sapcecarft", false),
    ] LANG("eng"));
}

/// The word length -> typo factor mapping can be configured.
///
/// See <https://github.com/valeriansaliou/sonic/issues/394>.
#[test]
fn test_config_typo_factor_word_lengths() {
    init_logging();
    let executor = make_test_executor(|app_conf| {
        // 1 typo from 3 letters, 2 typos from 5 letters, never 3 typos.
        app_conf.search.typo_factor_word_lengths = vec![3, 5];
    });

    exec!(executor -> PUSH "messages" "user:1" "chat:1" ASTRONOMY_WORDS LANG("eng"));
    exec!(executor -> TRIGGER consolidate);

    #[rustfmt::skip]
    let examples: [(&str, &[&str]); 3] = [
        // 3-letter word, distance = 1.
        ("sum", &["sun"]),
        // 6-letter word, distance = 2.
        ("nzbala", &["nebula"]),
        // 10-letter word, distance = 3.
        ("sapcecrzft", &[]),
    ];

    for (needle, expected_suggestions) in examples {
        let expected_response: &[&str] = if expected_suggestions.is_empty() {
            &[]
        } else {
            &["chat:1"]
        };

        let response = exec!(executor -> QUERY "messages" "user:1" needle LANG("eng"));
        assert_eq!(response, expected_response, "QUERY {needle:?}");

        let suggestions = exec!(executor -> SUGGEST "messages" "user:1" needle LANG("eng"));
        assert_eq!(suggestions, expected_suggestions, "SUGGEST {needle:?}");
    }
}

/// Typo correction can be disabled by configuring no word length.
///
/// See <https://github.com/valeriansaliou/sonic/issues/394>.
#[test]
fn test_config_typo_correction_disabled() {
    init_logging();
    let executor = make_test_executor(|app_conf| {
        app_conf.search.typo_factor_word_lengths = vec![];
    });

    exec!(executor -> PUSH "messages" "user:1" "chat:1" ASTRONOMY_WORDS LANG("eng"));
    exec!(executor -> TRIGGER consolidate);

    // NOTE: All of those match with the default configuration
    //   (see `test_search_allows_typos`).
    for needle in ["ssun", "nzbula", "plusars", "sapcecrzft"] {
        let response = exec!(executor -> QUERY "messages" "user:1" needle LANG("eng"));
        assert_eq!(response, [] as [&str; 0], "QUERY {needle:?}");

        let suggestions = exec!(executor -> SUGGEST "messages" "user:1" needle LANG("eng"));
        assert_eq!(suggestions, [] as [&str; 0], "SUGGEST {needle:?}");
    }
}

/// Ensures the order of words in search queries is insignificant.
#[test]
#[ignore = "Not supported yet (FIXME)"]
fn test_search_term_order_insignificant() {
    #[rustfmt::skip]
    test_ingest_then_query!(push: ASTRONOMY_WORDS [ensure_no_stopword], query: [
        ("satellite pulsars nebula", true),
        (&format!("missing {ASTRONOMY_WORDS}"), true),
    ]);
}

#[test]
#[ignore = "Not supported yet"]
fn test_chinese_typo_correction() {
    // We should make sure languages with shorter graphemes get typo correction
    // too.
    unimplemented!()
}
