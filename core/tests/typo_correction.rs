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

/// This test sentence contains words that are 3–10 characters-long.
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
    test_ingest_then_query!(push: ASTRONOMY_WORDS [ensure_all_terms_indexed] LANG("eng"), query: [
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

/// Ensures the order of words in search queries is insignificant.
#[test]
#[ignore = "Not supported yet (FIXME)"]
fn test_search_term_order_insignificant() {
    #[rustfmt::skip]
    test_ingest_then_query!(push: ASTRONOMY_WORDS [ensure_all_terms_indexed], query: [
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
