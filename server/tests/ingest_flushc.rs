// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;

use crate::common::prelude::*;

#[test]
fn flushc() {
    let ctx = start_empty(|command| command);

    let multiplexer = SonicMultiplexer::new().unwrap();

    let ingest =
        SonicChannelIngestBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();
    let control =
        SonicChannelControlBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();

    ingest
        .push("collection", "bucket", "object", "foo bar baz")
        .unwrap();

    control.trigger_consolidate().unwrap();

    let res = ingest.countc("collection").unwrap();
    assert_eq!(res, 1);

    let res = ingest.flushc("collection").unwrap();
    assert_eq!(res, 1);

    let res = ingest.countc("collection").unwrap();
    assert_eq!(res, 0);
}
