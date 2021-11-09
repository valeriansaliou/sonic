// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::fs::File;
use std::io::Read;

use super::Config;
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
