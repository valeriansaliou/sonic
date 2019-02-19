// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::io::Write;
use std::net::TcpStream;
use std::str::{self, SplitWhitespace};

use super::command::{
    ChannelCommandBase, ChannelCommandError, ChannelCommandIngest, ChannelCommandResponse,
    ChannelCommandSearch,
};
use crate::LINE_FEED;

pub struct ChannelMessage;
pub struct ChannelMessageModeSearch;
pub struct ChannelMessageModeIngest;

#[derive(PartialEq)]
pub enum ChannelMessageResult {
    Continue,
    Close,
}

pub trait ChannelMessageMode {
    fn handle(message: &str) -> Result<Vec<ChannelCommandResponse>, ChannelCommandError>;
}

impl ChannelMessage {
    pub fn on<M: ChannelMessageMode>(
        mut stream: &TcpStream,
        message_slice: &[u8],
    ) -> ChannelMessageResult {
        let message = str::from_utf8(message_slice).unwrap_or("");

        debug!("got channel message: {}", message);

        let mut result = ChannelMessageResult::Continue;

        // Handle response arguments to issued command
        let response_args_groups = match M::handle(&message) {
            Ok(resp_groups) => resp_groups
                .iter()
                .map(|resp| match resp {
                    ChannelCommandResponse::Ok
                    | ChannelCommandResponse::Pong
                    | ChannelCommandResponse::Pending(_)
                    | ChannelCommandResponse::Result(_)
                    | ChannelCommandResponse::Event(_, _, _)
                    | ChannelCommandResponse::Nil
                    | ChannelCommandResponse::Void
                    | ChannelCommandResponse::Err(_) => resp.to_args(),
                    ChannelCommandResponse::Ended(_) => {
                        result = ChannelMessageResult::Close;
                        resp.to_args()
                    }
                })
                .collect(),
            Err(reason) => vec![ChannelCommandResponse::Err(reason).to_args()],
        };

        // Serve response messages on socket
        for response_args in response_args_groups {
            if response_args.0.is_empty() == false {
                if let Some(ref values) = response_args.1 {
                    let values_string = values.join(" ");

                    write!(stream, "{} {}{}", response_args.0, values_string, LINE_FEED)
                        .expect("write failed");

                    debug!(
                        "wrote response with values: {} ({})",
                        response_args.0, values_string
                    );
                } else {
                    write!(stream, "{}{}", response_args.0, LINE_FEED).expect("write failed");

                    debug!("wrote response with no values: {}", response_args.0);
                }
            }
        }

        return result;
    }

    fn extract<'a>(message: &'a str) -> (&'a str, SplitWhitespace) {
        // Extract command name and arguments
        let mut parts = message.split_whitespace();
        let command = parts.next().unwrap_or("");

        debug!("will dispatch search command: {}", command);

        return (command, parts);
    }
}

impl ChannelMessageMode for ChannelMessageModeSearch {
    fn handle(message: &str) -> Result<Vec<ChannelCommandResponse>, ChannelCommandError> {
        let (command, parts) = ChannelMessage::extract(message);

        match command.to_uppercase().as_str() {
            "" => Ok(vec![ChannelCommandResponse::Void]),
            "QUERY" => ChannelCommandSearch::dispatch_query(parts),
            "PING" => ChannelCommandBase::dispatch_ping(parts),
            "QUIT" => ChannelCommandBase::dispatch_quit(parts),
            _ => Ok(vec![ChannelCommandResponse::Err(
                ChannelCommandError::UnknownCommand,
            )]),
        }
    }
}

impl ChannelMessageMode for ChannelMessageModeIngest {
    fn handle(message: &str) -> Result<Vec<ChannelCommandResponse>, ChannelCommandError> {
        let (command, parts) = ChannelMessage::extract(message);

        match command.to_uppercase().as_str() {
            "" => Ok(vec![ChannelCommandResponse::Void]),
            "PUSH" => ChannelCommandIngest::dispatch_push(parts),
            "POP" => ChannelCommandIngest::dispatch_pop(parts),
            "COUNT" => ChannelCommandIngest::dispatch_count(parts),
            "FLUSHC" => ChannelCommandIngest::dispatch_flushc(parts),
            "FLUSHB" => ChannelCommandIngest::dispatch_flushb(parts),
            "PING" => ChannelCommandBase::dispatch_ping(parts),
            "QUIT" => ChannelCommandBase::dispatch_quit(parts),
            _ => Ok(vec![ChannelCommandResponse::Err(
                ChannelCommandError::UnknownCommand,
            )]),
        }
    }
}
