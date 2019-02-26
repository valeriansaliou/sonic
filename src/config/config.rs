// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::net::SocketAddr;
use std::path::PathBuf;

use super::defaults;

#[derive(Deserialize)]
pub struct Config {
    pub server: ConfigServer,
    pub channel: ConfigChannel,
    pub store: ConfigStore,
}

#[derive(Deserialize)]
pub struct ConfigServer {
    #[serde(default = "defaults::server_log_level")]
    pub log_level: String,
}

#[derive(Deserialize)]
pub struct ConfigChannel {
    #[serde(default = "defaults::channel_inet")]
    pub inet: SocketAddr,

    #[serde(default = "defaults::channel_tcp_timeout")]
    pub tcp_timeout: u64,

    pub search: ConfigChannelSearch,
}

#[derive(Deserialize)]
pub struct ConfigChannelSearch {
    #[serde(default = "defaults::channel_search_query_limit_default")]
    pub query_limit_default: u16,

    #[serde(default = "defaults::channel_search_query_limit_maximum")]
    pub query_limit_maximum: u16,
}

#[derive(Deserialize)]
pub struct ConfigStore {
    pub kv: ConfigStoreKV,
    pub fst: ConfigStoreFST,
}

#[derive(Deserialize)]
pub struct ConfigStoreKV {
    #[serde(default = "defaults::store_kv_path")]
    pub path: PathBuf,
}

#[derive(Deserialize)]
pub struct ConfigStoreFST {
    #[serde(default = "defaults::store_fst_path")]
    pub path: PathBuf,
}
