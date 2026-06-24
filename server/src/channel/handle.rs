// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;
use std::result::Result;
use std::str;
use std::sync::Arc;
use std::time::Duration;

use sonic::Executor;

use super::message::{
    ChannelMessageModeControl, ChannelMessageModeIngest, ChannelMessageModeSearch,
    ChannelMessageResult,
};
use super::mode::ChannelMode;
use super::statistics::CLIENTS_CONNECTED;
use crate::LINE_FEED;

pub struct ChannelHandle {
    pub app_conf: Arc<crate::Config>,
    pub executor: Executor,
}

enum ChannelHandleError {
    Closed,
    InvalidMode,
    AuthenticationRequired,
    AuthenticationFailed,
    NotRecognized,
    TimedOut,
    ConnectionAborted,
    Interrupted,
    Unknown,
}

const LINE_END_GAP: usize = 1;
const BUFFER_SIZE: usize = 20000;
const MAX_LINE_SIZE: usize = BUFFER_SIZE + LINE_END_GAP + 1;
const TCP_TIMEOUT_NON_ESTABLISHED: u64 = 10;
const PROTOCOL_REVISION: u8 = 1;
const BUFFER_LINE_SEPARATOR: u8 = b'\n';

const CONNECTED_BANNER: &str = concat!(
    "CONNECTED <",
    env!("CARGO_PKG_NAME"),
    " v",
    env!("CARGO_PKG_VERSION"),
    ">"
);

impl ChannelHandleError {
    pub fn to_str(&self) -> &'static str {
        match *self {
            ChannelHandleError::Closed => "closed",
            ChannelHandleError::InvalidMode => "invalid_mode",
            ChannelHandleError::AuthenticationRequired => "authentication_required",
            ChannelHandleError::AuthenticationFailed => "authentication_failed",
            ChannelHandleError::NotRecognized => "not_recognized",
            ChannelHandleError::TimedOut => "timed_out",
            ChannelHandleError::ConnectionAborted => "connection_aborted",
            ChannelHandleError::Interrupted => "interrupted",
            ChannelHandleError::Unknown => "unknown",
        }
    }
}

impl ChannelHandle {
    pub fn client(&self, mut stream: TcpStream) {
        // Configure stream (non-established)
        self.configure_stream(&stream, false);

        // Send connected banner
        write!(stream, "{}{}", CONNECTED_BANNER, LINE_FEED).expect("write failed");

        // Increment connected clients count
        *CLIENTS_CONNECTED.write().unwrap() += 1;

        // Ensure channel mode is set
        match self.ensure_start(&stream) {
            Ok(mode) => {
                // Configure stream (established)
                self.configure_stream(&stream, true);

                // Send started acknowledgement (with environment variables)
                write!(
                    stream,
                    "STARTED {} protocol({}) buffer({}){}",
                    mode.to_str(),
                    PROTOCOL_REVISION,
                    BUFFER_SIZE,
                    LINE_FEED
                )
                .expect("write failed");

                self.handle_stream(mode, stream);
            }
            Err(err) => {
                write!(stream, "ENDED {}{}", err.to_str(), LINE_FEED).expect("write failed");
            }
        }

        // Decrement connected clients count
        *CLIENTS_CONNECTED.write().unwrap() -= 1;
    }

    fn configure_stream(&self, stream: &TcpStream, is_established: bool) {
        let tcp_timeout = if is_established {
            self.app_conf.channel.tcp_timeout
        } else {
            TCP_TIMEOUT_NON_ESTABLISHED
        };

        assert!(stream.set_nodelay(true).is_ok());

        assert!(
            stream
                .set_read_timeout(Some(Duration::new(tcp_timeout, 0)))
                .is_ok()
        );
        assert!(
            stream
                .set_write_timeout(Some(Duration::new(tcp_timeout, 0)))
                .is_ok()
        );
    }

