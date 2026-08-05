// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

//! Non-regression tests.
//!
//! Ensures fixed bugs don’t come back.
//!
//! Tests are sorted by issue number, for easier navigation.

mod common;

use crate::common::*;

/// See <https://github.com/valeriansaliou/sonic/issues/166>.
#[test]
fn issue_166() {
    #[rustfmt::skip]
    test_ingest_then_query!(push: "Search Index", query: [
        ("search", true),
        ("earch", true),
    ]);
}

/// See <https://github.com/valeriansaliou/sonic/issues/173>.
#[test]
fn issue_173() {
    #[rustfmt::skip]
    test_ingest_then_query!(push: "Alexander Tipugin", query: [
        ("alexander", true),
        ("alex", true),
        ("lexander", true),
        ("exander", true), // Bug used to return `false`.
        ("exander Tipugin", true), // Bug used to return `false`.
    ]);
}

/// See <https://github.com/valeriansaliou/sonic/issues/245>.
#[test]
fn issue_245() {
    #[rustfmt::skip]
    test_ingest_then_query!(push: "Veronika Šibanová Veronika Sibanova", query: [
        ("Ve", true), // Bug used to return `false`.
        ("Ver", true), // Bug used to return `false`.
        ("Vero", true),
        ("Veron", true),
        ("Veroni", true),
        ("Veronik", true),
        ("Veronika", true),
        ("Veronika S", true),
        ("Veronika Si", true),
        ("Veronika Sib", true),
        ("Veronika Siba", true),
        ("Veronika Siban", true),
        ("Veronika Sibano", true),
        ("Veronika Sibanov", true),
        ("Veronika Sibanova", true),
        ("S", true), // Bug used to return `false`.
        ("Si", true), // Bug used to return `false`.
        ("Sib", true),
        ("Siba", true),
        ("Siban", true),
        ("Sibano", true),
        ("Sibanov", true),
        ("Sibanova", true),
        ("Sibanova V", true),
        ("Sibanova Ve", true), // Bug used to return `false`.
        ("Sibanova Ver", true), // Bug used to return `false`.
        ("Sibanova Vero", true),
        ("Sibanova Veron", true),
        ("Sibanova Veroni", true),
        ("Sibanova Veronik", true),
        ("Sibanova Veronika", true),
    ]);
}

/// See <https://github.com/valeriansaliou/sonic/issues/262>.
#[test]
fn issue_262() {
    #[rustfmt::skip]
    let test_cases = [
        ("I met darren", true),
        ("darren yesterday", true),
    ];

    // Sanity check: explicit locales at ingestion and query work as expected.
    test_ingest_then_query!(
        push: "I met darren yesterday. Great fun!" LANG("eng"),
        query: test_cases LANG("eng"),
    );

    // This is what the user reported (used to not work).
    test_ingest_then_query!(
        push: "I met darren yesterday. Great fun!",
        query: test_cases,
    );

    // I should also work this way.
    // It’s common to know the language when ingesting, but not at query time.
    test_ingest_then_query!(
        push: "I met darren yesterday. Great fun!" LANG("eng"),
        query: test_cases,
    );
}

/// See <https://github.com/valeriansaliou/sonic/issues/264>.
#[test]
#[ignore = "Not supported yet (missing tf)"]
fn issue_264() {
    #[rustfmt::skip]
    let test_cases = [
        (00, "Schizophrenia relapse"),
        (01, "Schizophrenia-like symptoms"),
        (02, "Schizophrenia, Latent"),
        (03, "Schizophrenia, Pseudoneurotic"),
        (04, "early onset schizophrenia"),
        (05, "FH: Schizophrenia"),
        (06, "Chronic disorganized schizophrenia"),
        (07, "Schizophrenia, process"),
        (08, "Schizophrenia, Childhood"),
        (09, "SCHIZOPHRENIA EPISODIC"),
        (10, "Chronic schizophrenia"),
        (11, "Incipient Schizophrenia"),
        (12, "Simple schizophrenia NOS"),
        (13, "Chronic paranoid schizophrenia"),
        (14, "Schizophrenia"),
        (15, "Late onset schizophrenia"),
        (16, "Chronic residual schizophrenia"),
        (17, "mixed schizophrenia"),
        (18, "Paranoid Schizophrenia"),
        (19, "Schizophrenia, Disorganized"),
    ];

    let executor = make_test_executor(|_| {});

    for (idx, text) in test_cases {
        let id = &format!("doc:{idx}");
        exec!(executor -> PUSH "docs" "default" id text LANG("eng"));
    }
    exec!(executor -> TRIGGER consolidate);

    let query = "schizophrenia";
    let response = exec!(executor -> QUERY "docs" "default" query);
    assert_eq!(
        response.first().map(String::as_str),
        Some("doc:14"),
        "response={response:?}"
    );
}
