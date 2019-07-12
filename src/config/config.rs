// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::net::SocketAddr;
use std::path::PathBuf;

use super::defaults;
use super::env_var;

#[derive(Deserialize)]
pub struct Config {
    pub server: ConfigServer,
    pub channel: ConfigChannel,
    pub store: ConfigStore,
}

#[derive(Deserialize)]
pub struct ConfigServer {
    #[serde(
        default = "defaults::server_log_level",
        deserialize_with = "env_var::str"
    )]
    pub log_level: String,
}

#[derive(Deserialize)]
pub struct ConfigChannel {
    #[serde(
        default = "defaults::channel_inet",
        deserialize_with = "env_var::socket_addr"
    )]
    pub inet: SocketAddr,

    #[serde(default = "defaults::channel_tcp_timeout")]
    pub tcp_timeout: u64,

    #[serde(default, deserialize_with = "env_var::opt_str")]
    pub auth_password: Option<String>,

    pub search: ConfigChannelSearch,
}

#[derive(Deserialize)]
pub struct ConfigChannelSearch {
    #[serde(default = "defaults::channel_search_query_limit_default")]
    pub query_limit_default: u16,

    #[serde(default = "defaults::channel_search_query_limit_maximum")]
    pub query_limit_maximum: u16,

    #[serde(default = "defaults::channel_search_query_alternates_try")]
    pub query_alternates_try: usize,

    #[serde(default = "defaults::channel_search_suggest_limit_default")]
    pub suggest_limit_default: u16,

    #[serde(default = "defaults::channel_search_suggest_limit_maximum")]
    pub suggest_limit_maximum: u16,
}

#[derive(Deserialize)]
pub struct ConfigStore {
    pub kv: ConfigStoreKV,
    pub fst: ConfigStoreFST,
}

#[derive(Deserialize)]
pub struct ConfigStoreKV {
    #[serde(
        default = "defaults::store_kv_path",
        deserialize_with = "env_var::path_buf"
    )]
    pub path: PathBuf,

    #[serde(default = "defaults::store_kv_retain_word_objects")]
    pub retain_word_objects: usize,

    pub pool: ConfigStoreKVPool,
    pub database: ConfigStoreKVDatabase,
}

#[derive(Deserialize)]
pub struct ConfigStoreKVPool {
    #[serde(default = "defaults::store_kv_pool_inactive_after")]
    pub inactive_after: u64,
}

#[derive(Deserialize)]
pub struct ConfigStoreKVDatabase {
    #[serde(default = "defaults::store_kv_database_flush_after")]
    pub flush_after: u64,

    #[serde(default = "defaults::store_kv_database_compress")]
    pub compress: bool,

    #[serde(default = "defaults::store_kv_database_parallelism")]
    pub parallelism: u16,

    pub max_files: Option<u32>,

    #[serde(default = "defaults::store_kv_database_max_compactions")]
    pub max_compactions: u16,

    #[serde(default = "defaults::store_kv_database_max_flushes")]
    pub max_flushes: u16,

    #[serde(default = "defaults::store_kv_database_write_buffer")]
    pub write_buffer: usize,

    #[serde(default = "defaults::store_kv_database_write_ahead_log")]
    pub write_ahead_log: bool,
}

#[derive(Deserialize)]
pub struct ConfigStoreFST {
    #[serde(
        default = "defaults::store_fst_path",
        deserialize_with = "env_var::path_buf"
    )]
    pub path: PathBuf,

    pub pool: ConfigStoreFSTPool,
    pub graph: ConfigStoreFSTGraph,
}

#[derive(Deserialize)]
pub struct ConfigStoreFSTPool {
    #[serde(default = "defaults::store_fst_pool_inactive_after")]
    pub inactive_after: u64,
}

#[derive(Deserialize)]
pub struct ConfigStoreFSTGraph {
    #[serde(default = "defaults::store_fst_graph_consolidate_after")]
    pub consolidate_after: u64,

    #[serde(default = "defaults::store_fst_graph_max_size")]
    pub max_size: usize,

    #[serde(default = "defaults::store_fst_graph_max_words")]
    pub max_words: usize,
}