    fn handle_stream(&self, mode: ChannelMode, mut stream: TcpStream) {
        // Initialize packet buffer
        let mut buffer: VecDeque<u8> = VecDeque::with_capacity(MAX_LINE_SIZE);

        // Wait for incoming messages
        'handler: loop {
            let mut read = [0; MAX_LINE_SIZE];

            match stream.read(&mut read) {
                Ok(n) => {
                    // Should close?
                    if n == 0 {
                        break;
                    }

                    let (mut chunk, mut read) =
                        read[..n].split_at(n.min(MAX_LINE_SIZE - buffer.len()));

                    // Add chunk to buffer
                    buffer.extend(chunk);
                    assert!(buffer.len() <= MAX_LINE_SIZE);

                    // Handle full lines from buffer (keep the last incomplete line in buffer)
                    {
                        let mut processed_line = Vec::with_capacity(MAX_LINE_SIZE);

                        while let Some(byte) = buffer.pop_front() {
                            // Commit line and start a new one?
                            if byte == BUFFER_LINE_SEPARATOR {
                                if self.on_message(&mode, &stream, &processed_line)
                                    == ChannelMessageResult::Close
                                {
                                    // Should close?
                                    break 'handler;
                                }

                                // Important: clear the contents of the line, as it has just been \
                                //   processed.
                                processed_line.clear();

                                (chunk, read) =
                                    read.split_at(read.len().min(MAX_LINE_SIZE - buffer.len()));

                                // Add chunk to buffer
                                buffer.extend(chunk);
                                assert!(buffer.len() <= MAX_LINE_SIZE);
                            } else {
                                // Append current byte to processed line
                                processed_line.push(byte);
                            }
                        }

                        // Incomplete line remaining? Put it back in buffer.
                        if !processed_line.is_empty() {
                            buffer.extend(processed_line);
                            assert!(buffer.len() <= MAX_LINE_SIZE);
                        }
                    }

                    // Check for buffer overflow.
                    // NOTE: To avoid a needless read of `MAX_LINE_SIZE` bytes,
                    //   we also ensure there is enough space for the line
                    //   separator. If there isn’t, next loop cycle will abort
                    //   because the line is too long anyway.
                    let separator_len = char::from(BUFFER_LINE_SEPARATOR).len_utf8();

                    if (buffer.len() + read.len()) < (MAX_LINE_SIZE - separator_len) {
                        buffer.extend(read);
                    } else {
                        // Do not continue, as there is too much pending data
                        // in the buffer. Most likely the client does not
                        // implement a proper back-pressure management system,
                        // thus we terminate it.
                        tracing::error!("closing channel thread because of buffer overflow");

                        panic!(
                            "buffer overflow ({}/{} bytes)",
                            buffer.len() + read.len(),
                            MAX_LINE_SIZE
                        );
                    }
                }
                Err(err) => {
                    tracing::error!("closing channel thread with traceback: {}", err);

                    panic!("closing channel");
                }
            }
        }
    }

    fn ensure_start(&self, mut stream: &TcpStream) -> Result<ChannelMode, ChannelHandleError> {
        #[allow(clippy::never_loop)]
        loop {
            let mut read = [0; MAX_LINE_SIZE];

            match stream.read(&mut read) {
                Ok(n) => {
                    if n == 0 {
                        return Err(ChannelHandleError::Closed);
                    }

                    let mut parts = str::from_utf8(&read[0..n]).unwrap_or("").split_whitespace();

                    if parts.next().unwrap_or("").to_uppercase().as_str() == "START" {
                        if let Some(res_mode) = parts.next() {
                            tracing::debug!("got mode response: {}", res_mode);

                            // Extract mode
                            if let Ok(mode) = ChannelMode::from_str(res_mode) {
                                // Check if authenticated?
                                if let Some(ref auth_password) = self.app_conf.channel.auth_password
                                {
                                    if let Some(provided_auth) = parts.next() {
                                        // Compare provided password with configured password
                                        if provided_auth != auth_password {
                                            tracing::info!("password provided, but does not match");

                                            return Err(ChannelHandleError::AuthenticationFailed);
                                        }
                                    } else {
                                        tracing::info!("no password provided, but one required");

                                        // No password was provided, but we require one
                                        return Err(ChannelHandleError::AuthenticationRequired);
                                    }
                                }

                                return Ok(mode);
                            }
                        }

                        return Err(ChannelHandleError::InvalidMode);
                    }

                    return Err(ChannelHandleError::NotRecognized);
                }
                Err(err) => {
                    let err_reason = match err.kind() {
                        ErrorKind::TimedOut => ChannelHandleError::TimedOut,
                        ErrorKind::ConnectionAborted => ChannelHandleError::ConnectionAborted,
                        ErrorKind::Interrupted => ChannelHandleError::Interrupted,
                        _ => ChannelHandleError::Unknown,
                    };

                    return Err(err_reason);
                }
            }
        }
    }

    fn on_message(
        &self,
        mode: &ChannelMode,
        stream: &TcpStream,
        message_slice: &[u8],
    ) -> ChannelMessageResult {
        use crate::channel::message::ChannelMessageMode as _;

        match mode {
            ChannelMode::Search => ChannelMessageModeSearch {
                executor: &self.executor,
                search_config: &self.app_conf.sonic.search,
                normalization_config: &self.app_conf.sonic.normalization,
            }
            .on(stream, message_slice),
            ChannelMode::Ingest => ChannelMessageModeIngest {
                executor: &self.executor,
                normalization_config: &self.app_conf.sonic.normalization,
            }
            .on(stream, message_slice),
            ChannelMode::Control => ChannelMessageModeControl {
                executor: &self.executor,
            }
            .on(stream, message_slice),
        }
    }
}
