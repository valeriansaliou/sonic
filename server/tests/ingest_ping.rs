// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;

use crate::common::prelude::*;

#[test]
fn ping() {
    let ctx = start_empty(|command| command);

    let multiplexer = SonicMultiplexer::new().unwrap();

    let ingest =
        SonicChannelIngestBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();

    () = ingest.ping().unwrap();
}
