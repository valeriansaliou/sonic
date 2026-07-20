// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Crisp IM SAS
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

use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use clap::{Arg, Command};
use sonic_router::admin::AdminServer;
use sonic_router::config::Config;
use sonic_router::directory::Directory;
use sonic_router::error::{RouterError, RouterResult};
use sonic_router::proxy::ProxyServer;
use sonic_router::shutdown::{Shutdown, ShutdownSignal};
use tracing::level_filters::LevelFilter;

fn main() {
    if let Err(error) = run() {
        eprintln!("sonic-router failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> RouterResult<()> {
    let matches = Command::new(clap::crate_name!())
        .version(clap::crate_version!())
        .author(clap::crate_authors!())
        .about(clap::crate_description!())
        .arg(
            Arg::new("config")
                .short('c')
                .long("config")
                .help("Path to configuration file")
                .default_value("./router.cfg"),
        )
        .get_matches();
    let path = PathBuf::from(
        matches
            .get_one::<String>("config")
            .expect("config argument missing"),
    );

    let config = Config::read(&path)?;

    tracing_subscriber::fmt()
        .with_max_level(
            LevelFilter::from_str(&config.server.log_level)
                .map_err(|error| RouterError::context("invalid_log_level", error))?,
        )
        .init();

    let directory = Arc::new(Directory::open(&config.directory.path, &config.servers)?);

    let shutdown_signal = ShutdownSignal::new();
    let shutdown = Shutdown::default();
    let signal_shutdown = shutdown.clone();

    thread::Builder::new()
        .name("sonic-router-shutdown".to_owned())
        .spawn(move || {
            let signal = shutdown_signal.wait();
            tracing::info!("stopping gracefully (got signal: {signal})");
            signal_shutdown.request();
        })?;

    let topology_directory = Arc::clone(&directory);
    let topology_path = path.clone();
    let mut configured_servers = config.servers.clone();
    let topology_shutdown = shutdown.clone();

    let topology_thread = thread::Builder::new()
        .name("sonic-router-topology".to_owned())
        .spawn(move || {
            while !topology_shutdown.is_requested() {
                thread::sleep(Duration::from_secs(2));
                if topology_shutdown.is_requested() {
                    break;
                }
                match Config::read(&topology_path) {
                    Ok(updated) if updated.servers != configured_servers => {
                        match topology_directory.replace_backends(&updated.servers) {
                            Ok(()) => {
                                tracing::info!("reloaded {} Sonic servers", updated.servers.len());
                                configured_servers = updated.servers;
                            }
                            Err(error) => {
                                tracing::error!("rejected Sonic server topology: {error}");
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(error) => tracing::error!("cannot reload router config: {error}"),
                }
            }
        })?;

    let admin = AdminServer {
        address: config.admin.inet,
        auth_password: config.admin.auth_password,
        directory: Arc::clone(&directory),
        backend_timeout: Duration::from_secs(config.channel.tcp_timeout),
    };
    let admin_shutdown = shutdown.clone();
    let admin_failure_shutdown = shutdown.clone();

    let admin_thread = thread::Builder::new()
        .name("sonic-router-admin".to_owned())
        .spawn(move || {
            let result = admin.run(admin_shutdown);
            if result.is_err() {
                admin_failure_shutdown.request();
            }
            result
        })?;

    let proxy = ProxyServer {
        address: config.channel.inet,
        auth_password: config.channel.auth_password,
        tcp_timeout: config.channel.tcp_timeout,
        bulk_buffer_size: config.channel.bulk_buffer_size,
        directory,
    };

    let proxy_result = proxy.run(shutdown.clone());
    shutdown.request();

    topology_thread
        .join()
        .map_err(|_| RouterError::code("topology_thread_panicked"))?;
    let admin_result = admin_thread
        .join()
        .map_err(|_| RouterError::code("admin_thread_panicked"))?;

    proxy_result?;
    admin_result?;

    tracing::info!("router stopped");

    Ok(())
}
