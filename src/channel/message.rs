// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::io::Write;
use std::net::TcpStream;
use std::str::{self, SplitWhitespace};
use std::time::Instant;

use super::command::{
    ChannelCommandBase, ChannelCommandControl, ChannelCommandError, ChannelCommandIngest,
    ChannelCommandResponse, ChannelCommandSearch, COMMANDS_MODE_CONTROL, COMMANDS_MODE_INGEST,
    COMMANDS_MODE_SEARCH,
};
use crate::LINE_FEED;

pub struct ChannelMessage;
pub struct ChannelMessageModeSearch;
pub struct ChannelMessageModeIngest;
pub struct ChannelMessageModeControl;

const COMMAND_ELAPSED_MILLIS_SLOW_WARN: u128 = 50;

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

        let command_start = Instant::now();

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

        // Measure and log time it took to execute command
        // Notice: this is critical as to raise developer awareness on the performance bits when \
        //   altering commands-related code, or when making changes to underlying store executors.
        let command_took = command_start.elapsed();

        if command_took.as_millis() >= COMMAND_ELAPSED_MILLIS_SLOW_WARN {
            warn!(
                "took a lot of time: {}ms to process channel message",
                command_took.as_millis(),
            );
        } else {
            info!(
                "took {}ms/{}us/{}ns to process channel message",
                command_took.as_millis(),
                command_took.as_micros(),
                command_took.as_nanos(),
            );
        }

        return result;
    }

    fn extract<'a>(message: &'a str) -> (String, SplitWhitespace) {
        // Extract command name and arguments
        let mut parts = message.split_whitespace();
        let command = parts.next().unwrap_or("").to_uppercase();

        debug!("will dispatch search command: {}", command);

        return (command, parts);
    }
}

impl ChannelMessageMode for ChannelMessageModeSearch {
    fn handle(message: &str) -> Result<Vec<ChannelCommandResponse>, ChannelCommandError> {
        gen_channel_message_mode_handle!(message, COMMANDS_MODE_SEARCH, {
            "QUERY" => ChannelCommandSearch::dispatch_query,
            "SUGGEST" => ChannelCommandSearch::dispatch_suggest,
            "HELP" => ChannelCommandSearch::dispatch_help,
        })
    }
}

impl ChannelMessageMode for ChannelMessageModeIngest {
    fn handle(message: &str) -> Result<Vec<ChannelCommandResponse>, ChannelCommandError> {
        gen_channel_message_mode_handle!(message, COMMANDS_MODE_INGEST, {
            "PUSH" => ChannelCommandIngest::dispatch_push,
            "POP" => ChannelCommandIngest::dispatch_pop,
            "COUNT" => ChannelCommandIngest::dispatch_count,
            "FLUSHC" => ChannelCommandIngest::dispatch_flushc,
            "FLUSHB" => ChannelCommandIngest::dispatch_flushb,
            "FLUSHO" => ChannelCommandIngest::dispatch_flusho,
            "HELP" => ChannelCommandIngest::dispatch_help,
        })
    }
}

impl ChannelMessageMode for ChannelMessageModeControl {
    fn handle(message: &str) -> Result<Vec<ChannelCommandResponse>, ChannelCommandError> {
        gen_channel_message_mode_handle!(message, COMMANDS_MODE_CONTROL, {
            "TRIGGER" => ChannelCommandControl::dispatch_trigger,
            "HELP" => ChannelCommandControl::dispatch_help,
        })
    }
}
