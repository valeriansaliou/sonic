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

use crate::common::data::WIKIPEDIA_PARAGRAPHS_SEARCH_ENGINE;
use crate::common::*;

fn main() -> Result<(), std::io::Error> {
    eprintln!("\n=== Create multiplexer ===");
    let mut multiplexer = timed!({ SonicMultiplexer::new()? });

    let collection = "collection";
    let bucket = "bucket";

    timed!({
        eprintln!("\n=== Setup ===");
        let ingest = SonicChannelIngestBlocking::connect(
            (Ipv6Addr::LOCALHOST, 1491),
            "SecretPassword",
            &mut multiplexer,
        )?;

        for (i, p) in WIKIPEDIA_PARAGRAPHS_SEARCH_ENGINE.into_iter().enumerate() {
            ingest.push_with_options(
                collection,
                bucket,
                format!("object:{i}"),
                p,
                &[&Lang("eng")],
            )?;
        }

        let control = SonicChannelControlBlocking::connect(
            (Ipv6Addr::LOCALHOST, 1491),
            "SecretPassword",
            &mut multiplexer,
        )?;

        control.trigger_consolidate()?;
    });

    let mut search = timed!({
        eprintln!("\n=== START search ===");
        let search = SonicChannelSearchBlocking::connect(
            (Ipv6Addr::LOCALHOST, 1491),
            "SecretPassword",
            &mut multiplexer,
        )?;
        eprintln!("Version: {}", search.server_info().version);
        eprintln!("Channel: {:?}", search.channel_info());
        search
    });

    timed!({
        eprintln!("\n=== List (simple) ===");
        let res = search.list(collection, bucket)?;
        eprintln!("List (simple) result: {res:?}");
    });

    timed!({
        eprintln!("\n=== List (with options) ===");
        let res = search.list_with_options(collection, bucket, &[&Limit(10), &Offset(20)])?;
        eprintln!("List (with options) result: {res:?}");
    });

    timed!({
        eprintln!("\n=== Query (simple) ===");
        let res = search.query(collection, bucket, "engine")?;
        eprintln!("Query (simple) result: {res:?}");
    });

    timed!({
        eprintln!("\n=== Query (with options) ===");
        let res = search.query_with_options(
            collection,
            bucket,
            "engine",
            &[&Limit(10), &Lang("eng"), &Offset(0)],
        )?;
        eprintln!("Query (with options) result: {res:?}");
    });

    timed!({
        eprintln!("\n=== Quit search ===");
        search.quit()?;
    });

    eprintln!("\n=== Drop all ===");
    Ok(())
}
