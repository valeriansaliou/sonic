// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicU16;
use std::sync::{LazyLock, atomic};

use crate::common::globals::{ADDR, SONIC_BIN_PATH, SONIC_DATA_PATH};
use crate::common::path_guard::PathGuard;
use crate::common::spawn_guard::SpawnGuard;

pub mod client_helpers;
pub mod huggingface;
mod itertools;
pub mod logging;
mod path_guard;
pub mod random;
pub mod spawn_guard;

#[allow(unused_imports)]
pub mod prelude {
    pub use sonic_client::SonicMultiplexer;
    pub use sonic_client::control::SonicChannelControlBlocking;
    pub use sonic_client::ingest::SonicChannelIngestBlocking;
    pub use sonic_client::options::*;
    pub use sonic_client::search::SonicChannelSearchBlocking;

    pub use crate::common::RunContext;
    pub use crate::common::globals::*;
    pub use crate::common::itertools::Join as _;
    pub use crate::common::logging::{LoggingOptions, init_logging};
    pub use crate::common::random;
    pub use crate::common::{start_sonic, start_sonic_empty};
}

pub mod globals {
    use std::net::Ipv6Addr;
    use std::path::{Path, PathBuf};
    use std::sync::LazyLock;

    pub const ADDR: (Ipv6Addr, u16) = (Ipv6Addr::LOCALHOST, 1491);

    pub(crate) const SONIC_PASSWORD: &str = "SecretPassword";

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
                        Path::new(env!("CARGO_TARGET_TMPDIR"))
                            .parent()
                            .unwrap()
                            .join(profile)
                            .join("sonic")
                    })
            })
            .unwrap_or_else(|_| {
                let path = Path::new(env!("CARGO_TARGET_TMPDIR"))
                    .parent()
                    .unwrap()
                    .join("release/sonic");
                tracing::warn!("Environment variable \"SONIC_BIN\" not found, using local build");
                path
            })
    });

    pub const SONIC_DATA_PATH: &str = concat!(
        env!("CARGO_TARGET_TMPDIR"),
        "/bench-data/",
        env!("CARGO_CRATE_NAME")
    );
}

static TEST_COUNTER: AtomicU16 = AtomicU16::new(0);

pub struct RunContext {
    pub id: u16,
    pub seed: LazyLock<u64>,
    pub addr: std::net::SocketAddr,
    spawn_guard: SpawnGuard,
    data_guard: PathGuard,
}

fn new_data_path(test_id: u16) -> PathBuf {
    let path = Path::new(SONIC_DATA_PATH).join(test_id.to_string());

    if path.exists() {
        std::fs::remove_dir_all(&path).unwrap();
    }

    path
}

pub fn start_sonic_empty(update_command: impl FnOnce(&mut Command) -> &mut Command) -> RunContext {
    let run_id = TEST_COUNTER.fetch_add(1, atomic::Ordering::SeqCst);

    let data_path = new_data_path(run_id);

    let spawn_guard = start_sonic(&data_path, update_command);

    let data_guard = PathGuard(data_path);

    RunContext {
        id: run_id,
        seed: LazyLock::new(random::random_seed),
        addr: ADDR.into(),
        spawn_guard,
        data_guard,
    }
}

#[must_use]
pub fn start_sonic(
    data_path: &Path,
    update_command: impl FnOnce(&mut Command) -> &mut Command,
) -> SpawnGuard {
    // let sonic_config_path = concat!(env!("CARGO_MANIFEST_DIR"), "/benches/config.cfg");

    eprint!("\n");
    tracing::info!("Benchmarking using {:?}", SONIC_BIN_PATH.as_path());
    let sonic = update_command(
        Command::new(SONIC_BIN_PATH.as_path())
            // .args(["-c", sonic_config_path])
            .env("SONIC_SERVER__LOG_LEVEL", "WARN"),
    )
    .env("SONIC_STORE__KV__PATH", data_path.join("kv"))
    .env("SONIC_STORE__FST__PATH", data_path.join("fst"))
    .spawn()
    .unwrap();

    // Auto-kill Sonic.
    let mut sonic = SpawnGuard(sonic);
    sonic.wait_until_ready(ADDR);
    // println!("Started Sonic");

    sonic
}

pub fn no_prepopulate(_ctx: &RunContext) {}

pub fn prepopulate_gdpr(ctx: &RunContext) {
    use sonic_client::SonicMultiplexer;
    use sonic_client::control::SonicChannelControlBlocking;
    use sonic_client::ingest::SonicChannelIngestBlocking;
    use sonic_client::options::Lang;

    let multiplexer = SonicMultiplexer::new().unwrap();

    // NOTE: This is NOT legal advice. It is solely for example purposes.
    let articles = [
        "The GDPR applies to any organization—regardless of where it is located—\
            that processes the personal data of people in the European Union or \
            European Economic Area in connection with offering goods or services \
            to them or monitoring their behavior.",
        "GDPR compliance means implementing the technical, organizational, \
            and legal measures required by the GDPR to protect personal data \
            and uphold individuals’ privacy rights.",
        "The European Union establishes regulations and directives that create \
            common legal standards across its member states in areas such as \
            privacy, competition, consumer protection, and digital markets.",
        "Brussels is a major center for technology policy and digital \
            regulation, shaping rules that influence companies and software \
            services worldwide.",
    ];

    let sonic =
        SonicChannelIngestBlocking::connect(ctx.addr, "SecretString", &multiplexer).unwrap();
    for (i, text) in articles.into_iter().enumerate() {
        sonic
            .push_with_options(
                "articles",
                "default",
                format!("article:{}", i + 1),
                text,
                &[&Lang("eng")],
            )
            .unwrap();
    }
    drop(sonic);

    let sonic =
        SonicChannelControlBlocking::connect(ctx.addr, "SecretString", &multiplexer).unwrap();

    sonic.trigger_consolidate().unwrap();

    // NOTE: Do not flush even when Sonic supports `TRIGGER flush`, as
    //   it would cause differences between old and new versions of
    //   Sonic.

    drop(sonic);
}
