// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;

use crate::common::prelude::*;

#[test]
fn push() {
    let ctx = start_empty(|command| command);

    let multiplexer = SonicMultiplexer::new().unwrap();

    let ingest =
        SonicChannelIngestBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();

    () = ingest
        .push("collection", "bucket", "object", "foo bar")
        .unwrap();
}

/// This test ensures the server closes the connection if a line overflows the
/// buffer. We don’t use a client on purpose, as they would support this use
/// case and testing it would result in testing the client, not the server.
#[test]
fn push_overflow() {
    use std::io::{BufRead as _, BufReader, Write};
    use std::net::TcpStream;

    let ctx = start_empty(|command| command);

    let buffer_size = {
        let multiplexer = SonicMultiplexer::new().unwrap();

        let sonic =
            SonicChannelIngestBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();

        sonic.channel_info().buffer_size
    };

    let mut stream = TcpStream::connect(ctx.addr).unwrap();

    {
        let line = BufReader::new(&stream).lines().next().unwrap().unwrap();
        assert!(line.starts_with("CONNECTED "), "{line}");
    }

    stream.write_all(b"START ingest SecretPassword").unwrap();

    {
        let line = BufReader::new(&stream).lines().next().unwrap().unwrap();
        assert!(line.starts_with("STARTED ingest "), "{line}");
    }

    let buf_len = buffer_size + 1;
    let mut buf = String::with_capacity(buf_len);
    buf.push_str("PUSH collection bucket object ");
    buf.extend(std::iter::repeat_n('a', buf_len.saturating_sub(buf.len())));
    assert_eq!(buf.len(), buf_len);
    stream.write_all(buf.as_bytes()).unwrap();

    {
        let line_opt = BufReader::new(&stream).lines().next();
        assert!(line_opt.is_none(), "{:?}", line_opt.unwrap());
    }
}

#[test]
fn push_bad_chars() {
    let ctx = start_empty(|command| command);

    let multiplexer = SonicMultiplexer::new().unwrap();

    let ingest =
        SonicChannelIngestBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();
    let control =
        SonicChannelControlBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();
    let search =
        SonicChannelSearchBlocking::connect(ctx.addr, "SecretPassword", &multiplexer).unwrap();

    () = ingest
        .push("collection", "bucket", "object", "\"foo\" \t \n \r\n bar")
        .unwrap();

    control.trigger_consolidate().unwrap();

    let res = search.list("collection", "bucket").unwrap();
    assert_eq!(res.as_slice(), &[Box::from("bar"), Box::from("foo")]);
}
