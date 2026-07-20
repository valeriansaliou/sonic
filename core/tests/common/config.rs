// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::common::util::unique_hex;

pub fn make_config(toml: &str) -> sonic::Config {
    let raw_config: config::Config = config::Config::builder()
        .add_source(config::File::from_str(toml, config::FileFormat::Toml))
        .build()
        .expect("error reading config");

    // Parse configuration.
    let config = raw_config
        .try_deserialize::<sonic::Config>()
        .expect("syntax error in config");

    // Validate configuration.
    config.validate();

    config
}

pub fn defaults_toml() -> String {
    let test_id = unique_hex().unwrap();

    let test_data_path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(&test_id);
    std::fs::create_dir_all(&test_data_path).unwrap();

    let kv_store_path = test_data_path.join("kv-store");
    std::fs::create_dir(&kv_store_path).unwrap();
    let fst_store_path = test_data_path.join("fst-store");
    std::fs::create_dir(&fst_store_path).unwrap();

    format!(
        r#"
        [normalization]
        diacritic_folding_enabled = true
        stemming_enabled = false

        [tokenization]
        detect_special_patterns = true
        compat_split_special_patterns = false

        [search]
        query_limit_default = 10
        query_limit_maximum = 100
        query_alternates_try = 4
        query_candidates_maximum = 1000
        list_limit_default = 100
        list_limit_maximum = 500

        [store.kv]
        path = {kv_store_path:?}
        pool.inactive_after = 1800
        database.flush_after = 900
        database.compress = true
        database.parallelism = 2
        database.max_compactions = 1
        database.max_flushes = 1
        database.write_buffer = 16384
        database.write_ahead_log = true

        [store.fst]
        path = {fst_store_path:?}
        pool.inactive_after = 300
        graph.consolidate_after = 180
        graph.max_size = 2048
        graph.max_words = 250000
        graph.min_frequency = 1
        "#,
    )
}
