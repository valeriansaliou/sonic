// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;

use crate::common::prelude::*;

#[test]
fn pop() {
    let ctx = start_empty(|command| command);

    let multiplexer = SonicMultiplexer::new().unwrap();

    let ingest =
        SonicChannelIngestBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();
    let control =
        SonicChannelControlBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();
    let search =
        SonicChannelSearchBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();

    ingest
        .push("collection", "bucket", "object", "foo bar baz")
        .unwrap();

    control.trigger_consolidate().unwrap();

    let terms = search.list("collection", "bucket").unwrap();
    assert_eq!(
        terms.as_slice(),
        &[Box::from("bar"), Box::from("baz"), Box::from("foo")]
    );

    let res = ingest
        .pop("collection", "bucket", "object", "foo bar")
        .unwrap();
    assert_eq!(res, 2);

    control.trigger_consolidate().unwrap();

    let terms = search.list("collection", "bucket").unwrap();
    assert_eq!(terms.as_slice(), &[Box::from("baz")]);
}
