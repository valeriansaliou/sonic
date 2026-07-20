// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Crisp IM SAS
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;

use crate::common::prelude::*;
use sonic_client::ingest::{BulkDocument, BulkMode};
use sonic_client::options::{FromTimestamp, ToTimestamp};
use sonic_client::search::Document;

#[test]
fn query_documents_filters_by_timestamp() {
    let ctx = start_empty(|command| command);
    let multiplexer = SonicMultiplexer::new().unwrap();
    let ingest =
        SonicChannelIngestBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();
    let search =
        SonicChannelSearchBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();

    ingest
        .upsert(
            "messages",
            "default",
            "message:1",
            "hello old",
            1_000,
            &serde_json::json!({"author": "alice"}),
        )
        .unwrap();
    ingest
        .upsert(
            "messages",
            "default",
            "message:2",
            "hello recent",
            2_000,
            &serde_json::json!({"author": "bob"}),
        )
        .unwrap();

    let documents = search
        .query_documents(
            "messages",
            "default",
            "hello",
            &[&FromTimestamp(1_500), &ToTimestamp(2_500)],
        )
        .unwrap();
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].oid, "message:2");
    assert_eq!(documents[0].metadata["author"], "bob");
}

#[test]
fn bulk_upsert_round_trips_over_protocol() {
    let ctx = start_empty(|command| command);
    let multiplexer = SonicMultiplexer::new().unwrap();
    let ingest =
        SonicChannelIngestBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();
    let search =
        SonicChannelSearchBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();
    let documents = (0u64..200)
        .map(|index| {
            let mut state = index.wrapping_add(1);
            let payload = (0..4096)
                .map(|_| {
                    state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    const ALPHABET: &[u8] =
                        b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_";
                    char::from(ALPHABET[(state as usize) % ALPHABET.len()])
                })
                .collect::<String>();
            BulkDocument {
                bucket: "default".to_owned(),
                document: Document {
                    oid: format!("message:{index}"),
                    timestamp_ms: index,
                    text: format!("bulk shared {index} {payload}"),
                    metadata: serde_json::json!({"index": index}),
                },
            }
        })
        .collect::<Vec<_>>();
    let result = ingest
        .upsert_batch("messages", BulkMode::Fresh, &documents)
        .unwrap();
    assert_eq!(result.written, 200);
    assert_eq!(result.rejected, 0);
    let results = search
        .query_documents("messages", "default", "shared", &[])
        .unwrap();
    assert_eq!(results.len(), 10);
    assert_eq!(results[0].oid, "message:199");
    let control =
        SonicChannelControlBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();
    let stats = control.stats("messages", true).unwrap();
    assert_eq!(stats.logical.unwrap().document_count, 200);
}
