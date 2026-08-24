// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

//! Tests that v1.x.x configurations are compatible when updating Sonic.

mod common;

use std::process::{Command, Stdio};

use crate::common::{SONIC_BIN_PATH, prelude::*, wait_until_ready};

#[test]
fn test_config_compatibility() {
    init_logging(None, Default::default());

    let read_dir = std::fs::read_dir(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("packaged-configs"),
    )
    .unwrap();

    for entry in read_dir {
        let entry = entry.unwrap();

        tracing::info!("Testing {}", entry.file_name().into_string().unwrap());

        let sonic = Command::new(SONIC_BIN_PATH.as_path())
            .arg("-c")
            .arg(entry.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();

        // Auto-kill Sonic.
        let mut sonic = SpawnGuard(sonic);

        match wait_until_ready(&(std::net::Ipv6Addr::LOCALHOST, 1491).into()) {
            Ok(()) => drop(sonic),
            Err(err) => {
                std::io::copy(sonic.stdout.as_mut().unwrap(), &mut std::io::stdout()).unwrap();
                std::io::copy(sonic.stderr.as_mut().unwrap(), &mut std::io::stderr()).unwrap();

                panic!("{err}");
            }
        }
    }
}
