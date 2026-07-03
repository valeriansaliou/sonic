// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;

use crate::common::prelude::*;

#[test]
fn info() {
    let ctx = start_empty(|command| command);

    let multiplexer = SonicMultiplexer::new().unwrap();

    let control =
        SonicChannelControlBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();

    let res = control.info().unwrap();
    assert_eq!(res.commands_total, 2);
}
