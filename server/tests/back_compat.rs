// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

//! Backward compatibility tests.
//!
//! Sometimes the library behaves a certain way and we want to ensure future
//! API changes don’t affect it.

mod common;

use crate::common::*;

/// Highlights the fact that without FST consolidation, queries seem out of
/// sync with insertions.
#[test]
fn test_consolidation_required() {
    init_logging();
    let executor = make_test_executor();

    assert_eq!(exec!(executor -> COUNT "foo"), Ok(0));

    exec!(executor -> PUSH "foo" "bar" "baz" "Example");

    assert_eq!(exec!(executor -> COUNT "foo"), Ok(0));
    assert_eq!(exec!(executor -> COUNT "foo" "bar"), Ok(0));

    exec!(executor -> TRIGGER consolidate);

    assert_eq!(exec!(executor -> COUNT "foo"), Ok(1));
    assert_eq!(exec!(executor -> COUNT "foo" "bar"), Ok(1));
}

/// Ensures that pushing some text creates the associated collection and bucket.
#[test]
fn test_push_creates_collection_and_bucket() {
    init_logging();
    let executor = make_test_executor();

    assert_eq!(exec!(executor -> COUNT "foo"), Ok(0));

    exec!(executor -> PUSH "foo" "bar" "baz" "Example");

    exec!(executor -> TRIGGER consolidate);

    assert_eq!(exec!(executor -> COUNT "foo"), Ok(1));
    assert_eq!(exec!(executor -> COUNT "foo" "bar"), Ok(1));
}
