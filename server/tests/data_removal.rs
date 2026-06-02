// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

//! Feature: Data removal

mod common;

use crate::common::*;

fn setup(executor: &ExecutorGuard) {
    exec!(executor -> PUSH "messages" "user:1" "chat:1" "Can I remove data from the index?");
    exec!(executor -> PUSH "messages" "user:1" "chat:1" "Yes, you can!");
    exec!(executor -> PUSH "messages" "user:2" "chat:1" "Hey you!"); // "chat:1" reused on purpose, to test edge cases
    exec!(executor -> TRIGGER consolidate);

    let response = exec!(executor -> LIST "messages" "user:1");
    assert_contains!(response, ["yes", "you", "can"]);
}

/// Collections can be de-indexed.
#[test]
#[ignore = "Known issue (FIXME)"]
fn test_collection_deindexing() {
    init_logging();
    let executor = make_test_executor();

    setup(&executor);

    let response = exec!(executor -> FLUSHC "messages");
    assert_eq!(response, 2, "Expecting 2 buckets in “messages”");

    let response = exec!(executor -> LIST "messages" "user:1");
    assert_eq!(response, vec![] as Vec<String>);
}

/// Buckets can be de-indexed.
#[test]
fn test_bucket_deindexing() {
    init_logging();
    let executor = make_test_executor();

    setup(&executor);

    let response = exec!(executor -> FLUSHB "messages" "user:1");
    assert_eq!(response, 1, "Expecting 1 bucket named “user:1”");

    let response = exec!(executor -> LIST "messages" "user:1");
    assert_eq!(response, vec![] as Vec<String>);
}

/// Individual items can be de-indexed.
#[test]
#[ignore = "Known issue (FIXME)"]
fn test_object_deindexing() {
    init_logging();
    let executor = make_test_executor();

    setup(&executor);

    let count = exec!(executor -> COUNT "messages" "user:1" "can").unwrap();
    tracing::debug!("Count: {count}");
    let response1 = exec!(executor -> FLUSHO "messages" "user:1" "can");
    exec!(executor -> TRIGGER consolidate);
    let response2 = exec!(executor -> FLUSHO "messages" "user:1" "can");

    assert_eq!(
        response1, 2,
        "1: Expecting 2 occurences of “can” {response1} {response2}"
    );
    assert_eq!(response2, 2, "2: Expecting 2 occurences of “can”");

    let response = exec!(executor -> LIST "messages" "user:1");
    assert!(!response.contains(&"can".to_owned()));
    assert!(response.contains(&"yes".to_owned()));
}
