// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use unicode_normalization::UnicodeNormalization;

use crate::config::StopwordsConfig;

pub(super) fn is_stopword(word: &str, config: &StopwordsConfig) -> bool {
    let word = word.nfkd().to_string();

    if config.deny.contains(&word) {
        // Word is a stopword (per configuration).
        return true;
    }

    if config.allow.contains(&word) {
        // Word is *not* a stopword (per configuration).
        return false;
    }

    false
}
