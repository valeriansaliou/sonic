// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

#![allow(dead_code)]

use std::sync::LazyLock;

pub mod download;
pub mod load;

pub static NSHARDS: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("NSHARDS").map_or_else(
        |_err| {
            let default = 4;
            tracing::info!("`NSHARDS` not configured, using {default:?} as default.");
            default
        },
        |s| s.parse::<usize>().unwrap(),
    )
});
