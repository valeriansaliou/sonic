// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

#[macro_use]
extern crate log;
#[macro_use]
extern crate clap;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate serde_derive;
extern crate iso639_2;
extern crate rand;
extern crate rocksdb;
extern crate toml;
extern crate unicode_segmentation;

mod channel;
mod config;
mod lexer;
mod query;
mod store;

use std::ops::Deref;
use std::str::FromStr;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use clap::{App, Arg};
use log::LevelFilter;

use channel::listen::ChannelListenBuilder;
use config::config::Config;
use config::logger::ConfigLogger;
use config::reader::ConfigReader;
use store::fst::StoreFSTBuilder;
use store::kv::StoreKVBuilder;

struct AppArgs {
    config: String,
}

pub static LINE_FEED: &'static str = "\r\n";

pub static THREAD_NAME_CHANNEL_MASTER: &'static str = "sonic-channel-master";
pub static THREAD_NAME_CHANNEL_CLIENT: &'static str = "sonic-channel-client";

lazy_static! {
    static ref APP_ARGS: AppArgs = make_app_args();
    static ref APP_CONF: Config = ConfigReader::make();
}

fn make_app_args() -> AppArgs {
    let matches = App::new(crate_name!())
        .version(crate_version!())
        .author(crate_authors!("\n"))
        .about(crate_description!())
        .arg(
            Arg::with_name("config")
                .short("c")
                .long("config")
                .help("Path to configuration file")
                .default_value("./config.cfg")
                .takes_value(true),
        )
        .get_matches();

    // Generate owned app arguments
    AppArgs {
        config: String::from(matches.value_of("config").expect("invalid config value")),
    }
}

fn ensure_states() {
    // Ensure all statics are valid (a `deref` is enough to lazily initialize them)
    let (_, _) = (APP_ARGS.deref(), APP_CONF.deref());
}

fn main() {
    let _logger = ConfigLogger::init(
        LevelFilter::from_str(&APP_CONF.server.log_level).expect("invalid log level"),
    );

    info!("starting up");

    // Ensure all states are bound
    ensure_states();

    // Spawn channel (foreground thread)
    // Notice: this requires databases to be connected first
    match (StoreKVBuilder::new(), StoreFSTBuilder::new()) {
        (Ok(kv_store), Ok(fst_store)) => {
            ChannelListenBuilder::new().run(Arc::new(kv_store), Arc::new(fst_store))
        }
        (Err(err), _) => panic!("could not open key-value database: {}", err),
        (_, Err(err)) => panic!("could not open graph database: {}", err),
    }

    error!("failed to start");
}
