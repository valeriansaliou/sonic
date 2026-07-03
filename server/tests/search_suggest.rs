// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;

use crate::common::prelude::*;

#[test]
fn suggest() {
    let ctx = start_prepopulated(|command| command);

    let multiplexer = SonicMultiplexer::new().unwrap();

    let search =
        SonicChannelSearchBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();

    let res = search.suggest("articles", "default", "europ").unwrap();
    assert!(!res.is_empty());
}
