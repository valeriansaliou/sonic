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
