// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::sync::{LazyLock, Once};

use tracing_subscriber::EnvFilter;

pub(crate) static LOG_LEVEL: LazyLock<tracing::Level> =
    LazyLock::new(
        move || match std::env::var("LOG_LEVEL").map(|s| s.to_ascii_uppercase()) {
            Ok(level) if level == "TRACE" => tracing::Level::TRACE,
            Ok(level) if level == "DEBUG" => tracing::Level::DEBUG,
            Ok(level) if level == "INFO" => tracing::Level::INFO,
            Ok(level) if level == "WARN" => tracing::Level::WARN,
            _ => tracing::Level::DEBUG,
        },
    );
static INIT_LOGGING: Once = Once::new();

pub fn init_logging(filter: &str, with_file: bool, with_line_number: bool) {
    INIT_LOGGING.call_once(|| {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::new(format!("{},{filter}", *LOG_LEVEL)))
            .with_target(true)
            .with_file(with_file)
            .with_line_number(with_line_number)
            .without_time()
            .with_level(true)
            .with_thread_ids(true)
            .with_writer(tracing_subscriber::fmt::TestWriter::new)
            .with_ansi(true)
            .init();
    });
}
