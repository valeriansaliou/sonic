// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

pub fn defaults_toml() -> &'static str {
    r#"
    [server]
    log_level = "error"

    [channel]
    inet = "[::1]:1491"
    tcp_timeout = 300
    search.query_limit_default = 10
    search.query_limit_maximum = 100
    search.query_alternates_try = 4
    search.suggest_limit_default = 5
    search.suggest_limit_maximum = 20
    search.list_limit_default = 100
    search.list_limit_maximum = 500

    [store.kv]
    path = "./data/store/kv/"
    retain_word_objects = 1000
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
    "#
}

pub fn read_config(app_args: &crate::AppArgs) -> Config {
    let config_path = &app_args.config;

    // Abort if the user specified a config path that does not exist.
    if config_path != crate::DEFAULT_CONFIG_FILE_PATH && !std::path::Path::new(config_path).exists()
    {
        panic!("Cannot find config file at '{config_path}'");
    }

    debug!("reading config file: {config_path}");

    // Read configuration.
    let raw_config: config::Config = config::Config::builder()
        // Start from defaults.
        .add_source(config::File::from_str(
            defaults_toml(),
            config::FileFormat::Toml,
        ))
        // Merge static configuration (from file).
        .add_source(config::File::new(config_path, config::FileFormat::Toml).required(false))
        // Merge environment overrides.
        .add_source(
            config::Environment::with_prefix("SONIC")
                .separator("__")
                .prefix_separator("_"),
        )
        .build()
        .expect("error reading config");

    // Parse configuration.
    let config = raw_config
        .try_deserialize::<Config>()
        .expect("syntax error in config");

    // Validate configuration.
    config.sonic.validate();

    config
}

// NOTE: Channel config will be moved here, but for now we can’t because the
//   library uses the `channel.search` fields.
#[derive(serde::Deserialize)]
pub struct Config {
    pub server: ConfigServer,

    #[serde(flatten)]
    pub sonic: sonic::Config,
}

#[derive(serde::Deserialize)]
pub struct ConfigServer {
    #[serde(deserialize_with = "sonic::util::serde::env_var::str")]
    pub log_level: String,
}
