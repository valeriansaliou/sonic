// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::prelude::*;

pub struct ConfigLogger;

impl ConfigLogger {
    pub fn init(level: LevelFilter) {
        tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .with_level(true)
                    .with_target(false)
                    .with_span_events(FmtSpan::NONE)
                    .with_filter(level),
            )
            .init();
    }
}
