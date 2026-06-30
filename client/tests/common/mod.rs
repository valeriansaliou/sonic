// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::net::{Ipv6Addr, SocketAddr};

#[must_use]
pub fn start_sonic() -> (SpawnGuard, SocketAddr) {
    use std::process::Command;
    use std::sync::atomic::{AtomicU16, Ordering};
    use std::time::Duration;

    static COUNTER: AtomicU16 = AtomicU16::new(0);

    let counter = COUNTER.fetch_add(1, Ordering::SeqCst);
    let addr = SocketAddr::from((Ipv6Addr::LOCALHOST, 1550 + counter));

    let sonic_bin_path = concat!(env!("CARGO_TARGET_TMPDIR"), "/../debug/sonic");
    let sonic_data_path = std::path::Path::new(concat!(env!("CARGO_TARGET_TMPDIR"), "/data"));
    // let sonic_config_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/config.cfg");

    if sonic_data_path.exists() {
        std::fs::remove_dir_all(sonic_data_path).unwrap();
    }

    let sonic = Command::new(sonic_bin_path)
        // .args(["-c", sonic_config_path])
        .env("SONIC_CHANNEL__INET", addr.to_string())
        .env("SONIC_SERVER__LOG_LEVEL", "WARN")
        .env("SONIC_STORE__KV__PATH", sonic_data_path.join("kv"))
        .env("SONIC_STORE__FST__PATH", sonic_data_path.join("fst"))
        .spawn()
        .unwrap();

    // Auto-kill Sonic.
    let mut sonic = SpawnGuard(sonic);

    std::thread::sleep(Duration::from_millis(500));
    if let Some(status) = sonic.try_wait().unwrap() {
        panic!("Sonic exited with {status}")
    };
    // println!("Started Sonic");

    (sonic, addr)
}

pub struct SpawnGuard(pub std::process::Child);

impl std::ops::Deref for SpawnGuard {
    type Target = std::process::Child;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for SpawnGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Drop for SpawnGuard {
    fn drop(&mut self) {
        self.0.kill().unwrap();
        self.0.wait().unwrap();
    }
}
