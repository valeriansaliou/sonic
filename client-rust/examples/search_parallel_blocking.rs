// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;

use std::net::Ipv6Addr;
use std::sync::Arc;

use sonic_client::SonicMultiplexer;
use sonic_client::control::SonicChannelControlBlocking;
use sonic_client::ingest::SonicChannelIngestBlocking;
use sonic_client::options::Lang;
use sonic_client::search::SonicChannelSearchBlocking;

use crate::common::data::WIKIPEDIA_PARAGRAPHS_SEARCH_ENGINE;
use crate::common::*;

const ADDR: (Ipv6Addr, u16) = (Ipv6Addr::LOCALHOST, 1491);
/// WARN: DON’T HARDCODE A PASSWORD IN PRODUCTION CODE! This is just an example!
const PASS: &str = "SecretPassword";
const COLLECTION: &str = "collection";
const BUCKET: &str = "bucket";

fn worker(multiplexer: Arc<SonicMultiplexer>, queries: &[&str]) -> std::io::Result<()> {
    let mut conn = SonicChannelSearchBlocking::connect(ADDR, PASS, &multiplexer)?;

    for &query in queries.into_iter() {
        conn.query_with_options(COLLECTION, BUCKET, query, &[&Lang("eng")])?;
        conn.suggest(COLLECTION, BUCKET, query)?;
    }

    timed!({
        eprintln!("\n=== Quit search ===");
        conn.quit()?;
    });

    Ok(())
}

fn main() -> Result<(), std::io::Error> {
    let start = std::time::Instant::now();

    eprintln!("\n=== Create multiplexer ===");
    let mut multiplexer = timed!({ Arc::new(SonicMultiplexer::new()?) });

    timed!({
        eprintln!("\n=== Setup ===");
        let ingest = SonicChannelIngestBlocking::connect(ADDR, PASS, &mut multiplexer)?;

        for (i, p) in WIKIPEDIA_PARAGRAPHS_SEARCH_ENGINE.into_iter().enumerate() {
            ingest.push_with_options(
                COLLECTION,
                BUCKET,
                format!("object:{i}"),
                p,
                &[&Lang("eng")],
            )?;
        }

        let control = SonicChannelControlBlocking::connect(ADDR, PASS, &mut multiplexer)?;

        control.trigger_consolidate()?;
    });

    #[rustfmt::skip]
    let t1 = std::thread::spawn({
        let multiplexer = Arc::clone(&multiplexer);
        move || worker(
            multiplexer,
            &["search", "engine", "probabili"],
        )
    });
    #[rustfmt::skip]
    let t2 = std::thread::spawn({
        let multiplexer = Arc::clone(&multiplexer);
        move || worker(
            multiplexer,
            &["database", "relevance", "index"],
        )
    });
    #[rustfmt::skip]
    let t3 = std::thread::spawn({
        let multiplexer = Arc::clone(&multiplexer);
        move || worker(
            multiplexer,
            &["criteria", "information", "similar"],
        )
    });

    t1.join().unwrap()?;
    t2.join().unwrap()?;
    t3.join().unwrap()?;

    eprintln!("\nTotal execution time: {:.3?}", start.elapsed());

    eprintln!("\n=== Drop all ===");

    Ok(())
}
