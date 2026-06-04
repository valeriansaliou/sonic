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

/// See <https://github.com/valeriansaliou/sonic/issues/262>.
#[test]
#[ignore = "Known issue (FIXME)"]
fn issue_262() {
    #[rustfmt::skip]
    let test_cases = [
        ("I met darren", true),
        ("darren yesterday", true),
    ];

    // Sanity check: explicit locales at ingestion and query work as expected.
    test_ingest_then_query!("I met darren yesterday. Great fun!" LANG("eng"), test_cases LANG("eng"));

    // This is what the user reported (used to not work).
    test_ingest_then_query!("I met darren yesterday. Great fun!", test_cases);

    // I should also work this way.
    // It’s common to know the language when ingesting, but not at query time.
    test_ingest_then_query!("I met darren yesterday. Great fun!" LANG("eng"), test_cases);
}

/// See <https://github.com/valeriansaliou/sonic/issues/245>.
#[test]
fn issue_245() {
    #[rustfmt::skip]
    test_ingest_then_query!("Veronika Šibanová Veronika Sibanova", [
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
