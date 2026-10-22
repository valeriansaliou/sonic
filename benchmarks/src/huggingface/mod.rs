// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

#![allow(dead_code)]

use std::sync::LazyLock;

pub mod download;
pub mod load;
pub mod wikipedia;
pub mod wikipedia_common;

pub static NSHARDS: LazyLock<Option<usize>> = LazyLock::new(|| {
    std::env::var("NSHARDS")
        .map(|s| s.parse::<usize>().unwrap())
        .ok()
});
