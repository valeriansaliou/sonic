// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

//! Feature: Sub-object matching
//!
//! Sonic v2 should be able to index and match and de-index individual portions
//! of an object. Think about a chat. Sonic would have to:
//!
//! - Index messages one by one.
//! - Be able to tell which specific message matches a query.
//! - Be able to de-index a single message (e.g. message deleted).

mod common;

use crate::common::*;

/// Ensures `QUERY` still yields the same results whether content was pushed
/// with a fragment identifier or not.
#[test]
fn test_sub_object_query_backcompat() {
    init_logging();
    let executor = make_test_executor(|_| {});

    exec!(executor -> PUSH "messages" "user:1" "chat:1" "msg:1" "Wow! Just found out about Sonic!");
    exec!(executor -> PUSH "messages" "user:1" "chat:1" "msg:2" "Have you noticed how fast it is?");
    exec!(executor -> TRIGGER consolidate);

    let response = exec!(executor -> QUERY "messages" "user:1" "Sonic");
    assert_eq!(response, ["chat:1"]);

    let response = exec!(executor -> QUERY "messages" "user:1" "fast");
    assert_eq!(response, ["chat:1"]);
}
