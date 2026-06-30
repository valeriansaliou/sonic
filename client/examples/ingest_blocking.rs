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
use sonic_client::options::Lang;

use crate::common::*;

fn main() -> Result<(), std::io::Error> {
    eprintln!("\n=== Create multiplexer ===");
    let mut multiplexer = timed!({ SonicMultiplexer::new()? });

    let collection = "collection";
    let bucket = "bucket";

    let ingest = timed!({
        eprintln!("\n=== START ingest ===");
        let ingest = SonicChannelIngestBlocking::connect(
            (Ipv6Addr::LOCALHOST, 1491),
            "SecretPassword",
            &mut multiplexer,
        )?;
        eprintln!("Version: {}", ingest.server_info().version);
        eprintln!("Channel: {:?}", ingest.channel_info());
        ingest
    });

    timed!({
        eprintln!("\n=== Ingest (simple) ×100 ===");
        for i in 1..=100 {
            ingest.push(
                collection,
                bucket,
                format!("object:{i}"),
                "The quick brown fox jumps over the lazy dog.",
            )?;
        }
    });

    timed!({
        eprintln!("\n=== Ingest (with options) ×100 ===");
        for i in 101..=200 {
            ingest.push_with_options(
                collection,
                bucket,
                format!("object:{i}"),
                "The quick brown fox jumps over the lazy dog.",
                &[&Lang("eng")],
            )?;
        }
    });

    timed!({
        eprintln!("\n=== Drop ingest ===");
        drop(ingest);
    });

    let control = timed!({
        eprintln!("\n=== START control ===");
        let control = SonicChannelControlBlocking::connect(
            (Ipv6Addr::LOCALHOST, 1491),
            "SecretPassword",
            &mut multiplexer,
        )?;
        eprintln!("Version: {}", control.server_info().version);
        eprintln!("Channel: {:?}", control.channel_info());
        control
    });

    timed!({
        eprintln!("\n=== Consolidate ===");
        control.trigger_consolidate()?;
        eprintln!("Consolidation successful");
    });

    timed!({
        eprintln!("\n=== Server stats ===");
        let res = control.info()?;
        eprintln!("Server stats: {res:?}");
    });

    timed!({
        eprintln!("\n=== Drop control ===");
        drop(control);
    });

    eprintln!("\n=== Drop all ===");
    Ok(())
}
