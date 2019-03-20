// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Sonic OSS License v1.0 (SOSSL v1.0)

use std::fs::File;
use std::io::Read;
use toml;

use super::config::*;
use crate::APP_ARGS;

pub struct ConfigReader;

impl ConfigReader {
    pub fn make() -> Config {
        debug!("reading config file: {}", &APP_ARGS.config);

        let mut file = File::open(&APP_ARGS.config).expect("cannot find config file");
        let mut conf = String::new();

        file.read_to_string(&mut conf)
            .expect("cannot read config file");

        debug!("read config file: {}", &APP_ARGS.config);

        // Parse configuration
        let config = toml::from_str(&conf).expect("syntax error in config file");

        // Validate configuration
        Self::validate(&config);

        config
    }

    fn validate(config: &Config) {
        if config.store.fst.graph.consolidate_after >= config.store.fst.pool.inactive_after {
            panic!("consolidate_after for fst must be strictly lower than inactive_after");
        }
    }
}
