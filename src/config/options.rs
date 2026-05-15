// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::net::SocketAddr;
use std::path::PathBuf;

use super::env_var;

#[derive(Deserialize)]
pub struct Config {
    pub server: ConfigServer,

    pub channel: ConfigChannel,

    pub store: ConfigStore,
}

#[derive(Deserialize)]
pub struct ConfigServer {
    #[serde(deserialize_with = "env_var::str")]
    pub log_level: String,
}

#[derive(Deserialize)]
pub struct ConfigChannel {
    #[serde(deserialize_with = "env_var::socket_addr")]
    pub inet: SocketAddr,

    pub tcp_timeout: u64,

    #[serde(default, deserialize_with = "env_var::opt_str")]
    pub auth_password: Option<String>,

    pub search: ConfigChannelSearch,
}

#[derive(Deserialize)]
pub struct ConfigChannelSearch {
    pub query_limit_default: u16,

    pub query_limit_maximum: u16,

    pub query_alternates_try: usize,

    pub suggest_limit_default: u16,

    pub suggest_limit_maximum: u16,

    pub list_limit_default: u16,

    pub list_limit_maximum: u16,
}

#[derive(Deserialize)]
pub struct ConfigStore {
    pub kv: ConfigStoreKV,

    pub fst: ConfigStoreFST,
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
}
