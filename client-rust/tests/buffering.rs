// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;

use sonic_client::SonicMultiplexer;
use sonic_client::ingest::SonicChannelIngestBlocking;

use crate::common::start_sonic;

/// WARN: DON’T HARDCODE A PASSWORD IN PRODUCTION CODE! This is just an example!
const PASS: &str = "SecretPassword";
const COLLECTION: &str = "collection";
const BUCKET: &str = "bucket";

/// Checks that buffering is supported by the library.
///
/// This test intentionally adds quotes (`"`) in the string to ensure the
/// buffering algorithm is aware of escaping.
#[test]
fn test_buffering() {
    let (_guard, addr) = start_sonic();

    let multiplexer = SonicMultiplexer::new().unwrap();

    let sonic = SonicChannelIngestBlocking::connect(addr, PASS, &multiplexer).unwrap();

    let buffer_size = sonic.channel_info().buffer_size;

    let str = "foo \"bar\" ";
    let text = str.repeat((buffer_size / str.len()) + 4);

    sonic.push(COLLECTION, BUCKET, "object", text).unwrap();
}
