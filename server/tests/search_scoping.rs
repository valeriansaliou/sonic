// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

//! Feature: Search scoping

mod common;

use crate::common::*;

/// Search queries can be scoped by bucket.
#[test]
fn test_search_scoping_by_bucket() {
    init_logging();
    let executor = make_test_executor();

    exec!(executor -> PUSH "messages" "user:1" "chat:1" "Hey! Have you noticed how fast Sonic is?");
    exec!(executor -> PUSH "messages" "user:2" "chat:2" "Wow! Just found out about Sonic!");
    exec!(executor -> TRIGGER consolidate);

    let response = exec!(executor -> QUERY "messages" "user:1" "Sonic");
    assert_eq!(response, ["chat:1"]);
}
