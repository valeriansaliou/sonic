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

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;

use crate::util::serde::env_var;

#[derive(Deserialize)]
pub struct Config {
    pub normalization: ConfigNormalization,

    pub tokenization: ConfigTokenization,

    pub stopwords: ConfigStopwords,

    pub search: ConfigSearch,

    pub store: ConfigStore,
}

impl Config {
    pub fn validate(&self) {
        // Check 'write_buffer' for KV
        if self.store.kv.database.write_buffer_size == Some(0) {
            panic!("write_buffer for kv must not be zero");
        }

        // Check 'flush_after' for KV
        if self.store.kv.database.flush_after >= self.store.kv.pool.inactive_after {
            panic!("flush_after for kv must be strictly lower than inactive_after");
        }

        // Check 'flush_after' for KV
        if self.store.kv.database.max_flushes.is_some()
            && self.store.kv.database.max_background_jobs.is_some()
        {
            panic!("max_background_jobs makes max_flushes unneeded, don’t configure both");
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
    #[serde(with = "crate::util::serde::none_string_as_none")]
    pub unicode_normalization: Option<UnicodeNormalization>,

    pub diacritic_folding_enabled: bool,

    #[cfg(feature = "stemming")]
    pub stemming_enabled: bool,
}

#[derive(Deserialize, Debug, Clone, Copy)]
pub enum UnicodeNormalization {
    /// Unicode Normalization Form C.
    Nfc,
    /// Unicode Normalization Form KC.
    Nfkc,
}

/// Configuration group for tokenization options.
#[derive(Deserialize, Clone, Copy)]
pub struct ConfigTokenization {
    pub detect_special_patterns: bool,

    #[serde(alias = "split_special_patterns")]
    pub compat_split_special_patterns: bool,
}

#[derive(Deserialize, Clone, Default)]
pub struct ConfigStopwords {
    #[serde(deserialize_with = "to_stopwords")]
    pub allow: HashSet<String>,

    #[serde(deserialize_with = "to_stopwords")]
    pub deny: HashSet<String>,
}

fn to_stopwords<'de, D>(deserializer: D) -> Result<HashSet<String>, D::Error>
where
    D: serde::de::Deserializer<'de>,
{
    use unicode_normalization::UnicodeNormalization as _;

    let vec: Vec<Box<str>> = Deserialize::deserialize(deserializer)?;
    let stopwords_iter = vec.into_iter().map(|s| s.nfkd().to_string());
    Ok(HashSet::from_iter(stopwords_iter))
}

#[derive(Deserialize)]
pub struct ConfigSearch {
    pub query_limit_default: u16,

    pub query_limit_maximum: u16,

    pub query_alternates_try: usize,

    pub query_minimum_term_idf_default: f32,

    pub query_minimum_term_idf_minimum_object_count: u64,

    pub suggest_limit_default: u16,

    pub suggest_limit_maximum: u16,

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

    pub retain_word_objects: usize,

    pub pool: ConfigStoreKVPool,

    pub database: ConfigStoreKVDatabase,
}

#[derive(Deserialize)]
pub struct ConfigStoreKVPool {
    pub inactive_after: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigStoreKVDatabase {
    pub flush_after: u64,

    pub write_ahead_log: bool,

    /// Whether or not to compress.
    ///
    /// Will get overriden if [`compression_type`](Self::compression_type) is
    /// also specified.
    #[serde(default)]
    pub compress: Option<bool>,

    #[serde(default)]
    pub parallelism: Option<i32>,

    #[serde(default)]
    #[serde(alias = "max_files")]
    pub max_open_files: Option<i32>,

    // TODO(major): Make this MB, as in Kvrocks.
    /// WARN: In KB!
    #[serde(default = "default_write_buffer_size")]
    #[serde(alias = "write_buffer")]
    pub write_buffer_size: Option<usize>,

    #[serde(default)]
    pub max_write_buffer_number: Option<i32>,

    #[serde(default)]
    pub min_write_buffer_number: Option<i32>,

    #[serde(default)]
    pub min_write_buffer_number_to_merge: Option<i32>,

    #[serde(default)]
    pub block_cache_size: Option<u32>,

    #[serde(default)]
    pub cache_index_and_filter_blocks: Option<bool>,

    #[serde(default)]
    #[serde(alias = "compression")]
    #[serde(deserialize_with = "to_rocksdb_compression_type_opt")]
    pub compression_type: Option<rocksdb::DBCompressionType>,

    #[serde(default)]
    #[serde(alias = "wal_compression")]
    #[serde(deserialize_with = "to_rocksdb_compression_type_opt")]
    pub wal_compression_type: Option<rocksdb::DBCompressionType>,

    #[serde(default)]
    pub wal_ttl_seconds: Option<u64>,

    #[serde(default)]
    pub wal_size_limit_mb: Option<u64>,

    #[serde(default)]
    pub wal_bytes_per_sync: Option<u64>,

    #[serde(default)]
    #[serde(deserialize_with = "to_rocksdb_recovery_mode_opt")]
    pub wal_recovery_mode: Option<rocksdb::DBRecoveryMode>,

    #[serde(default)]
    pub compression_level: Option<i32>,

    #[serde(default)]
    #[serde(alias = "compression_start_level")]
    pub min_level_to_compress: Option<std::ffi::c_int>,

    #[serde(default)]
    #[serde(alias = "level0_file_num_compaction_trigger")]
    pub level_zero_file_num_compaction_trigger: Option<i32>,

    #[serde(default)]
    #[serde(alias = "level0_slowdown_writes_trigger")]
    pub level_zero_slowdown_writes_trigger: Option<i32>,

    #[serde(default)]
    #[serde(alias = "level0_stop_writes_trigger")]
    pub level_zero_stop_writes_trigger: Option<i32>,

    #[serde(default)]
    pub max_bytes_for_level_base: Option<u64>,

    #[serde(default)]
    pub max_bytes_for_level_multiplier: Option<f64>,

    #[serde(default)]
    pub target_file_size_base: Option<u64>,

    #[serde(default)]
    pub max_background_jobs: Option<i32>,

    #[serde(default)]
    #[serde(alias = "max_compactions")]
    pub max_subcompactions: Option<u32>,

    #[serde(default)]
    pub max_flushes: Option<u32>,

    #[serde(default)]
    pub stats_dump_period_sec: Option<u32>,
}

fn default_write_buffer_size() -> Option<usize> {
    Some(16384)
}

fn parse_rocksdb_compression_type<E: serde::de::Error>(
    str: &str,
) -> Result<rocksdb::DBCompressionType, E> {
    // NOTE: Some values are not available because not compiled in rocksdb
    //   (feature flag is off).
    match str.to_ascii_lowercase().as_str() {
        "none" => Ok(rocksdb::DBCompressionType::None),
        // "snappy" => Ok(rocksdb::DBCompressionType::Snappy),
        // "zlib" => Ok(rocksdb::DBCompressionType::Zlib),
        // "bz2" => Ok(rocksdb::DBCompressionType::Bz2),
        // "lz4" => Ok(rocksdb::DBCompressionType::Lz4),
        // "lz4hc" => Ok(rocksdb::DBCompressionType::Lz4hc),
        "zstd" => Ok(rocksdb::DBCompressionType::Zstd),
        _ => Err(serde::de::Error::unknown_variant(
            str,
            &[
                "none",
                // "snappy",
                // "zlib",
                // "bz2",
                // "lz4",
                // "lz4hc",
                "zstd",
            ],
        )),
    }
}

fn to_rocksdb_compression_type_opt<'de, D>(
    deserializer: D,
) -> Result<Option<rocksdb::DBCompressionType>, D::Error>
where
    D: serde::de::Deserializer<'de>,
{
    let str: Option<String> = Deserialize::deserialize(deserializer)?;
    str.map(|s| parse_rocksdb_compression_type(&s)).transpose()
}

fn parse_rocksdb_recovery_mode<E: serde::de::Error>(
    str: &str,
) -> Result<rocksdb::DBRecoveryMode, E> {
    match str.to_ascii_lowercase().as_str() {
        "tolerate_corrupted_tail_records" | "TolerateCorruptedTailRecords" => {
            Ok(rocksdb::DBRecoveryMode::TolerateCorruptedTailRecords)
        }
        "absolute_consistency" | "AbsoluteConsistency" => {
            Ok(rocksdb::DBRecoveryMode::AbsoluteConsistency)
        }
        "point_in_time" | "PointInTime" => Ok(rocksdb::DBRecoveryMode::PointInTime),
        "skip_any_corrupted_record" | "SkipAnyCorruptedRecord" => {
            Ok(rocksdb::DBRecoveryMode::SkipAnyCorruptedRecord)
        }
        _ => Err(serde::de::Error::unknown_variant(
            str,
            &[
                "tolerate_corrupted_tail_records",
                "absolute_consistency",
                "point_in_time",
                "skip_any_corrupted_record",
            ],
        )),
    }
}

fn to_rocksdb_recovery_mode_opt<'de, D>(
    deserializer: D,
) -> Result<Option<rocksdb::DBRecoveryMode>, D::Error>
where
    D: serde::de::Deserializer<'de>,
{
    let str: Option<String> = Deserialize::deserialize(deserializer)?;
    str.map(|s| parse_rocksdb_recovery_mode(&s)).transpose()
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
}

#[cfg(test)]
pub(crate) mod tests {
    pub fn defaults_toml() -> &'static str {
        r#"
        [channel]
        inet = "[::1]:1491"
        tcp_timeout = 300

        [normalization]
        unicode_normalization = "none"
        diacritic_folding_enabled = true
        stemming_enabled = false

        [tokenization]
        detect_special_patterns = true
        compat_split_special_patterns = false

        [stopwords]
        allow = []
        deny = []

        [search]
        query_limit_default = 10
        query_limit_maximum = 100
        query_alternates_try = 4
        query_minimum_term_idf_default = 0.1
        query_minimum_term_idf_minimum_object_count = 100
        suggest_limit_default = 5
        suggest_limit_maximum = 20
        list_limit_default = 100
        list_limit_maximum = 500

        [store.kv]
        path = "./data/store/kv/"
        retain_word_objects = 1000
        pool.inactive_after = 1800
        database.flush_after = 900
        database.compression_type = "zstd"
        database.parallelism = 2
        database.write_ahead_log = true

        [store.fst]
        path = "./data/store/fst/"
        pool.inactive_after = 300
        graph.consolidate_after = 180
        graph.max_size = 2048
        graph.max_words = 250000
        "#
    }
}
