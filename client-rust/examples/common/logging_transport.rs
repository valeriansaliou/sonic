// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use sonic_client::transport::Transport;

use super::macros::logging::*;

pub struct Logging<T: Transport>(T);

impl<T: Transport> sonic_client::transport::Transport for Logging<T> {
    type Lines<'a>
        = LinesLogger<T::Lines<'a>>
    where
        Self: 'a;

    fn connect(addr: std::net::SocketAddr) -> std::io::Result<Self> {
        T::connect(addr).map(Self)
    }

    fn read_lines<'a>(&'a mut self) -> std::io::Result<Self::Lines<'a>> {
        self.0.read_lines().map(LinesLogger)
    }

    fn read_line_sync(&mut self) -> std::io::Result<String> {
        let res = self.0.read_line_sync();

        if let Ok(ref line) = res {
            log_debug!("< {line}");
        }

        res
    }

    fn write_with<R>(&mut self, write: impl FnOnce(&mut bytes::BytesMut) -> R) -> R {
        use bytes::BufMut as _;

        let mut line = bytes::BytesMut::new();

        let res = write(&mut line);

        log_debug!("> {}", String::from_utf8_lossy(&line));

        self.0.write_with(|buf| buf.put_slice(&line));

        res
    }

    fn write_line(&mut self, line: impl AsRef<[u8]>) {
        log_debug!("> {}", String::from_utf8_lossy(line.as_ref()));

        self.0.write_line(line)
    }

    fn has_buffered_writes(&self) -> bool {
        self.0.has_buffered_writes()
    }

    fn flush_writes(&mut self) -> std::io::Result<usize> {
        self.0.flush_writes()
    }
}

impl<T: Transport> AsMut<mio::net::TcpStream> for Logging<T> {
    fn as_mut(&mut self) -> &mut mio::net::TcpStream {
        self.0.as_mut()
    }
}

#[repr(transparent)]
pub struct LinesLogger<I: Iterator<Item = bytes::Bytes>>(I);

impl<I: Iterator<Item = bytes::Bytes>> Iterator for LinesLogger<I> {
    type Item = bytes::Bytes;

    fn next(&mut self) -> Option<Self::Item> {
        let res = self.0.next();

        if let Some(ref line) = res {
            log_debug!("< {}", String::from_utf8_lossy(line));
        }

        res
    }
}
