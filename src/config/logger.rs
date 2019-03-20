// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Sonic OSS License v1.0 (SOSSL v1.0)

use log;
use log::{Level, LevelFilter, Metadata, Record, SetLoggerError};

pub struct ConfigLogger;

impl log::Log for ConfigLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Debug
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            println!("({}) - {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

impl ConfigLogger {
    pub fn init(level: LevelFilter) -> Result<(), SetLoggerError> {
        log::set_max_level(level);
        log::set_logger(&ConfigLogger)
    }
}
