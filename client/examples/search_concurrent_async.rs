// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;

use std::net::Ipv6Addr;

use sonic_client::SonicMultiplexer;
use sonic_client::control::SonicChannelControlAsync;
use sonic_client::ingest::SonicChannelIngestBlocking;
use sonic_client::options::Lang;
use sonic_client::search::SonicChannelSearchAsync;

use crate::common::data::WIKIPEDIA_PARAGRAPHS_SEARCH_ENGINE;
use crate::common::*;

const COLLECTION: &str = "collection";
const BUCKET: &str = "bucket";

async fn task(conn: &SonicChannelSearchAsync, queries: &[&str]) -> std::io::Result<()> {
    for &query in queries.into_iter() {
        conn.query_with_options(COLLECTION, BUCKET, query, &[&Lang("eng")])
            .await?;
    }

    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), std::io::Error> {
    let start = std::time::Instant::now();

    eprintln!("\n=== Create multiplexer ===");
    let mut multiplexer = timed!({ SonicMultiplexer::new()? });

    timed!({
        eprintln!("\n=== Setup ===");
        let ingest = SonicChannelIngestBlocking::connect(
            (Ipv6Addr::LOCALHOST, 1491),
            "SecretPassword",
            &mut multiplexer,
        )?;

        for (i, p) in WIKIPEDIA_PARAGRAPHS_SEARCH_ENGINE.into_iter().enumerate() {
            ingest.push_with_options(
                COLLECTION,
                BUCKET,
                format!("object:{i}"),
                p,
                &[&Lang("eng")],
            )?;
        }

        let control = SonicChannelControlAsync::connect(
            (Ipv6Addr::LOCALHOST, 1491),
            "SecretPassword",
            &mut multiplexer,
        )?;

        control.trigger_consolidate().await?;
    });

    let mut search = timed!({
        eprintln!("\n=== START search ===");
        let search = SonicChannelSearchAsync::connect(
            (Ipv6Addr::LOCALHOST, 1491),
            "SecretPassword",
            &mut multiplexer,
        )?;
        eprintln!("Version: {}", search.server_info().version);
        eprintln!("Channel: {:?}", search.channel_info());
        search
    });

    #[rustfmt::skip]
    let task1 = task(
        &search,
        &["search", "engine", "probabili"],
    );
    #[rustfmt::skip]
    let task2 = task(
        &search,
        &["database", "relevance", "index"],
    );
    #[rustfmt::skip]
    let task3 = task(
        &search,
        &["criteria", "information", "similar"],
    );

    let (res1, res2, res3) = tokio::join!(task1, task2, task3);
    res1?;
    res2?;
    res3?;

    timed!({
        eprintln!("\n=== Quit search ===");
        search.quit().await?;
    });

    eprintln!("\nTotal execution time: {:.3?}", start.elapsed());

    eprintln!("\n=== Drop all ===");

    Ok(())
}
