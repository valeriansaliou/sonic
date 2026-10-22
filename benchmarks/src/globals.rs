// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

#[macro_export]
macro_rules! constant {
    ($var:ident, default: $default:expr) => {
        $var.unwrap_or_else(|| {
            tracing::info!(
                "`{}` not configured, using {:?} as default.",
                stringify!($var),
                $default
            );
            $default
        })
    };
}

// NOTE: We initialize `SONIC_BIN_PATH` lazily to avoid logging the
//   “Environment variable "SONIC_BIN" not found” warning on
//   `--load-baseline`.
pub static SONIC_BIN_PATH: LazyLock<PathBuf> = LazyLock::new(|| {
    std::env::var("SONIC_BIN")
        .map(PathBuf::from)
        .or_else(|_| {
            std::env::var("CARGO_PROFILE")
                .map(|profile| {
                    if profile == "dev" {
                        "debug".to_owned()
                    } else {
                        profile
                    }
                })
                .map(|profile| {
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .parent()
                        .unwrap()
                        .join("target")
                        .join(profile)
                        .join("sonic")
                })
        })
        .unwrap_or_else(|_| {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join("target/release/sonic");
            tracing::warn!("Environment variable \"SONIC_BIN\" not found, using local build");
            path
        })
});

pub static SHOW_PROGRESS: LazyLock<bool> = LazyLock::new(|| std::env::var("SHOW_PROGRESS").is_ok());

pub static PUSH_USE_NEW: LazyLock<bool> = LazyLock::new(|| {
    std::env::var("PUSH_USE_NEW").map_or_else(
        |_err| {
            let default = false;
            tracing::info!("`PUSH_USE_NEW` not configured, using {default:?} as default.");
            default
        },
        |s| matches!(s.as_str(), "1" | "true"),
    )
});

pub static AUTO_CLEANUP: LazyLock<bool> = LazyLock::new(|| {
    std::env::var("AUTO_CLEANUP").map_or_else(
        |_err| {
            let default = true;
            tracing::info!("`AUTO_CLEANUP` not configured, using {default:?} as default.");
            default
        },
        |s| matches!(s.as_str(), "1" | "true"),
    )
});

pub static BENCH_CONF: LazyLock<String> = LazyLock::new(|| {
    std::env::var("BENCH_CONF").unwrap_or_else(|_err| {
        let default = "preset_bulk_load";
        tracing::info!("`BENCH_CONF` not configured, using {default:?} as default.");
        default.to_owned()
    })
});

pub static BENCH_CONFS: LazyLock<Vec<(&str, PathBuf)>> = LazyLock::new(|| {
    BENCH_CONF
        .split(",")
        .map(|name| {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("configs/bench")
                .join(name)
                .with_extension("toml");
            if !path.exists() {
                panic!("{path:?} doesn’t exist.")
            };
            (name, path)
        })
        .collect()
});

pub static SONIC_CONF: LazyLock<String> = LazyLock::new(|| {
    std::env::var("SONIC_CONF").unwrap_or_else(|_err| {
        let default = "frequent_tasks";
        tracing::info!("`SONIC_CONF` not configured, using {default:?} as default.");
        default.to_owned()
    })
});

pub static SONIC_CONFS: LazyLock<Vec<(&str, PathBuf)>> = LazyLock::new(|| {
    SONIC_CONF
        .split(",")
        .map(|name| {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("configs/sonic")
                .join(name)
                .with_extension("toml");
            if !path.exists() {
                panic!("{path:?} doesn’t exist.")
            };
            (name, path)
        })
        .collect::<Vec<_>>()
});
