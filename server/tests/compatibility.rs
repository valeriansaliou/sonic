// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

//! Tests that v1.x.x configurations are compatible when updating Sonic.

mod common;

use std::process::Command;

use crate::common::{SONIC_BIN_PATH, prelude::*, wait_until_ready};

#[test]
fn test_config_compatibility() {
    let read_dir = std::fs::read_dir(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("packaged-configs"),
    )
    .unwrap();

    for entry in read_dir {
        let entry = entry.unwrap();

        eprintln!("Testing {}", entry.file_name().into_string().unwrap());

        let sonic = Command::new(SONIC_BIN_PATH.as_path())
            .arg("-c")
            .arg(entry.path())
            .spawn()
            .unwrap();

        // Auto-kill Sonic.
        let sonic = SpawnGuard(sonic);

        wait_until_ready(&(std::net::Ipv6Addr::LOCALHOST, 1491).into()).unwrap();

        drop(sonic);
    }
}
