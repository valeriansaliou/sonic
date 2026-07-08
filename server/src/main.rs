// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

#![deny(
    clippy::all,
    dead_code,
    unstable_features,
    unused_imports,
    unused_qualifications
)]
#![warn(
    clippy::inline_always, // Do not use unless benchmarked (explicit allow).
)]
#![allow(
    clippy::collapsible_if, // Style preference.
    clippy::explicit_auto_deref, // Style preference.
    clippy::needless_as_bytes, // Style preference. Better make those things explicit.
    clippy::needless_borrow, // Style preference.
    clippy::needless_borrows_for_generic_args, // Style preference.
)]

mod channel;
mod config;
mod logger;
mod tasker;

use std::ops::Deref;
use std::str::FromStr;
use std::sync::{Arc, LazyLock};
use std::thread;
use std::time::Duration;

use clap::{Arg, Command};

use channel::listen::{ChannelListen, ChannelListenBuilder};
use channel::statistics::ensure_states as ensure_states_channel_statistics;
use sonic::store::fst::StoreFSTPool;
use sonic::store::kv::StoreKVPool;
use tasker::runtime::TaskerBuilder;
use tasker::shutdown::ShutdownSignal;
use tracing::level_filters::LevelFilter;

use crate::config::{Config, read_config};
use crate::logger::ConfigLogger;

struct AppArgs {
    config: String,
}

#[cfg(unix)]
#[cfg(feature = "allocator-jemalloc")]
#[global_allocator]
static ALLOC: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

pub static LINE_FEED: &str = "\r\n";

pub static THREAD_NAME_CHANNEL_MASTER: &str = "sonic-channel-master";
pub static THREAD_NAME_CHANNEL_CLIENT: &str = "sonic-channel-client";
pub static THREAD_NAME_TASKER: &str = "sonic-tasker";

static APP_ARGS: LazyLock<AppArgs> = LazyLock::new(make_app_args);

const DEFAULT_CONFIG_FILE_PATH: &str = "./config.cfg";

fn make_app_args() -> AppArgs {
    let matches = Command::new(clap::crate_name!())
        .version(clap::crate_version!())
        .author(clap::crate_authors!())
        .about(clap::crate_description!())
        .arg(
            Arg::new("config")
                .short('c')
                .long("config")
                .help("Path to configuration file")
                .default_value(DEFAULT_CONFIG_FILE_PATH),
        )
        .get_matches();

    // Generate owned app arguments
    AppArgs {
        config: matches
            .get_one::<String>("config")
            .expect("invalid config value")
            .to_owned(),
    }
}

fn main() {
    ConfigLogger::init(
        std::env::var("SONIC_SERVER__LOG_LEVEL")
            .map(|level| LevelFilter::from_str(&level).expect("invalid log level"))
            .unwrap_or(LevelFilter::DEBUG),
    );

    let app_conf = read_config(&APP_ARGS.config);

    ConfigLogger::update(
        LevelFilter::from_str(&app_conf.server.log_level).expect("invalid log level"),
    );

    let shutdown_signal = ShutdownSignal::new();

    tracing::info!("starting up");

    // Ensure all states are bound
    ensure_states();

    // Create connection pools (does not open any connection yet)
    let kv_pool = StoreKVPool::new(Arc::clone(&app_conf.sonic.store.kv));
    let fst_pool = StoreFSTPool::new(Arc::clone(&app_conf.sonic.store.fst), Default::default());

    // Spawn tasker (background thread)
    thread::spawn(spawn_tasker(kv_pool.clone(), fst_pool.clone()));

    // Spawn channel (foreground thread)
    thread::spawn(spawn_channel(
        kv_pool.clone(),
        fst_pool.clone(),
        Arc::new(app_conf),
    ));

    tracing::info!("started");

    shutdown_signal.at_exit(move |signal| {
        tracing::info!("stopping gracefully (got signal: {})", signal);

        // Teardown Sonic Channel
        ChannelListen::teardown();

        // Perform a KV flush (ensures all in-memory changes are synced on-disk before shutdown)
        kv_pool.flush(true);

        // Perform a FST consolidation (ensures all in-memory items are synced on-disk before \
        //   shutdown; otherwise we would lose all non-consolidated FST changes)
        fst_pool.consolidate(true);

        tracing::info!("stopped");
    });
}

fn ensure_states() {
    // Ensure all statics are valid (a `deref` is enough to lazily initialize them)
    let _ = APP_ARGS.deref();

    // Ensure per-module states
    ensure_states_channel_statistics();
}

fn spawn_managed_thread<F, T>(name: &'static str, thread_name: &'static str, task: F)
where
    F: Fn() -> T + Clone,
    F: Send + 'static,
    T: Send + 'static,
{
    tracing::debug!("spawn managed thread: {name}");

    let worker = thread::Builder::new()
        .name(thread_name.to_string())
        .spawn(task.clone());

    // Block on worker thread (join it)
    let has_error = if let Ok(worker_thread) = worker {
        worker_thread.join().is_err()
    } else {
        true
    };

    if has_error {
        tracing::error!("managed thread crashed ({name}), setting it up again");

        // Prevents thread start loop floods
        thread::sleep(Duration::from_secs(1));

        spawn_managed_thread(name, thread_name, task);
    }
}

fn spawn_channel(
    kv_pool: StoreKVPool,
    fst_pool: StoreFSTPool,
    app_conf: Arc<Config>,
) -> impl FnOnce() {
    let builder = ChannelListenBuilder {
        app_conf,
        kv_pool,
        fst_pool,
    };

    move || {
        spawn_managed_thread("channel", THREAD_NAME_CHANNEL_MASTER, move || {
            builder.build().run();
        })
    }
}

fn spawn_tasker(kv_pool: StoreKVPool, fst_pool: StoreFSTPool) -> impl FnOnce() {
    let builder = TaskerBuilder { kv_pool, fst_pool };

    move || {
        spawn_managed_thread("tasker", THREAD_NAME_TASKER, move || {
            builder.build().run();
        })
    }
}
