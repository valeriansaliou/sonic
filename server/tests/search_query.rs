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
