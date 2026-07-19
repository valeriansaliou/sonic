// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use mio::net::TcpStream;

use crate::{TCP_READ_BUFFER_CAPACITY, TCP_WRITE_BUFFER_CAPACITY, logging::*};

pub trait Transport: AsMut<TcpStream> + Sized + Send {
    type Lines<'a>: Iterator<Item = bytes::Bytes>
    where
        Self: 'a;

    fn connect(addr: std::net::SocketAddr) -> std::io::Result<Self>;

    fn read_lines<'a>(&'a mut self) -> std::io::Result<Self::Lines<'a>>;

    fn read_line_sync(&mut self) -> std::io::Result<String>;

    fn write_with<R>(&mut self, write: impl FnOnce(&mut bytes::BytesMut) -> R) -> R;

    fn write_line(&mut self, line: impl AsRef<[u8]>);

    fn has_buffered_writes(&self) -> bool;

    fn flush_writes(&mut self) -> std::io::Result<usize>;
}

pub struct SonicStream {
    /// TCP stream used by all connections attached to this dispatcher.
    ///
    /// For example, since Search operations are read-only, multiple connections
    /// can use the same underlying TCP stream without causing race conditions.
    stream: TcpStream,

    /// Buffer for bytes to send to the server.
    ///
    /// Messages are sent by the main event loop. To avoid unnnecessary
    /// allocations, this buffer holds all data to send during the next cycle
    /// (for this connection).
    write_buf: bytes::BytesMut,

    /// Buffer for bytes received from the server.
    ///
    /// We cannot guarantee all bytes until the newline will arrive at the same
    /// time, therefore we have to keep partial data in memory until the next
    /// event loop cycle.
    read_buf: bytes::BytesMut,
}

impl Transport for SonicStream {
    type Lines<'a> = Lines<'a>;

    fn connect(addr: std::net::SocketAddr) -> std::io::Result<Self> {
        TcpStream::connect(addr).map(|stream| Self {
            stream,
            write_buf: bytes::BytesMut::with_capacity(TCP_WRITE_BUFFER_CAPACITY),
            read_buf: bytes::BytesMut::with_capacity(TCP_READ_BUFFER_CAPACITY),
        })
    }

    fn read_lines<'a>(&'a mut self) -> std::io::Result<Lines<'a>> {
        use std::io::Read as _;

        let len_before = self.read_buf.len();

        'read_stream: loop {
            let mut tmp = [0u8; 4096];

            match self.stream.read(&mut tmp) {
                Ok(0) => {
                    log_trace!("Read empty chunk");
                    break 'read_stream;
                }
                Ok(n) => {
                    log_trace!("Read {n} bytes chunk");

                    // FIXME: We might overflow the buffer here. It’s not
                    //   problematic, but it might causes a reallocation we
                    //   could avoid.
                    self.read_buf.extend_from_slice(&tmp[..n]);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    break 'read_stream;
                }
                Err(e) => return Err(e),
            }
        }

        log_debug!("Read {n} bytes", n = self.read_buf.len() - len_before);

        Ok(Lines::new(&mut self.read_buf))
    }

    fn read_line_sync(&mut self) -> std::io::Result<String> {
        use std::io::{BufRead as _, BufReader};

        let mut buf_reader = BufReader::new(&self.stream);

        let mut response = String::new();

        loop {
            match buf_reader.read_line(&mut response) {
                Ok(_read) => {
                    let new_len = response.trim_ascii_end().len();
                    response.truncate(new_len);
                    return Ok(response);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // log_trace!("Would block.");
                    continue;
                }
                Err(e) => return Err(e),
            };
        }
    }

    fn write_with<R>(&mut self, write: impl FnOnce(&mut bytes::BytesMut) -> R) -> R {
        use bytes::BufMut as _;

        let res = write(&mut self.write_buf);
        self.write_buf.put_slice(b"\r\n");

        res
    }

    fn write_line(&mut self, line: impl AsRef<[u8]>) {
        use bytes::BufMut as _;

        self.write_buf.put_slice(line.as_ref());
        log_trace!("Wrote {} bytes line", line.as_ref().len());
        self.write_buf.put_slice(b"\r\n");
        log_trace!("Wrote \\r\\n");
    }

    #[inline]
    fn has_buffered_writes(&self) -> bool {
        !self.write_buf.is_empty()
    }

    fn flush_writes(&mut self) -> std::io::Result<usize> {
        use bytes::Buf as _;
        use std::io::Write as _;

        if self.write_buf.is_empty() {
            return Ok(0);
        }

        match self.stream.write(&self.write_buf) {
            Ok(written) => {
                self.write_buf.advance(written);
                Ok(written)
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(0),
            Err(err) => Err(err),
        }
    }
}

impl AsMut<TcpStream> for SonicStream {
    #[inline]
    fn as_mut(&mut self) -> &mut TcpStream {
        &mut self.stream
    }
}

pub struct Lines<'a> {
    bytes: &'a mut bytes::BytesMut,
    processed_lines: usize,
}

impl<'a> Lines<'a> {
    fn new(buf: &'a mut bytes::BytesMut) -> Self {
        Self {
            bytes: buf,
            processed_lines: 0,
        }
    }
}

impl<'a> Iterator for Lines<'a> {
    type Item = bytes::Bytes;

    fn next(&mut self) -> Option<Self::Item> {
        use bytes::Buf as _;

        if let Some(pos) = self.bytes.iter().position(|&b| b == b'\n') {
            self.processed_lines += 1;

            // Split on `\n` (`self.bytes` will start at `\n`).
            let mut line_bytes = self.bytes.split_to(pos);

            // Remove `\n` from `self.bytes`.
            self.bytes.advance(1);

            // Remove `\r` if present.
            if line_bytes.ends_with(b"\r") {
                line_bytes.truncate(line_bytes.len() - 1);
            };

            Some(bytes::Bytes::from(line_bytes))
        } else {
            None
        }
    }
}

impl<'a> Drop for Lines<'a> {
    fn drop(&mut self) {
        if self.processed_lines > 1 {
            log_debug!("Processed {} lines at once", self.processed_lines);
        }
    }
}
