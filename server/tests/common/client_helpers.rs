// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use sonic_client::control::{self, SonicChannelControlBlocking};
use sonic_client::make_command;

pub fn trigger_flush(channel: &SonicChannelControlBlocking) -> std::io::Result<()> {
    channel.send(
        make_command!("TRIGGER flush"),
        control::Discriminant::Ok,
        |_data| Ok(()),
    )
}

pub fn trigger_compact(
    channel: &SonicChannelControlBlocking,
    collections: &[&str],
) -> std::io::Result<()> {
    let mut args = String::with_capacity(collections.len() * 16);
    for &collection in collections.into_iter() {
        args.push(' ');
        args.push_str(collection);
    }

    channel.send(
        make_command!("TRIGGER compact{}", args),
        control::Discriminant::Ok,
        |_data| Ok(()),
    )
}
