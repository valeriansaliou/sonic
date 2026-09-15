// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;

use crate::common::prelude::*;

/// Ensures read timeouts release client slots.
///
/// See <https://github.com/valeriansaliou/sonic/pull/403>
/// and <https://github.com/valeriansaliou/sonic/issues/256>.
///
/// Copyright: 2026, DualFroz <me@dualfroz.com>
#[test]
fn pull_403() {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    const TCP_TIMEOUT: u64 = 1;

    let ctx =
        start_empty(|command| command.env("SONIC_CHANNEL__TCP_TIMEOUT", TCP_TIMEOUT.to_string()));

    {
        let mut stream = TcpStream::connect(ctx.addr).unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();

        reader.read_line(&mut line).unwrap();

        stream
            .write_all(format!("START search {SONIC_PASSWORD}\n").as_bytes())
            .unwrap();

        line.clear();
        reader.read_line(&mut line).unwrap();
        assert!(line.starts_with("STARTED"), "unexpected greeting: {line:?}");

        // Stay idle until the server hits its read timeout, then hang up.
        std::thread::sleep(Duration::from_secs(TCP_TIMEOUT + 1));
    }

    std::thread::sleep(Duration::from_millis(200));

    let multiplexer = SonicMultiplexer::new().unwrap();
    let control =
        SonicChannelControlBlocking::connect(ctx.addr, SONIC_PASSWORD, &multiplexer).unwrap();

    let stats = control.info().unwrap();

    assert_eq!(stats.clients_connected, 1);
}
