// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

//! Sonic library configuration.
//!
//! It does not include server nor channel configuration, which are specific
//! to the `sonic-server` binary.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;

use crate::util::serde::env_var;

#[derive(Deserialize)]
pub struct Config {
    pub normalization: ConfigNormalization,

    pub tokenization: ConfigTokenization,

    pub search: ConfigSearch,

    pub store: ConfigStore,
}

impl Config {
    pub fn validate(&self) {
        if self.search.query_candidates_maximum == 0 {
            panic!("query_candidates_maximum must not be zero");
        }

        // Check 'write_buffer' for KV
        if self.store.kv.database.write_buffer == 0 {
            panic!("write_buffer for kv must not be zero");
        }

        // Check 'flush_after' for KV
        if self.store.kv.database.flush_after >= self.store.kv.pool.inactive_after {
            panic!("flush_after for kv must be strictly lower than inactive_after");
        }

        // Check 'consolidate_after' for FST
        if self.store.fst.graph.consolidate_after >= self.store.fst.pool.inactive_after {
            panic!("consolidate_after for fst must be strictly lower than inactive_after");
        }
    }
}

/// Configuration group for normalization options (Unicode normalization,
/// stemming, lemmatization…).
#[derive(Deserialize, Clone, Copy)]
pub struct ConfigNormalization {
    pub diacritic_folding_enabled: bool,

    #[cfg(feature = "stemming")]
    pub stemming_enabled: bool,
}

/// Configuration group for tokenization options.
#[derive(Deserialize, Clone, Copy)]
pub struct ConfigTokenization {
    pub detect_special_patterns: bool,

    #[serde(alias = "split_special_patterns")]
    pub compat_split_special_patterns: bool,
}

#[derive(Deserialize)]
pub struct ConfigSearch {
    pub query_limit_default: u16,

    pub query_limit_maximum: u16,

    pub query_alternates_try: usize,

    pub query_candidates_maximum: usize,

    pub list_limit_default: u16,

    pub list_limit_maximum: u16,
}

#[derive(Deserialize)]
pub struct ConfigStore {
    pub kv: Arc<ConfigStoreKV>,

    pub fst: Arc<ConfigStoreFST>,
}

#[derive(Deserialize)]
pub struct ConfigStoreKV {
    #[serde(deserialize_with = "env_var::path_buf")]
    pub path: PathBuf,

    pub pool: ConfigStoreKVPool,

    pub database: ConfigStoreKVDatabase,
}

#[derive(Deserialize)]
pub struct ConfigStoreKVPool {
    pub inactive_after: u64,
}

#[derive(Deserialize)]
pub struct ConfigStoreKVDatabase {
    pub flush_after: u64,

    pub compress: bool,

    pub parallelism: u16,

    pub max_files: Option<u32>,

    pub max_compactions: u16,

    pub max_flushes: u16,

    pub write_buffer: usize,

    pub write_ahead_log: bool,
}

#[derive(Deserialize)]
pub struct ConfigStoreFST {
    #[serde(deserialize_with = "env_var::path_buf")]
    pub path: PathBuf,

    pub pool: ConfigStoreFSTPool,

    pub graph: ConfigStoreFSTGraph,
}

#[derive(Deserialize)]
pub struct ConfigStoreFSTPool {
    pub inactive_after: u64,
}

#[derive(Deserialize)]
pub struct ConfigStoreFSTGraph {
    pub consolidate_after: u64,

    pub max_size: usize,

    pub max_words: usize,

    pub min_frequency: u32,
}

#[cfg(test)]
pub(crate) mod tests {
    pub fn defaults_toml() -> &'static str {
        r#"
        [channel]
        inet = "[::1]:1491"
        tcp_timeout = 300

        [normalization]
        diacritic_folding_enabled = true
        stemming_enabled = false

        [tokenization]
        detect_special_patterns = true
        compat_split_special_patterns = false

        [search]
        query_limit_default = 10
        query_limit_maximum = 100
        query_alternates_try = 4
        query_candidates_maximum = 1000
        list_limit_default = 100
        list_limit_maximum = 500

        [store.kv]
        path = "./data/store/kv/"
        pool.inactive_after = 1800
        database.flush_after = 900
        database.compress = true
        database.parallelism = 2
        database.max_compactions = 1
        database.max_flushes = 1
        database.write_buffer = 16384
        database.write_ahead_log = true

        [store.fst]
        path = "./data/store/fst/"
        pool.inactive_after = 300
        graph.consolidate_after = 180
        graph.max_size = 2048
        graph.max_words = 250000
        graph.min_frequency = 2
        "#
    }
}
