// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::sync::Arc;

pub fn defaults_toml() -> &'static str {
    r#"
    [server]
    log_level = "error"

    [channel]
    inet = "[::1]:1491"
    tcp_timeout = 300
    bulk_buffer_size = 8388608

    [normalization]
    # TODO(major): Enable by default.
    diacritic_folding_enabled = false
    stemming_enabled = false

    [tokenization]
    detect_special_patterns = true
    # TODO(major): Disable by default.
    compat_split_special_patterns = true

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

impl Config {
    pub fn parse(source: impl ::config::Source + Send + Sync + 'static) -> Self {
        // Read configuration.
        let raw_config: config::Config = config::Config::builder()
            // Start from defaults.
            .add_source(config::File::from_str(
                defaults_toml(),
                config::FileFormat::Toml,
            ))
            // Merge static configuration (from file).
            .add_source(source)
            // Merge environment overrides.
            .add_source(
                config::Environment::with_prefix("SONIC")
                    .separator("__")
                    .prefix_separator("_"),
            )
            .build()
            .expect("error reading config");

        // `#[serde(flatten)]` breaks type coercion from the `config` crate,
        // so we can’t have `crate::Config` define
        // `#[serde(flatten)] sonic: sonic::Config`. It’s a bit dirty but at
        // least we don’t have to manually add custom deserialization logic to
        // all non-string fields (in the core!).
        #[derive(serde::Deserialize)]
        pub struct ServerConfigTemp {
            pub channel: ConfigChannel,
            pub server: ConfigServer,
        }

        // Parse configuration.
        let mut server_config = raw_config
            .clone()
            .try_deserialize::<ServerConfigTemp>()
            .expect("syntax error in config");
        let mut core_config = raw_config
            .try_deserialize::<sonic::Config>()
            .expect("syntax error in config");

        back_compat::migrate_channel_search(&mut server_config.channel, &mut core_config);

        // Validate configuration.
        core_config.validate();
        assert!(
            server_config.channel.bulk_buffer_size >= 20_000,
            "bulk_buffer_size must be at least 20000"
        );

        Config {
            channel: server_config.channel,
            server: server_config.server,
            sonic: Arc::new(core_config),
        }
    }
}

pub fn read_config(path: &str) -> Config {
    // Abort if the user specified a config path that does not exist.
    if path != crate::DEFAULT_CONFIG_FILE_PATH && !std::path::Path::new(path).exists() {
        panic!("Cannot find config file at {path:?}");
    }

    let path = std::path::absolute(path).unwrap();
    tracing::debug!("reading config file: {path:?}");

    Config::parse(
        config::File::from(path)
            .format(config::FileFormat::Toml)
            .required(false),
    )
}

pub struct Config {
    pub channel: ConfigChannel,

    pub server: ConfigServer,

    pub sonic: Arc<sonic::Config>,
}

#[allow(deprecated)]
#[derive(serde::Deserialize)]
pub struct ConfigChannel {
    #[serde(deserialize_with = "sonic::util::serde::env_var::socket_addr")]
    pub inet: std::net::SocketAddr,

    pub tcp_timeout: u64,

    pub bulk_buffer_size: usize,

    #[serde(default, deserialize_with = "sonic::util::serde::env_var::opt_str")]
    pub auth_password: Option<String>,

    #[deprecated(since = "1.6.0", note = "Use `search` instead of `channel.search`")]
    #[serde(default)]
    pub search: Option<back_compat::ConfigChannelSearch>,
}

#[derive(serde::Deserialize)]
pub struct ConfigServer {
    #[serde(deserialize_with = "sonic::util::serde::env_var::str")]
    pub log_level: String,
}

#[allow(deprecated)]
mod back_compat {
    #[deprecated(since = "1.6.0", note = "Use `search` instead of `channel.search`")]
    #[derive(serde::Deserialize)]
    pub struct ConfigChannelSearch {
        #[serde(default)]
        pub query_limit_default: Option<u16>,

        #[serde(default)]
        pub query_limit_maximum: Option<u16>,

        #[serde(default)]
        pub query_alternates_try: Option<usize>,

        #[serde(default)]
        pub suggest_limit_default: Option<u16>,

        #[serde(default)]
        pub suggest_limit_maximum: Option<u16>,

        #[serde(default)]
        pub list_limit_default: Option<u16>,

        #[serde(default)]
        pub list_limit_maximum: Option<u16>,
    }

    // This is dirty, but AFAIK (@RemiBardon) the `config` crate doesn’t
    // provide a better API and hopefully we won’t have to do this again.
    pub fn migrate_channel_search(
        channel: &mut crate::config::ConfigChannel,
        sonic: &mut sonic::Config,
    ) {
        if let Some(search) = channel.search.take() {
            tracing::warn!(
                "You’re still using the deprecated `channel.search` key. \
                Please use `search` instead. \
                For this run, we will override `search` with `channel.search`."
            );

            let ConfigChannelSearch {
                query_limit_default,
                query_limit_maximum,
                query_alternates_try,
                suggest_limit_default,
                suggest_limit_maximum,
                list_limit_default,
                list_limit_maximum,
            } = search;

            if let Some(query_limit_default) = query_limit_default {
                sonic.search.query_limit_default = query_limit_default;
            }
            if let Some(query_limit_maximum) = query_limit_maximum {
                sonic.search.query_limit_maximum = query_limit_maximum;
            }
            if let Some(query_alternates_try) = query_alternates_try {
                sonic.search.query_alternates_try = query_alternates_try;
            }
            let _ = (suggest_limit_default, suggest_limit_maximum);
            if let Some(list_limit_default) = list_limit_default {
                sonic.search.list_limit_default = list_limit_default;
            }
            if let Some(list_limit_maximum) = list_limit_maximum {
                sonic.search.list_limit_maximum = list_limit_maximum;
            }
        }
    }

    #[cfg(test)]
    #[test]
    fn test_channel_search_migration() {
        let old_config_toml = r#"
        [channel]
        search.query_limit_default = 42
        search.query_alternates_try = 42
        search.suggest_limit_default = 42
        search.suggest_limit_maximum = 42
        search.list_limit_default = 42
        search.list_limit_maximum = 42
        "#;

        let config = crate::Config::parse(config::File::from_str(
            old_config_toml,
            config::FileFormat::Toml,
        ));

        assert!(
            config.channel.search.is_none(),
            "`channel.search` not emptied"
        );
        assert_eq!(
            config.sonic.search.list_limit_default, 42,
            "should be overriden"
        );
        assert_ne!(
            config.sonic.search.query_limit_maximum, 42,
            "should not be overriden"
        );
    }

    #[cfg(test)]
    #[test]
    fn test_channel_search_migration_mixed() {
        let old_config_toml = r#"
        [channel.search]
        query_limit_default = 42
        # query_limit_maximum = 42
        query_alternates_try = 42
        suggest_limit_default = 42
        suggest_limit_maximum = 42
        list_limit_default = 42
        list_limit_maximum = 42

        [search]
        query_limit_default = 8
        query_limit_maximum = 8
        "#;

        let config = crate::Config::parse(config::File::from_str(
            old_config_toml,
            config::FileFormat::Toml,
        ));

        assert!(
            config.channel.search.is_none(),
            "`channel.search` not emptied"
        );
        assert_eq!(
            config.sonic.search.list_limit_default, 42,
            "should be overriden"
        );
        assert_eq!(
            config.sonic.search.query_limit_maximum, 8,
            "should not be overriden"
        );
    }
}
