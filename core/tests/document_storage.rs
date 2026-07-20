// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Crisp IM SAS
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;

use crate::common::*;
use sonic::query::QueryTimeRange;
use sonic::store::document::{StoreDocument, StoreDocumentRecord};

fn upsert(executor: &ExecutorGuard, collection: &str, oid: &str, timestamp_ms: u64, text: &str) {
    upsert_in_bucket(executor, collection, "default", oid, timestamp_ms, text);
}

fn upsert_in_bucket(
    executor: &ExecutorGuard,
    collection: &str,
    bucket: &str,
    oid: &str,
    timestamp_ms: u64,
    text: &str,
) {
    let document = StoreDocument::new(
        oid,
        timestamp_ms,
        text,
        serde_json::json!({"kind": "message"}),
    )
    .unwrap();
    let lexer = sonic::lexer::TokenLexerBuilder::from(
        sonic::lexer::TokenLexerMode::NormalizeAndCleanup,
        None,
        text,
        executor.app_conf.normalization,
        executor.app_conf.tokenization,
    )
    .unwrap();
    executor
        .upsert(object_ref!(collection, bucket, oid), lexer, document)
        .unwrap();
}

fn query_documents(
    executor: &ExecutorGuard,
    collection: &str,
    term: &str,
    range: Option<QueryTimeRange>,
) -> Vec<StoreDocument> {
    query_documents_in_bucket(executor, collection, "default", term, range)
}

fn query_documents_in_bucket(
    executor: &ExecutorGuard,
    collection: &str,
    bucket: &str,
    term: &str,
    range: Option<QueryTimeRange>,
) -> Vec<StoreDocument> {
    let lexer = sonic::lexer::TokenLexerBuilder::from(
        sonic::lexer::TokenLexerMode::NormalizeAndCleanup,
        None,
        term,
        executor.app_conf.normalization,
        executor.app_conf.tokenization,
    )
    .unwrap();
    executor
        .search_documents(bucket_ref!(collection, bucket), "", lexer, 10, 0, range)
        .unwrap()
}

#[test]
fn test_bulk_upsert_handles_shared_postings_and_multiple_buckets() {
    init_logging();
    let executor = make_test_executor();
    let records = vec![
        StoreDocumentRecord {
            bucket: "current".to_owned(),
            document: StoreDocument::new("movie:1", 1_000, "shared first", serde_json::json!({}))
                .unwrap(),
        },
        StoreDocumentRecord {
            bucket: "current".to_owned(),
            document: StoreDocument::new("movie:2", 2_000, "shared second", serde_json::json!({}))
                .unwrap(),
        },
        StoreDocumentRecord {
            bucket: "archive".to_owned(),
            document: StoreDocument::new("movie:3", 3_000, "shared archive", serde_json::json!({}))
                .unwrap(),
        },
    ];
    assert_eq!(
        executor
            .upsert_batch("movies", records, true)
            .unwrap()
            .written,
        3
    );
    assert_eq!(
        query_documents_in_bucket(&executor, "movies", "current", "shared", None)
            .iter()
            .map(|document| document.oid.as_str())
            .collect::<Vec<_>>(),
        ["movie:2", "movie:1"]
    );
    assert_eq!(
        query_documents_in_bucket(&executor, "movies", "archive", "shared", None)[0].oid,
        "movie:3"
    );
    let stats = executor.stats("movies", true).unwrap();
    assert_eq!(stats.schema_version, 14);
    let logical = stats.logical.unwrap();
    assert_eq!(logical.document_count, 3);
    assert_eq!(logical.term_postings.associations, 6);
    assert_eq!(logical.time_postings.associations, 3);
    assert!(logical.document_encoded_bytes >= logical.document_text_bytes);

    let replacement = StoreDocumentRecord {
        bucket: "current".to_owned(),
        document: StoreDocument::new("movie:1", 4_000, "replacement", serde_json::json!({}))
            .unwrap(),
    };
    assert_eq!(
        executor
            .upsert_batch("movies", vec![replacement], false)
            .unwrap()
            .written,
        1
    );
    assert!(query_documents_in_bucket(&executor, "movies", "current", "first", None).is_empty());
}

#[test]
fn test_upsert_replaces_document_and_filters_by_date() {
    init_logging();
    let executor = make_test_executor();
    upsert(&executor, "messages", "message:1", 1_000, "common original");
    upsert(&executor, "messages", "message:2", 2_000, "common recent");

    let results = query_documents(
        &executor,
        "messages",
        "common",
        Some(QueryTimeRange::new(1_500, 2_500).unwrap()),
    );
    assert_eq!(
        results
            .iter()
            .map(|doc| doc.oid.as_str())
            .collect::<Vec<_>>(),
        ["message:2"]
    );
    assert_eq!(
        query_documents(
            &executor,
            "messages",
            "common",
            Some(QueryTimeRange::new(2_000, 2_000).unwrap()),
        )[0]
        .oid,
        "message:2"
    );

    upsert(&executor, "messages", "message:2", 500, "replacement");
    assert!(query_documents(&executor, "messages", "recent", None).is_empty());
    assert_eq!(
        query_documents(&executor, "messages", "replacement", None)[0].timestamp_ms,
        500
    );
}

#[test]
fn test_document_export_import_rebuilds_index() {
    init_logging();
    let source = make_test_executor();
    upsert(&source, "source", "message:1", 1_000, "portable document");
    upsert_in_bucket(
        &source,
        "source",
        "archive",
        "message:2",
        2_000,
        "archived document",
    );
    let path = std::env::temp_dir().join(format!(
        "sonic-documents-{}-{}.ndjson.zst",
        std::process::id(),
        common::util::unique_hex().unwrap()
    ));
    assert_eq!(source.export_documents("source", None, &path), Ok(2));

    let target = make_test_executor();
    assert_eq!(target.import_documents("target", &path), Ok(2));
    let results = query_documents(&target, "target", "portable", None);
    assert_eq!(results[0].text, "portable document");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn test_document_lifecycle_rejects_partial_mutations_and_flushes() {
    init_logging();
    let executor = make_test_executor();
    upsert(
        &executor,
        "messages",
        "message:1",
        1_000,
        "managed document",
    );
    let push_lexer = sonic::lexer::TokenLexerBuilder::from(
        sonic::lexer::TokenLexerMode::NormalizeAndCleanup,
        None,
        "extra",
        executor.app_conf.normalization,
        executor.app_conf.tokenization,
    )
    .unwrap();
    assert_eq!(
        executor.push(object_ref!("messages", "default", "message:1"), push_lexer),
        Err(())
    );
    let pop_lexer = sonic::lexer::TokenLexerBuilder::from(
        sonic::lexer::TokenLexerMode::NormalizeOnly,
        None,
        "managed",
        executor.app_conf.normalization,
        executor.app_conf.tokenization,
    )
    .unwrap();
    assert_eq!(
        executor.pop(object_ref!("messages", "default", "message:1"), pop_lexer),
        Err(())
    );
    assert_eq!(
        executor.flusho(object_ref!("messages", "default", "message:1")),
        Ok(2)
    );
    assert!(query_documents(&executor, "messages", "managed", None).is_empty());

    upsert(&executor, "messages", "message:2", 2_000, "bucket cleanup");
    assert_eq!(executor.flushb(bucket_ref!("messages", "default")), Ok(1));
    assert!(query_documents(&executor, "messages", "cleanup", None).is_empty());
}
