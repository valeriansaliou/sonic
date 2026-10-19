// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

#![allow(dead_code)]

use std::{path::PathBuf, sync::LazyLock};

pub const SONIC_BENCH_PASSWORD: &str = "SecretPassword";

pub const BENCH_DATA_PATH: &str = concat!(
    env!("CARGO_TARGET_TMPDIR"),
    "/bench-data/",
    env!("CARGO_CRATE_NAME")
);

pub static SONIC_BENCH_TRACES_PATH: LazyLock<PathBuf> = LazyLock::new(|| {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("bench-traces");

    if !path.exists() {
        std::fs::create_dir_all(&path).unwrap();
    }

    path
});

pub fn profiling_output(run_name: Option<&str>) -> std::path::PathBuf {
    let mut output = SONIC_BENCH_TRACES_PATH.clone();

    if let Some(ref run_name) = run_name {
        let id = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        output = output
            .join(run_name)
            .with_extension(id.to_string())
            .with_added_extension("trace");
    }

    output
}
