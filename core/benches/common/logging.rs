// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::sync::{LazyLock, Once};

pub(crate) static LOG_LEVEL: LazyLock<tracing::Level> =
    LazyLock::new(
        move || match std::env::var("LOG_LEVEL").map(|s| s.to_ascii_uppercase()) {
            Ok(level) if level == "TRACE" => tracing::Level::TRACE,
            Ok(level) if level == "DEBUG" => tracing::Level::DEBUG,
            Ok(level) if level == "INFO" => tracing::Level::INFO,
            Ok(level) if level == "WARN" => tracing::Level::WARN,
            _ => tracing::Level::WARN,
        },
    );
static INIT_LOGGING: Once = Once::new();

pub fn init_logging() {
    INIT_LOGGING.call_once(|| {
        tracing_subscriber::fmt()
            .with_max_level(*LOG_LEVEL)
            .with_target(false)
            .with_file(true)
            .with_line_number(true)
            .without_time()
            .with_level(true)
            .with_writer(tracing_subscriber::fmt::TestWriter::new)
            .init();
    });
}
