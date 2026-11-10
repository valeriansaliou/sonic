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

    let make_stream = || {
        let buffer_size = {
            let multiplexer = SonicMultiplexer::new().unwrap();

            let sonic =
                SonicChannelIngestBlocking::connect(ctx.addr, "SecretPassword", &multiplexer)
                    .unwrap();

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

        (stream, buffer_size)
    };

    let make_command = |len| {
        let mut buf = String::with_capacity(len);
        buf.push_str("PUSH collection bucket object \"");
        buf.extend(std::iter::repeat_n(
            'a',
            len.saturating_sub(buf.len()).saturating_sub(2),
        ));
        buf.push_str("\"\n");

        assert_eq!(buf.len(), len);
        buf
    };

    // Sonic succeeds if command exactly as long as the buffer.
    {
        let (mut stream, buffer_size) = make_stream();

        let command = make_command(buffer_size);

        stream.write_all(command.as_bytes()).unwrap();

        let res_opt = BufReader::new(&stream).lines().next();
        let line = res_opt
            .expect("Sonic should write")
            .expect("Sonic should not error");
        assert_eq!(line, "OK");
    }

    // Sonic succeeds if command exactly as long as the buffer, even when
    // written in multiple chunks.
    {
        let (mut stream, buffer_size) = make_stream();

        let command = make_command(buffer_size);
        let bytes = command.as_bytes();

        stream.write_all(&bytes[..(buffer_size - 1)]).unwrap();
        stream.write_all(&bytes[(buffer_size - 1)..]).unwrap();

        let res_opt = BufReader::new(&stream).lines().next();
        let line = res_opt
            .expect("Sonic should write")
            .expect("Sonic should not error");
        assert_eq!(line, "OK");
    }

    // Sonic aborts if command longer than the buffer.
    {
        let (mut stream, buffer_size) = make_stream();

        let command = make_command(buffer_size + 1);

        stream.write_all(command.as_bytes()).unwrap();

        let res_opt = BufReader::new(&stream).lines().next();
        match res_opt {
            Some(res) => assert_eq!(
                res.as_ref().map_err(std::io::Error::kind),
                Err(std::io::ErrorKind::ConnectionReset),
                "{res:?}"
            ),
            None => { /* Also possible */ }
        }
    }

    // Sonic aborts if command longer than the buffer, even when written in
    // multiple chunks.
    {
        let (mut stream, buffer_size) = make_stream();

        let command = make_command(buffer_size + 1);
        let bytes = command.as_bytes();

        stream.write_all(&bytes[..(buffer_size - 1)]).unwrap();
        stream.write_all(&bytes[(buffer_size - 1)..]).unwrap();

        let res_opt = BufReader::new(&stream).lines().next();
        match res_opt {
            Some(res) => assert_eq!(
                res.as_ref().map_err(std::io::Error::kind),
                Err(std::io::ErrorKind::ConnectionReset),
                "{res:?}"
            ),
            None => { /* Also possible */ }
        }
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
