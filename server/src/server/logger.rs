// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::sync::OnceLock;

use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::{Registry, prelude::*, reload};

static LOG_LEVEL_RELOAD_HANDLE: OnceLock<reload::Handle<LevelFilter, Registry>> = OnceLock::new();

pub struct ConfigLogger;

impl ConfigLogger {
    /// Initialize the global tracing subscriber (usually with a more verbose
    /// level until the config is loaded).
    pub fn init(level: LevelFilter) {
        let (filter, handle) = reload::Layer::new(level);

        tracing_subscriber::registry()
            .with(filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .with_level(true)
                    .with_target(false)
                    .with_span_events(FmtSpan::NONE),
            )
            .init();

        LOG_LEVEL_RELOAD_HANDLE.set(handle).expect(
            "Logger should be initialized once. \
            `Registry::init` would panic before anyway.",
        );
    }

    /// Update the global tracing subscriber level (e.g. after loading the
    /// configuration).
    pub fn update(level: LevelFilter) {
        let handle = LOG_LEVEL_RELOAD_HANDLE
            .get()
            .expect("Logging must be initialized before.");

        handle.modify(|f| *f = level).unwrap();
    }
}
