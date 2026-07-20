// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;

use crate::common::prelude::*;

#[test]
fn query() {
    let ctx = start_prepopulated(|command| command);

    let multiplexer = SonicMultiplexer::new().unwrap();

    let search =
        SonicChannelSearchBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();

    let res = search
        .query("articles", "default", "GDPR European Union")
        .unwrap();
    assert_eq!(
        res.as_slice(),
        &[
            Box::from("article:1"),
            Box::from("article:3"),
            Box::from("article:2")
        ]
    );
}

const SPECIAL_PATTERNS_TEST_CASES: [(&str, &str); 6] = [
    ("msg:1", "olivia@example.org"),
    ("msg:2", "olivio@example.org"),
    ("msg:3", "alicia@example.org"),
    ("msg:4", "olivia@example.com"),
    ("msg:5", "olivia and olivio"),
    ("msg:6", "olivia works at example.org"),
];

/// Shows that `tokenization.compat_split_special_patterns = false` yields the
/// best results.
#[test]
fn query_special() {
    let ctx = start_empty(|command| {
        command.env("SONIC_TOKENIZATION__COMPAT_SPLIT_SPECIAL_PATTERNS", "false")
    });

    let multiplexer = SonicMultiplexer::new().unwrap();

    let ingest =
        SonicChannelIngestBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();
    let search =
        SonicChannelSearchBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();

    for (id, text) in SPECIAL_PATTERNS_TEST_CASES {
        ingest.push("messages", "default", id, text).unwrap();
    }

    let res = search
        .query("messages", "default", "olivia@example.org")
        .unwrap();
    assert_eq!(res.as_slice(), &[Box::from("msg:1")]);
}

/// Shows that the compatibility setting works, although doing worse than with
/// `tokenization.compat_split_special_patterns = false`.
#[test]
fn query_special_compat() {
    let ctx = start_empty(|command| command);

    let multiplexer = SonicMultiplexer::new().unwrap();

    let ingest =
        SonicChannelIngestBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();
    let search =
        SonicChannelSearchBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();

    for (id, text) in SPECIAL_PATTERNS_TEST_CASES {
        ingest.push("messages", "default", id, text).unwrap();
    }

    let res = search
        .query("messages", "default", "olivia@example.org")
        .unwrap();
    assert_eq!(res.as_slice(), &[Box::from("msg:6"), Box::from("msg:1")]);
}

/// Shows that disabling `tokenization.detect_special_patterns` yields unwanted
/// results.
#[test]
fn query_special_disabled() {
    let ctx =
        start_empty(|command| command.env("SONIC_TOKENIZATION__DETECT_SPECIAL_PATTERNS", "false"));

    let multiplexer = SonicMultiplexer::new().unwrap();

    let ingest =
        SonicChannelIngestBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();
    let search =
        SonicChannelSearchBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();

    for (id, text) in SPECIAL_PATTERNS_TEST_CASES {
        ingest.push("messages", "default", id, text).unwrap();
    }

    let res = search
        .query("messages", "default", "olivia@example.org")
        .unwrap();
    assert_eq!(
        res.as_slice(),
        &[
            Box::from("msg:6"),
            Box::from("msg:1"),
            Box::from("msg:4"),
            Box::from("msg:3"),
            Box::from("msg:2"),
            Box::from("msg:5")
        ]
    );
}
