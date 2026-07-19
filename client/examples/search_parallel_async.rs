// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;

use std::net::Ipv6Addr;
use std::sync::Arc;

use sonic_client::SonicMultiplexer;
use sonic_client::control::SonicChannelControlAsync;
use sonic_client::ingest::SonicChannelIngestAsync;
use sonic_client::options::Lang;
use sonic_client::search::SonicChannelSearchAsync;

use crate::common::data::WIKIPEDIA_PARAGRAPHS_SEARCH_ENGINE;
use crate::common::*;

const ADDR: (Ipv6Addr, u16) = (Ipv6Addr::LOCALHOST, 1491);
/// WARN: DON’T HARDCODE A PASSWORD IN PRODUCTION CODE! This is just an example!
const PASS: &str = "SecretPassword";
const COLLECTION: &str = "collection";
const BUCKET: &str = "bucket";

async fn worker(multiplexer: Arc<SonicMultiplexer>, queries: &[&str]) -> std::io::Result<()> {
    let mut conn = SonicChannelSearchAsync::connect(ADDR, PASS, &multiplexer)?;

    for &query in queries.into_iter() {
        conn.query_with_options(COLLECTION, BUCKET, query, &[&Lang("eng")])
            .await?;
    }

    timed!({
        eprintln!("\n=== Quit search ===");
        conn.quit().await?;
    });

    Ok(())
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), std::io::Error> {
    let start = std::time::Instant::now();

    eprintln!("\n=== Create multiplexer ===");
    let mut multiplexer = timed!({ Arc::new(SonicMultiplexer::new()?) });

    timed!({
        eprintln!("\n=== Setup ===");
        let ingest = SonicChannelIngestAsync::connect(ADDR, PASS, &mut multiplexer)?;

        for (i, p) in WIKIPEDIA_PARAGRAPHS_SEARCH_ENGINE.into_iter().enumerate() {
            ingest
                .push_with_options(
                    COLLECTION,
                    BUCKET,
                    format!("object:{i}"),
                    p,
                    &[&Lang("eng")],
                )
                .await?;
        }

        let control = SonicChannelControlAsync::connect(ADDR, PASS, &mut multiplexer)?;

        control.trigger_consolidate().await?;
    });

    #[rustfmt::skip]
    let t1 = tokio::spawn(worker(
        Arc::clone(&multiplexer),
        &["search", "engine", "probabili"],
    ));
    #[rustfmt::skip]
    let t2 = tokio::spawn(worker(
        Arc::clone(&multiplexer),
        &["database", "relevance", "index"],
    ));
    #[rustfmt::skip]
    let t3 = tokio::spawn(worker(
        Arc::clone(&multiplexer),
        &["criteria", "information", "similar"],
    ));

    t1.await.unwrap()?;
    t2.await.unwrap()?;
    t3.await.unwrap()?;

    eprintln!("\nTotal execution time: {:.3?}", start.elapsed());

    eprintln!("\n=== Drop all ===");

    Ok(())
}
