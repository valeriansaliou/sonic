// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;

use crate::common::prelude::*;

#[test]
fn trigger_consolidate() {
    let ctx = start_empty(|command| {
        command
            .env("SONIC_STORE__FST__GRAPH__CONSOLIDATE_AFTER", "3600")
            .env("SONIC_STORE__FST__POOL__INACTIVE_AFTER", "3700")
            .env("SONIC_STORE__KV__DATABASE__FLUSH_AFTER", "3600")
            .env("SONIC_STORE__KV__POOL__INACTIVE_AFTER", "3700")
    });

    let multiplexer = SonicMultiplexer::new().unwrap();

    let ingest =
        SonicChannelIngestBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();
    let control =
        SonicChannelControlBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();
    let search =
        SonicChannelSearchBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();

    ingest
        .push("collection", "bucket", "object", "foo bar")
        .unwrap();

    let terms = search.list("collection", "bucket").unwrap();
    assert_eq!(terms.len(), 0);

    () = control.trigger_consolidate().unwrap();

    let terms = search.list("collection", "bucket").unwrap();
    assert_eq!(terms.len(), 2);
}

#[test]
#[ignore = "Not supported by sonic_client yet (FIXME)"]
fn trigger_backup() {
    todo!()
}

#[test]
#[ignore = "Not supported by sonic_client yet (FIXME)"]
fn trigger_restore() {
    todo!()
}
