// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicU16;
use std::sync::{Arc, LazyLock, atomic};

use sonic_client::SonicMultiplexer;

use crate::globals::SONIC_BIN_PATH;
use crate::util::logging::LOG_LEVEL;

#[cfg(feature = "analysis")]
pub mod analysis;
pub mod globals;
#[cfg(feature = "huggingface")]
pub mod huggingface;
pub mod prepopulate;
#[cfg(feature = "profiling")]
pub mod profiling;
pub mod util;

pub mod prelude {
    pub use sonic_client::SonicMultiplexer;
    pub use sonic_client::control::SonicChannelControlBlocking;
    pub use sonic_client::ingest::SonicChannelIngestBlocking;
    pub use sonic_client::options::*;
    pub use sonic_client::search::SonicChannelSearchBlocking;

    pub use crate::constant;
    pub use crate::globals::*;
    pub use crate::util::itertools::Join as _;
    pub use crate::util::logging::{LoggingOptions, init_logging};
    pub use crate::util::random;
    pub use crate::{Ingestable, RunContext, new_empty_data_path};
}

pub struct RunContext {
    pub id: u16,
    pub seed: LazyLock<u64>,
    pub addr: std::net::SocketAddr,
    pub password: String,
    pub multiplexer: Arc<SonicMultiplexer>,
    sonic: Option<std::process::Child>,
    xctrace: Option<(std::process::Child, std::path::PathBuf)>,
    data_path: Option<PathBuf>,
    cleanup_on_drop: bool,
}

impl RunContext {
    pub fn new(cleanup_on_drop: bool) -> Self {
        static RUN_COUNTER: AtomicU16 = AtomicU16::new(0);

        let run_id = RUN_COUNTER.fetch_add(1, atomic::Ordering::SeqCst);

        Self {
            id: run_id,
            seed: LazyLock::new(util::random::random_seed),
            addr: (std::net::Ipv6Addr::LOCALHOST, 1491 + run_id).into(),
            password: String::new(),
            multiplexer: Arc::new(SonicMultiplexer::new().unwrap()),
            sonic: None,
            xctrace: None,
            data_path: None,
            cleanup_on_drop,
        }
    }
}

pub fn new_empty_data_path(base: impl AsRef<Path>, test_id: u16) -> PathBuf {
    let path = base.as_ref().join(test_id.to_string());

    if path.exists() {
        std::fs::remove_dir_all(&path).unwrap();
    }

    std::fs::create_dir_all(&path).unwrap();

    path
}

impl RunContext {
    /// Starts Sonic, without waiting for it to be ready.
    ///
    /// Call [`wait_until_ready`] to ensure Sonic is ready.
    ///
    /// [`wait_until_ready`]: Self::wait_until_ready
    pub fn start_sonic(
        &mut self,
        data_path: &Path,
        password: String,
        update_command: impl FnOnce(&mut Command) -> &mut Command,
    ) {
        // let sonic_config_path = concat!(env!("CARGO_MANIFEST_DIR"), "/benches/config.cfg");

        eprintln!();
        tracing::info!("Running {:?}", SONIC_BIN_PATH.as_path());

        if !data_path.exists() {
            panic!("No such directory: {data_path:?}");
        }

        // NOTE: Sonic is started for nothing if `profiling_mode` contains an
        //   incorrect value but it’s not a big deal.
        let sonic = update_command(Command::new(SONIC_BIN_PATH.as_path()).env(
            "SONIC_SERVER__LOG_LEVEL",
            LOG_LEVEL.map_or("WARN".to_owned(), |level| level.to_string()),
        ))
        .env("SONIC_STORE__KV__PATH", data_path.join("kv"))
        .env("SONIC_STORE__FST__PATH", data_path.join("fst"))
        .env("SONIC_STORE__OBJECT__PATH", data_path.join("object"))
        .spawn()
        .unwrap();
        self.sonic = Some(sonic);
        self.data_path = Some(data_path.to_owned());
        self.password = password;
        tracing::debug!("Started Sonic");

        //         if let Some(profiling_mode) = PROFILING_MODE.as_ref() {
        //             self.start_profiling(profiling_mode);
        //         }
        //
        //         self.wait_until_ready();
        //         tracing::debug!("Sonic is ready");
    }

    /// Wait until Sonic is ready.
    pub fn wait_until_ready(&mut self) {
        use std::net::TcpStream;
        use std::time::{Duration, Instant};

        let sonic = self.sonic.as_mut().unwrap();
        let addr = self.addr;

        let timeout = Duration::from_secs(30);
        let deadline = Instant::now() + timeout;

        loop {
            if let Some(status) = sonic.try_wait().unwrap() {
                panic!("Sonic exited with {status}");
            }

            if TcpStream::connect_timeout(&addr, Duration::from_millis(100)).is_ok() {
                tracing::debug!("Sonic is ready");

                return;
            }

            assert!(
                Instant::now() < deadline,
                "Sonic did not listen on {addr} within {timeout:?}"
            );

            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for RunContext {
    fn drop(&mut self) {
        println!();
        tracing::info!("Shutting down…");

        // Stop profiling.
        if let Some((xctrace, xctrace_output)) = self.xctrace.as_mut() {
            std::process::Command::new("kill")
                .args(["-INT", &xctrace.id().to_string()])
                .status()
                .unwrap();
            xctrace.wait().unwrap();

            if xctrace_output.ends_with(".trace") {
                tracing::info!("Saved profiling trace as {xctrace_output:?}");
            } else {
                tracing::info!("Saved profiling trace in {xctrace_output:?}");
            }
        }

        // Print analytics if enabled.
        #[cfg(feature = "analysis")]
        self.print_index_size_if_enabled();

        // Kill Sonic.
        if let Some(mut sonic) = self.sonic.take() {
            sonic.kill().unwrap();
            sonic.wait().unwrap();
        }

        if !self.cleanup_on_drop {
            if let Some(ref data_path) = self.data_path {
                tracing::info!("Data path: {data_path:?}");
            }

            return;
        }

        // Delete Sonic data if requested.
        if let Some(data_path) = self.data_path.take()
            && data_path.exists()
        {
            if data_path.is_dir() {
                std::fs::remove_dir_all(&data_path).unwrap();
            } else {
                std::fs::remove_file(&data_path).unwrap();
            }
        }
    }
}

pub trait Ingestable {
    fn id(&self) -> &str;

    fn title(&self) -> &str {
        self.id()
    }

    fn data(&self) -> &str;

    fn size_char(size: usize) -> char;
}
