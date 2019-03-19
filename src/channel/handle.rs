// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;
use std::result::Result;
use std::str;
use std::time::Duration;

use super::message::{
    ChannelMessage, ChannelMessageModeControl, ChannelMessageModeIngest, ChannelMessageModeSearch,
    ChannelMessageResult,
};
use super::mode::ChannelMode;
use crate::APP_CONF;
use crate::LINE_FEED;

pub struct ChannelHandle;

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

static PROTOCOL_REVISION: u8 = 1;
static BUFFER_LINE_SEPARATOR: u8 = '\n' as u8;

lazy_static! {
    static ref CONNECTED_BANNER: String = format!(
        "CONNECTED <{} v{}>",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );
}

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
    pub fn client(mut stream: TcpStream) {
        // Configure stream (non-established)
        ChannelHandle::configure_stream(&stream, false);

        // Send connected banner
        write!(stream, "{}{}", *CONNECTED_BANNER, LINE_FEED).expect("write failed");

        // Ensure channel mode is set
        match Self::ensure_start(&stream) {
            Ok(mode) => {
                // Configure stream (established)
                ChannelHandle::configure_stream(&stream, true);

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

                // Initialize packet buffer
                let mut buffer = Vec::with_capacity(MAX_LINE_SIZE);

                // Wait for incoming messages
                'handler: loop {
                    let mut read = [0; MAX_LINE_SIZE];

                    match stream.read(&mut read) {
                        Ok(n) => {
                            // Should close?
                            if n == 0 {
                                break;
                            }

                            // Buffer chunk
                            buffer.extend_from_slice(&read[0..n]);

                            // Should handle this chunk? (terminated)
                            if buffer[buffer.len() - 1] == BUFFER_LINE_SEPARATOR {
                                {
                                    // Handle all buffered chunks as lines
                                    let buffer_split =
                                        buffer.split(|value| value == &BUFFER_LINE_SEPARATOR);

                                    for line in buffer_split {
                                        if line.is_empty() == false {
                                            if Self::on_message(&mode, &stream, line)
                                                == ChannelMessageResult::Close
                                            {
                                                // Should close?
                                                break 'handler;
                                            }
                                        }
                                    }
                                }

                                // Reset buffer
                                buffer.clear();
                            } else {
                                // This buffer does not end with a line separator; it likely \
                                //   contains data that is way too long, and thus it should be \
                                //   aborted to avoid stacking up too much data in a row.
                                info!("closing channel thread because of buffer overflow");

                                panic!("buffer overflow");
                            }
                        }
                        Err(err) => {
                            info!("closing channel thread with traceback: {}", err);

                            panic!("closing channel");
                        }
                    }
                }
            }
            Err(err) => {
                write!(stream, "ENDED {}{}", err.to_str(), LINE_FEED).expect("write failed");
            }
        }
    }

    fn configure_stream(stream: &TcpStream, is_established: bool) {
        let tcp_timeout = if is_established == true {
            APP_CONF.channel.tcp_timeout
        } else {
            TCP_TIMEOUT_NON_ESTABLISHED
        };

        assert!(stream.set_nodelay(true).is_ok());

        assert!(stream
            .set_read_timeout(Some(Duration::new(tcp_timeout, 0)))
            .is_ok());
        assert!(stream
            .set_write_timeout(Some(Duration::new(tcp_timeout, 0)))
            .is_ok());
    }

    fn ensure_start(mut stream: &TcpStream) -> Result<ChannelMode, ChannelHandleError> {
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
                            debug!("got mode response: {}", res_mode);

                            // Extract mode
                            if let Ok(mode) = ChannelMode::from_str(res_mode) {
                                // Check if authenticated?
                                if let Some(ref auth_password) = APP_CONF.channel.auth_password {
                                    if let Some(provided_auth) = parts.next() {
                                        // Compare provided password with configured password
                                        if provided_auth != auth_password {
                                            info!("password provided, but does not match");

                                            return Err(ChannelHandleError::AuthenticationFailed);
                                        }
                                    } else {
                                        info!("no password provided, but one required");

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
        mode: &ChannelMode,
        stream: &TcpStream,
        message_slice: &[u8],
    ) -> ChannelMessageResult {
        match mode {
            ChannelMode::Search => {
                ChannelMessage::on::<ChannelMessageModeSearch>(stream, message_slice)
            }
            ChannelMode::Ingest => {
                ChannelMessage::on::<ChannelMessageModeIngest>(stream, message_slice)
            }
            ChannelMode::Control => {
                ChannelMessage::on::<ChannelMessageModeControl>(stream, message_slice)
            }
        }
    }
}
