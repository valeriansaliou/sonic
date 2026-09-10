// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

/// Automatically kills the child process on [Drop].
pub struct SpawnGuard {
    pub(super) sonic: std::process::Child,
    pub(super) xctrace: Option<std::process::Child>,
}

impl SpawnGuard {
    pub fn new(sonic: std::process::Child) -> Self {
        Self {
            sonic,
            xctrace: None,
        }
    }

    /// Wait until the child listens on the expected address.
    pub fn wait_until_ready(&mut self, addr: impl Into<SocketAddr>) {
        let addr = addr.into();
        let deadline = Instant::now() + Duration::from_secs(30);

        loop {
            if let Some(status) = self.try_wait().unwrap() {
                panic!("Sonic exited with {status}");
            }
            if TcpStream::connect_timeout(&addr, Duration::from_millis(100)).is_ok() {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "Sonic did not listen on {addr} within 30 seconds"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl std::ops::Deref for SpawnGuard {
    type Target = std::process::Child;

    fn deref(&self) -> &Self::Target {
        &self.sonic
    }
}

impl std::ops::DerefMut for SpawnGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.sonic
    }
}

impl Drop for SpawnGuard {
    fn drop(&mut self) {
        if let Some(xctrace) = self.xctrace.as_mut() {
            std::process::Command::new("kill")
                .args(["-INT", &xctrace.id().to_string()])
                .status()
                .unwrap();
            xctrace.wait().unwrap();
        }

        self.sonic.kill().unwrap();
        self.sonic.wait().unwrap();
    }
}
