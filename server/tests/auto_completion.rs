// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;

use crate::common::*;

/// Search queries should be auto-completed.
#[test]
fn test_search_auto_completion() {
    init_logging();

    #[rustfmt::skip]
    let examples = [
        ("Have you received the document?", "doc"),
        ("J’ai besoin d’un café 🥱", "caf"),
        ("北京", "北"),
    ];

    for (n, (message, term)) in examples.into_iter().enumerate() {
        let executor = make_test_executor_with_id(n);

        exec!(executor -> PUSH "messages" "user:1" "chat:1" message);
        exec!(executor -> TRIGGER consolidate);

        let response = exec!(executor -> QUERY "messages" "user:1" term);
        assert_eq!(response, ["chat:1"]);
    }
}
