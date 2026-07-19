// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Crisp IM SAS
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;

use crate::common::*;

#[test]
fn test_common_term_keeps_more_than_one_thousand_objects() {
    init_logging();
    let executor = make_test_executor();

    for index in 0..1_005 {
        let oid = format!("message:{index}");
        executor
            .push(
                object_ref!("messages", "default", &oid),
                sonic::lexer::TokenLexerBuilder::from(
                    sonic::lexer::TokenLexerMode::NormalizeAndCleanup,
                    None,
                    "commonterm",
                    executor.app_conf.normalization,
                    executor.app_conf.tokenization,
                )
                .unwrap(),
            )
            .unwrap();
    }

    let results = executor
        .search(
            bucket_ref!("messages", "default"),
            "",
            sonic::lexer::TokenLexerBuilder::from(
                sonic::lexer::TokenLexerMode::NormalizeAndCleanup,
                None,
                "commonterm",
                executor.app_conf.normalization,
                executor.app_conf.tokenization,
            )
            .unwrap(),
            5,
            1_000,
        )
        .unwrap();

    assert_eq!(
        results,
        [
            "message:4",
            "message:3",
            "message:2",
            "message:1",
            "message:0"
        ]
    );
}
