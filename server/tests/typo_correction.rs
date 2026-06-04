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
//! At the moment of this writing, the mapping function is the following:
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

macro_rules! test {
    ($sentence:tt $(LANG($ingest_lang:expr))?, $examples:tt $(LANG($query_lang:expr))?) => {
        init_logging();
        let executor = make_test_executor();

        exec!(executor -> PUSH "messages" "user:1" "chat:1" $sentence $(LANG($ingest_lang))?);
        exec!(executor -> TRIGGER consolidate);

        // Sanity check: ensure no stopword was provided (could make
        // examples pass for the wrong reason).
        assert_eq!(
            exec!(executor -> COUNT "messages" "user:1" "chat:1"),
            Ok($sentence.split_ascii_whitespace().count() as u32)
        );

        for (needle, should_match) in $examples.into_iter() {
            assert!(!needle.contains("{"), "Needle shouldn’t contain '{{', make sure you formatted the string correctly.");

            let response = exec!(executor -> QUERY "messages" "user:1" needle $(LANG($query_lang))?);
            if should_match {
                assert_eq!(response, ["chat:1"], "Did not find {needle:?} in {:?}", $sentence);
            } else {
                assert_eq!(response, vec![] as Vec<&str>, "Found {needle:?} in {:?}", $sentence);
            }
        }
    };
}

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
#[ignore = "Known issue (FIXME)"]
fn test_search_allows_typos() {
    #[rustfmt::skip]
    test!(ASTRONOMY_WORDS, [
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
        // FIXME: Broken by `fst_action.suggest_words(&term, alternates_try + 1, Some(1))` in `search.rs`.
        ("plusars", true), // pulsars
        // 9-letter word, distance = 2.
        ("saetllite", true), // satellite
        // 9-letter word, distance = 3.
        ("satemmite", false),
        // 10-letter word, distance = 3.
        ("sapcecrzft", true), // spacecraft
    ]);
}

/// Ensures the order of words in search queries is insignificant.
#[test]
#[ignore = "Not supported yet (FIXME)"]
fn test_search_term_order_insignificant() {
    #[rustfmt::skip]
    test!(ASTRONOMY_WORDS, [
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
