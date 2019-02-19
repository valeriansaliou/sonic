// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::io::{Read, Write};
use std::net::TcpStream;
use std::result::Result;
use std::str;
use std::time::Duration;

use super::command::ChannelCommand;
use super::command::ChannelCommandResponse;
use super::command::COMMAND_SIZE;
use crate::APP_CONF;
use crate::LINE_FEED;

pub struct ChannelHandle;

enum ChannelHandleError {
    Closed,
    NotRecognized,
    TimedOut,
    ConnectionAborted,
    Interrupted,
    Unknown,
}

#[derive(PartialEq)]
enum ChannelHandleMessageResult {
    Continue,
    Close,
}

const LINE_END_GAP: usize = 1;
const MAX_LINE_SIZE: usize = COMMAND_SIZE + LINE_END_GAP + 1;
const HASH_VALUE_SIZE: usize = 10;
const HASH_RESULT_SIZE: usize = 7 + LINE_END_GAP + 1;
const SHARD_DEFAULT: ChannelShard = 0;
const TCP_TIMEOUT_NON_ESTABLISHED: u64 = 20;

static BUFFER_LINE_SEPARATOR: u8 = '\n' as u8;

pub type ChannelShard = u8;

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
        // TODO: no need for such a split
        ChannelHandle::configure_stream(&stream, false);

        // Send connected banner
        write!(stream, "{}{}", *CONNECTED_BANNER, LINE_FEED).expect("write failed");

        // Configure stream (established)
        // TODO: no need for such a split
        ChannelHandle::configure_stream(&stream, true);

        // Send started acknowledgement
        write!(stream, "STARTED{}", LINE_FEED).expect("write failed");

        // Select default shard
        let mut shard = SHARD_DEFAULT;

        // Initialize packet buffer
        let mut buffer = Vec::new();

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
                                    if Self::on_message(&mut shard, &stream, line)
                                        == ChannelHandleMessageResult::Close
                                    {
                                        // Should close?
                                        break 'handler;
                                    }
                                }
                            }
                        }

                        // Reset buffer
                        buffer.clear();
                    }
                }
                Err(err) => {
                    info!("closing channel thread with traceback: {}", err);

                    panic!("closing channel channel");
                }
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

    fn on_message(
        shard: &mut ChannelShard,
        mut stream: &TcpStream,
        message_slice: &[u8],
    ) -> ChannelHandleMessageResult {
        let message = str::from_utf8(message_slice).unwrap_or("");

        debug!("got channel message on shard {}: {}", shard, message);

        let mut result = ChannelHandleMessageResult::Continue;

        let response = match Self::handle_message(shard, &message) {
            Ok(resp) => match resp {
                ChannelCommandResponse::Ok
                | ChannelCommandResponse::Pong
                | ChannelCommandResponse::Ended
                | ChannelCommandResponse::Nil
                | ChannelCommandResponse::Void => {
                    if resp == ChannelCommandResponse::Ended {
                        result = ChannelHandleMessageResult::Close;
                    }
                    resp.to_str()
                }
                _ => ChannelCommandResponse::Err.to_str(),
            },
            _ => ChannelCommandResponse::Err.to_str(),
        };

        if response.is_empty() == false {
            write!(stream, "{}{}", response, LINE_FEED).expect("write failed");

            debug!("wrote response: {}", response);
        }

        return result;
    }

    fn handle_message(
        shard: &mut ChannelShard,
        message: &str,
    ) -> Result<ChannelCommandResponse, Option<()>> {
        let mut parts = message.split_whitespace();
        let command = parts.next().unwrap_or("");

        debug!("will dispatch command: {}", command);

        match command {
            "" => Ok(ChannelCommandResponse::Void),
            "FLUSHB" => ChannelCommand::dispatch_flush_bucket(shard, parts),
            "FLUSHA" => ChannelCommand::dispatch_flush_auth(shard, parts),
            "PING" => ChannelCommand::dispatch_ping(),
            "SHARD" => ChannelCommand::dispatch_shard(shard, parts),
            "QUIT" => ChannelCommand::dispatch_quit(),
            _ => Ok(ChannelCommandResponse::Nil),
        }
    }
}
