// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

/// Automatically kills the child process on [Drop].
pub struct SpawnGuard(pub std::process::Child);

impl SpawnGuard {
    /// Wait until the child listens on the expected address.
    pub fn wait_until_ready(&mut self, addr: SocketAddr) {
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
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
