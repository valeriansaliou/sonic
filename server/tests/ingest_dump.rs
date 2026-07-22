// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Crisp IM SAS
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;

use crate::common::prelude::*;
use sonic_client::ingest::{BulkDocument, BulkMode};
use sonic_client::search::Document;

#[test]
fn dump_bucket_paginates_and_round_trips_documents() {
    let ctx = start_empty(|command| command);
    let multiplexer = SonicMultiplexer::new().unwrap();
    let ingest =
        SonicChannelIngestBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();

    let documents = (0u64..5)
        .map(|index| BulkDocument {
            bucket: "user:1".to_owned(),
            document: Document {
                oid: format!("message:{index}"),
                timestamp_ms: index,
                text: format!("hello {index}"),
                metadata: serde_json::json!({"index": index}),
            },
        })
        .collect::<Vec<_>>();
    ingest
        .upsert_batch("messages", BulkMode::Fresh, &documents)
        .unwrap();

    let first_page = ingest.dump_bucket("messages", "user:1", 3, 0).unwrap();
    assert_eq!(first_page.len(), 3);

    let second_page = ingest.dump_bucket("messages", "user:1", 3, 3).unwrap();
    assert_eq!(second_page.len(), 2);

    let mut oids = first_page
        .iter()
        .chain(second_page.iter())
        .map(|record| record.document.oid.clone())
        .collect::<Vec<_>>();
    oids.sort();
    assert_eq!(
        oids,
        vec![
            "message:0",
            "message:1",
            "message:2",
            "message:3",
            "message:4"
        ]
    );
    assert!(first_page.iter().all(|record| record.bucket == "user:1"));
}

#[test]
fn dump_bucket_on_unknown_bucket_returns_empty() {
    let ctx = start_empty(|command| command);
    let multiplexer = SonicMultiplexer::new().unwrap();
    let ingest =
        SonicChannelIngestBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();

    ingest
        .push("messages", "user:1", "message:1", "hello world")
        .unwrap();

    let page = ingest.dump_bucket("messages", "user:2", 100, 0).unwrap();
    assert!(page.is_empty());
}

#[test]
fn list_buckets_enumerates_all_buckets_in_a_collection() {
    let ctx = start_empty(|command| command);
    let multiplexer = SonicMultiplexer::new().unwrap();
    let ingest =
        SonicChannelIngestBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();

    ingest
        .push("messages", "user:1", "message:1", "hello world")
        .unwrap();
    ingest
        .push("messages", "user:2", "message:2", "hello again")
        .unwrap();

    let mut buckets = ingest.list_buckets("messages", 100, 0).unwrap();
    buckets.sort();
    assert_eq!(buckets, vec!["user:1", "user:2"]);
}

#[test]
fn list_buckets_on_empty_collection_returns_empty() {
    let ctx = start_empty(|command| command);
    let multiplexer = SonicMultiplexer::new().unwrap();
    let ingest =
        SonicChannelIngestBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();

    let buckets = ingest.list_buckets("missing", 100, 0).unwrap();
    assert!(buckets.is_empty());
}
