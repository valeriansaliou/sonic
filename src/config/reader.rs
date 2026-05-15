// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use super::options::Config;
use crate::APP_ARGS;

pub struct ConfigReader;

impl ConfigReader {
    pub fn make() -> Config {
        let config_path = &APP_ARGS.config;

        // Abort if the user specified a config path that does not exist.
        if config_path != crate::DEFAULT_CONFIG_FILE_PATH
            && !std::path::Path::new(config_path).exists()
        {
            panic!("Cannot find config file at '{config_path}'");
        }

        debug!("reading config file: {config_path}");

        // Read configuration.
        let raw_config: config::Config = config::Config::builder()
            // Start from defaults.
            .add_source(config::File::from_str(
                super::defaults::defaults(),
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
        Self::validate(&config);

        config
    }

    fn validate(config: &Config) {
        // Check 'write_buffer' for KV
        if config.store.kv.database.write_buffer == 0 {
            panic!("write_buffer for kv must not be zero");
        }

        // Check 'flush_after' for KV
        if config.store.kv.database.flush_after >= config.store.kv.pool.inactive_after {
            panic!("flush_after for kv must be strictly lower than inactive_after");
        }

        // Check 'consolidate_after' for FST
        if config.store.fst.graph.consolidate_after >= config.store.fst.pool.inactive_after {
            panic!("consolidate_after for fst must be strictly lower than inactive_after");
        }
    }
}
