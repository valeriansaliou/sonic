// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

pub(super) fn check_over_limits(
    bytes_count: usize,
    words_count: usize,
    fst_graph_config: &crate::config::FstStoreGraphConfig,
) -> bool {
    // Over bytes limit?
    let max_size = fst_graph_config.max_size * 1024;
    if bytes_count >= max_size {
        tracing::info!(
            "fst has exceeded maximum allowed bytes: {bytes_count} over limit: {max_size}"
        );

        return true;
    }

    // Over words limit?
    let max_words = fst_graph_config.max_words;
    if words_count >= max_words {
        tracing::info!(
            "fst has exceeded maximum allowed words: {words_count} over limit: {max_words}"
        );

        return true;
    }

    // Not over limit
    false
}
