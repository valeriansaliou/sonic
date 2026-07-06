// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

#![allow(dead_code)]

mod logging;
mod path_guard;
mod spawn_guard;

#[allow(unused_imports)]
pub mod prelude {
    pub use sonic_client::SonicMultiplexer;
    pub use sonic_client::control::SonicChannelControlBlocking;
    pub use sonic_client::ingest::SonicChannelIngestBlocking;
    pub use sonic_client::search::SonicChannelSearchBlocking;

    pub use crate::common::{start_empty, start_prepopulated};
}

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{
        LazyLock,
        atomic::{AtomicU16, Ordering},
    },
    time::Duration,
};

use path_guard::PathGuard;
use sonic_client::{
    SonicMultiplexer, control::SonicChannelControlBlocking, ingest::SonicChannelIngestBlocking,
    options::Lang,
};
use spawn_guard::SpawnGuard;

use crate::common::logging::init_logging;

static INSTANCE_COUNTER: AtomicU16 = AtomicU16::new(0);
static TEST_COUNTER: AtomicU16 = AtomicU16::new(0);

// NOTE: We initialize `SONIC_BIN_PATH` lazily to avoid logging the
//   “Environment variable "SONIC_BIN" not found” warning on start.
static SONIC_BIN_PATH: LazyLock<PathBuf> = LazyLock::new(|| {
    std::env::var("SONIC_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let path = Path::new(env!("CARGO_TARGET_TMPDIR"))
                .parent()
                .unwrap()
                .join("debug/sonic");
            eprintln!("Environment variable \"SONIC_BIN\" not found, using local build");
            path
        })
});

pub struct TestData {
    pub id: u16,
    pub addr: std::net::SocketAddr,
    spawn_guard: SpawnGuard,
    data_guard: PathGuard,
}

#[must_use]
fn start_sonic(
    addr: &std::net::SocketAddr,
    data_path: &Path,
    update_command: impl FnOnce(&mut Command) -> &mut Command,
) -> SpawnGuard {
    init_logging();

    eprintln!("Testing using {:?}", SONIC_BIN_PATH.as_path());
    let sonic = update_command(
        Command::new(SONIC_BIN_PATH.as_path())
            // .args(["-c", sonic_config_path])
            .env("SONIC_CHANNEL__INET", addr.to_string())
            .env("SONIC_SERVER__LOG_LEVEL", "WARN"),
    )
    .env("SONIC_STORE__KV__PATH", data_path.join("kv"))
    .env("SONIC_STORE__FST__PATH", data_path.join("fst"))
    .spawn()
    .unwrap();

    // Auto-kill Sonic.
    let sonic = SpawnGuard(sonic);

    let start = std::time::Instant::now();
    let multiplexer = SonicMultiplexer::new().unwrap();
    let mut error: Option<std::io::Error> = None;
    while start.elapsed() < Duration::from_secs(1) {
        use sonic_client::control::SonicChannelControlBlocking;

        match SonicChannelControlBlocking::connect(*addr, "SecretPassword", &multiplexer) {
            Ok(channel) => match channel.ping() {
                Ok(()) => {
                    error = None;
                    break;
                }
                Err(err) => {
                    error = Some(err);
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                }
            },
            Err(err) => {
                error = Some(err);
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
        }
    }

    if let Some(error) = error {
        panic!("{error}");
    }

    // println!("Started Sonic");

    sonic
}

#[must_use]
pub fn start_empty(update_command: impl FnOnce(&mut Command) -> &mut Command) -> TestData {
    let test_id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);

    let addr = new_addr();
    let data_path = new_data_path(test_id);

    let spawn_guard = start_sonic(&addr, &data_path, update_command);

    let data_guard = PathGuard(data_path);

    TestData {
        id: test_id,
        addr,
        spawn_guard,
        data_guard,
    }
}

#[must_use]
pub fn start_prepopulated(update_command: impl FnOnce(&mut Command) -> &mut Command) -> TestData {
    let test_id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);

    let addr = new_addr();
    let data_path = new_data_path(test_id);

    let spawn_guard = start_sonic(&addr, &data_path, update_command);

    let data_guard = PathGuard(data_path);

    {
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
            SonicChannelIngestBlocking::connect(addr, "SecretString", &multiplexer).unwrap();
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
            SonicChannelControlBlocking::connect(addr, "SecretString", &multiplexer).unwrap();
        sonic.trigger_consolidate().unwrap();
        drop(sonic);
    }

    TestData {
        id: test_id,
        addr,
        spawn_guard,
        data_guard,
    }
}

fn new_addr() -> std::net::SocketAddr {
    use std::net::Ipv6Addr;

    let counter = INSTANCE_COUNTER.fetch_add(1, Ordering::SeqCst);
    std::net::SocketAddr::from((Ipv6Addr::LOCALHOST, 1650 + counter))
}

const DATA_DIR_PATH: &str = concat!(env!("CARGO_TARGET_TMPDIR"), "/test-data");

fn new_data_path(test_id: u16) -> PathBuf {
    let path = Path::new(DATA_DIR_PATH).join(test_id.to_string());

    if path.exists() {
        std::fs::remove_dir_all(&path).unwrap();
    }

    path
}
