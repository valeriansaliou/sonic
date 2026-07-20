// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;

use std::net::Ipv6Addr;

use sonic_client::SonicMultiplexer;
use sonic_client::control::SonicChannelControlBlocking;
use sonic_client::ingest::SonicChannelIngestBlocking;
use sonic_client::options::{Lang, Limit, Offset};
use sonic_client::search::SonicChannelSearchBlocking;
use sonic_client::transport::SonicStream;

use crate::common::*;

// type Transport = SonicStream;
type Transport = crate::common::logging_transport::Logging<SonicStream>;

fn main() -> Result<(), std::io::Error> {
    let start = std::time::Instant::now();

    eprintln!("\n=== Create multiplexer ===");
    let mut multiplexer = timed!({ SonicMultiplexer::new()? });

    let collection = "collection";
    let bucket = "bucket";

    let ingest = timed!({
        eprintln!("\n=== START ingest ===");
        let ingest = SonicChannelIngestBlocking::connect_custom::<Transport>(
            (Ipv6Addr::LOCALHOST, 1491),
            "SecretPassword",
            &mut multiplexer,
        )?;
        eprintln!("Version: {}", ingest.server_info().version);
        eprintln!("Channel: {:?}", ingest.channel_info());
        ingest
    });

    timed!({
        eprintln!("\n=== Ping ===");
        ingest.ping()?;
        eprintln!("Ping successful");
    });

    timed!({
        eprintln!("\n=== Ingest (simple) ===");
        ingest.push(
            collection,
            bucket,
            "object:1",
            "The quick brown fox jumps over the lazy dog.",
        )?;
    });

    timed!({
        eprintln!("\n=== Ingest (with options) ===");
        ingest.push_with_options(
            collection,
            bucket,
            "object:2",
            "Quick search engines return results fast.",
            &[&Lang("eng")],
        )?;
    });

    timed!({
        eprintln!("\n=== Drop ingest ===");
        drop(ingest);
    });

    let control = timed!({
        eprintln!("\n=== START control ===");
        let control = SonicChannelControlBlocking::connect_custom::<Transport>(
            (Ipv6Addr::LOCALHOST, 1491),
            "SecretPassword",
            &mut multiplexer,
        )?;
        eprintln!("Version: {}", control.server_info().version);
        eprintln!("Channel: {:?}", control.channel_info());
        control
    });

    timed!({
        eprintln!("\n=== Ping ===");
        control.ping()?;
        eprintln!("Ping successful");
    });

    timed!({
        eprintln!("\n=== Consolidate ===");
        control.trigger_consolidate()?;
        eprintln!("Consolidation successful");
    });

    let mut search = timed!({
        eprintln!("\n=== START search ===");
        let search = SonicChannelSearchBlocking::connect_custom::<Transport>(
            (Ipv6Addr::LOCALHOST, 1491),
            "SecretPassword",
            &mut multiplexer,
        )?;
        eprintln!("Version: {}", search.server_info().version);
        eprintln!("Channel: {:?}", search.channel_info());
        search
    });

    timed!({
        eprintln!("\n=== Ping ===");
        search.ping()?;
        eprintln!("Ping successful");
    });

    timed!({
        eprintln!("\n=== Query (simple) ===");
        let res = search.query(collection, bucket, "quick")?;
        eprintln!("Query (simple) result: {res:?}");
    });

    timed!({
        eprintln!("\n=== Query (with options) ===");
        let res = search.query_with_options(
            collection,
            bucket,
            "quick",
            &[&Limit(10), &Lang("eng"), &Offset(0)],
        )?;
        eprintln!("Query (with options) result: {res:?}");
    });

    timed!({
        eprintln!("\n=== List (simple) ===");
        let res = search.list(collection, bucket)?;
        eprintln!("List (simple) result: {res:?}");
    });

    timed!({
        eprintln!("\n=== List (with options) ===");
        let res = search.list_with_options(collection, bucket, &[&Limit(10), &Offset(0)])?;
        eprintln!("List (with options) result: {res:?}");
    });

    timed!({
        eprintln!("\n=== Quit search ===");
        search.quit()?;
    });

    timed!({
        eprintln!("\n=== Server stats ===");
        let res = control.info()?;
        eprintln!("Server stats: {res:?}");
    });

    eprintln!("\nTotal execution time: {:.3?}", start.elapsed());

    eprintln!("\n=== Drop all ===");
    Ok(())
}
