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

    #[serde(default = "defaults::channel_search_list_limit_default")]
    pub list_limit_default: u16,

    #[serde(default = "defaults::channel_search_list_limit_maximum")]
    pub list_limit_maximum: u16,

    #[serde(default = "defaults::channel_search_stopwords")]
    pub stopwords: ConfigChannelSearchStopwords,
}

#[derive(Deserialize, Default)]
pub struct ConfigChannelSearchStopwords {
    pub epo: Option<Vec<String>>,
    pub eng: Option<Vec<String>>,
    pub rus: Option<Vec<String>>,
    pub cmn: Option<Vec<String>>,
    pub spa: Option<Vec<String>>,
    pub por: Option<Vec<String>>,
    pub ita: Option<Vec<String>>,
    pub ben: Option<Vec<String>>,
    pub fra: Option<Vec<String>>,
    pub deu: Option<Vec<String>>,

    pub ukr: Option<Vec<String>>,
    pub kat: Option<Vec<String>>,
    pub ara: Option<Vec<String>>,
    pub hin: Option<Vec<String>>,
    pub jpn: Option<Vec<String>>,
    pub heb: Option<Vec<String>>,
    pub yid: Option<Vec<String>>,
    pub pol: Option<Vec<String>>,
    pub amh: Option<Vec<String>>,
    pub jav: Option<Vec<String>>,

    pub kor: Option<Vec<String>>,
    pub nob: Option<Vec<String>>,
    pub dan: Option<Vec<String>>,
    pub swe: Option<Vec<String>>,
    pub fin: Option<Vec<String>>,
    pub tur: Option<Vec<String>>,
    pub nld: Option<Vec<String>>,
    pub hun: Option<Vec<String>>,
    pub ces: Option<Vec<String>>,
    pub ell: Option<Vec<String>>,

    pub bul: Option<Vec<String>>,
    pub bel: Option<Vec<String>>,
    pub mar: Option<Vec<String>>,
    pub kan: Option<Vec<String>>,
    pub ron: Option<Vec<String>>,
    pub slv: Option<Vec<String>>,
    pub hrv: Option<Vec<String>>,
    pub srp: Option<Vec<String>>,
    pub mkd: Option<Vec<String>>,
    pub lit: Option<Vec<String>>,

    pub lav: Option<Vec<String>>,
    pub est: Option<Vec<String>>,
    pub tam: Option<Vec<String>>,
    pub vie: Option<Vec<String>>,
    pub urd: Option<Vec<String>>,
    pub tha: Option<Vec<String>>,
    pub guj: Option<Vec<String>>,
    pub uzb: Option<Vec<String>>,
    pub pan: Option<Vec<String>>,
    pub aze: Option<Vec<String>>,

    pub ind: Option<Vec<String>>,
    pub tel: Option<Vec<String>>,
    pub pes: Option<Vec<String>>,
    pub mal: Option<Vec<String>>,
    pub ori: Option<Vec<String>>,
    pub mya: Option<Vec<String>>,
    pub nep: Option<Vec<String>>,
    pub sin: Option<Vec<String>>,
    pub khm: Option<Vec<String>>,
    pub tuk: Option<Vec<String>>,

    pub aka: Option<Vec<String>>,
    pub zul: Option<Vec<String>>,
    pub sna: Option<Vec<String>>,
    pub afr: Option<Vec<String>>,
    pub lat: Option<Vec<String>>,
    pub slk: Option<Vec<String>>,
    pub cat: Option<Vec<String>>,
    pub tgl: Option<Vec<String>>,
    pub hye: Option<Vec<String>>,
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
