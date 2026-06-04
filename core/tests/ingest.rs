// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;

use sonic::{
    lexer::{TokenLexerBuilder, TokenLexerMode},
    query::QuerySearchLimit,
};

use crate::common::{init_logging, item_ref::*, make_test_executor};

// Copyright: 2019, Nikita Vilunov <nikitaoryol@gmail.com>
#[test]
fn test_ingest_and_query() {
    init_logging();
    let executor = make_test_executor();

    let expected_documents = [
        (
            "conversation:1",
            "Batch normalization is a technique for improving the speed, \
            performance, and stability of artificial neural networks",
        ),
        (
            "conversation:2",
            "This scratch technique is much like the transform in some ways",
        ),
    ];

    let unexpected_documents = [(
        "conversation:3",
        "Glissando is a glide from one pitch to another",
    )];

    // Ingest documents
    for (key, value) in expected_documents.iter().chain(unexpected_documents.iter()) {
        () = executor
            .push(
                object_ref!("messages", "default", key),
                TokenLexerBuilder::from(TokenLexerMode::NormalizeOnly, value).unwrap(),
            )
            .unwrap();
    }

    // Perform search on ingested documents
    let response = executor
        .search(
            bucket_ref!("messages", "default"),
            "",
            TokenLexerBuilder::from(TokenLexerMode::NormalizeOnly, "technique").unwrap(),
            QuerySearchLimit::MAX,
            0,
        )
        .unwrap();

    for (key, _) in expected_documents.iter() {
        if !response.iter().any(|s| s == key) {
            assert!(false, "Expected document {key} was not found");
        }
    }

    for (key, _) in unexpected_documents.iter() {
        if response.iter().any(|s| s == key) {
            assert!(false, "Unexpected document {key} was returned");
        }
    }
}
