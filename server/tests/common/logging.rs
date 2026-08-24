// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::sync::{LazyLock, Once};

use tracing_subscriber::EnvFilter;

pub(crate) static LOG_LEVEL: LazyLock<Option<tracing::Level>> =
    LazyLock::new(
        move || match std::env::var("LOG_LEVEL").map(|s| s.to_ascii_uppercase()) {
            Ok(level) if level == "TRACE" => Some(tracing::Level::TRACE),
            Ok(level) if level == "DEBUG" => Some(tracing::Level::DEBUG),
            Ok(level) if level == "INFO" => Some(tracing::Level::INFO),
            Ok(level) if level == "WARN" => Some(tracing::Level::WARN),
            _ => None,
        },
    );
static INIT_LOGGING: Once = Once::new();

pub fn init_logging(filter: Option<&str>, options: LoggingOptions) {
    let filter = {
        let mut res = format!(
            "{},{}={}",
            LOG_LEVEL.unwrap_or(options.default_log_level),
            env!("CARGO_CRATE_NAME"),
            LOG_LEVEL.unwrap_or(tracing::Level::INFO)
        );
        if let Some(filter) = filter {
            res = format!("{res},{filter}");
        }
        res
    };

    INIT_LOGGING.call_once(|| {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::new(filter))
            .with_target(options.with_target)
            .with_file(options.with_file)
            .with_line_number(options.with_line_number)
            .with_level(options.with_level)
            .with_thread_ids(options.with_thread_ids)
            .with_ansi(true)
            .without_time()
            .with_writer(tracing_subscriber::fmt::TestWriter::new)
            .init();
    });
}

pub struct LoggingOptions {
    pub default_log_level: tracing::Level,
    pub with_target: bool,
    pub with_file: bool,
    pub with_line_number: bool,
    pub with_level: bool,
    pub with_thread_ids: bool,
}

impl Default for LoggingOptions {
    fn default() -> Self {
        Self {
            default_log_level: tracing::Level::DEBUG,
            with_target: true,
            with_file: true,
            with_line_number: true,
            with_level: true,
            with_thread_ids: false,
        }
    }
}
