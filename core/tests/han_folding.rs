// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

//! Feature: Han folding

mod common;

use crate::common::*;

/// Simplified and Traditional Chinese should be usable interchangeably.
#[test]
#[ignore = "Not supported yet"] // TIP: For this, see crate `deunicode`.
fn test_chinese_folding() {
    init_logging();

    #[rustfmt::skip]
    let examples = [
        ("圖書館", "图书馆"),
        ("图书馆", "圖書館"),
    ];

    for (n, (message, term)) in examples.into_iter().enumerate() {
        let executor = make_test_executor_with_id(n);

        exec!(executor -> PUSH "messages" "user:1" "chat:1" message);
        exec!(executor -> TRIGGER consolidate);

        let response = exec!(executor -> QUERY "messages" "user:1" term);
        assert_eq!(response, ["chat:1"]);
    }
}

/// Chinese should support mixed input.
#[test]
#[ignore = "Not supported yet"] // TIP: For this, see crate `deunicode`.
fn test_chinese_mixed_input() {
    init_logging();

    #[rustfmt::skip]
    let examples = [
        ("北京", "bei"),
        ("北jing", "北京"),
    ];

    for (n, (message, term)) in examples.into_iter().enumerate() {
        let executor = make_test_executor_with_id(n);

        exec!(executor -> PUSH "messages" "user:1" "chat:1" message);
        exec!(executor -> TRIGGER consolidate);

        let response = exec!(executor -> QUERY "messages" "user:1" term);
        assert_eq!(response, ["chat:1"]);
    }
}
