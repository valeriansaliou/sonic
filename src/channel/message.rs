// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::io::Write;
use std::str::{self, SplitWhitespace};
use std::time::Instant;

use super::command::{
    ChannelCommandBase, ChannelCommandControl, ChannelCommandError, ChannelCommandIngest,
    ChannelCommandResponse, ChannelCommandResponseArgs, ChannelCommandSearch,
    COMMANDS_MODE_CONTROL, COMMANDS_MODE_INGEST, COMMANDS_MODE_SEARCH,
};
use super::listen::CHANNEL_AVAILABLE;
use super::statistics::{COMMANDS_TOTAL, COMMAND_LATENCY_BEST, COMMAND_LATENCY_WORST};
use crate::LINE_FEED;

pub struct ChannelMessage<'a, TS> {
    stream: &'a mut TS,
    message: String,
    command_start: Instant,
    result: ChannelMessageResult,
    response_args: Option<ChannelCommandResponseArgs>,
    channel_available: bool,
}

pub struct ChannelMessageUtils;
pub struct ChannelMessageModeSearch;
pub struct ChannelMessageModeIngest;
pub struct ChannelMessageModeControl;

const COMMAND_ELAPSED_MILLIS_SLOW_WARN: u128 = 50;

#[derive(PartialEq, Clone)]
pub enum ChannelMessageResult {
    Continue,
    Close,
}

pub trait ChannelMessageMode {
    fn handle(message: &str) -> Result<ChannelCommandResponse, ChannelCommandError>;
}

impl<'a, TS> ChannelMessage<'a, TS>
where
    TS: Write,
{
    pub fn new(stream: &'a mut TS, message_slice: &[u8]) -> Self {
        let message = String::from_utf8(message_slice.to_vec()).unwrap_or(String::from(""));
        Self {
            stream,
            message,
            command_start: Instant::now(),
            result: ChannelMessageResult::Continue,
            response_args: None,
            channel_available: *CHANNEL_AVAILABLE.read().unwrap(),
        }
    }

    pub fn handle<M: ChannelMessageMode>(&mut self) -> ChannelMessageResult {
        self.print_command_received_msg();

        if self.channel_availability() == false {
            self.set_response_args(
                ChannelCommandResponse::Err(ChannelCommandError::ShuttingDown).to_args(),
            );
            self.send_reponse_messages();
            return self.result.clone();
        }

        self.handle_message::<M>();
        self.send_reponse_messages();
        self.print_elapsed_time();
        self.update_statistics();

        self.result.clone()
    }

    fn print_command_received_msg(&self) {
        debug!("received channel message: {}", self.message);
    }

    fn channel_availability(&self) -> bool {
        self.channel_available
    }

    // Handle response arguments to issued command
    fn handle_message<M: ChannelMessageMode>(&mut self) {
        match M::handle(&self.message) {
            Ok(resp) => match resp {
                ChannelCommandResponse::Ok
                | ChannelCommandResponse::Pong
                | ChannelCommandResponse::Pending(_)
                | ChannelCommandResponse::Result(_)
                | ChannelCommandResponse::Event(_, _, _)
                | ChannelCommandResponse::Void
                | ChannelCommandResponse::Err(_) => {
                    self.set_response_args(resp.to_args());
                }
                ChannelCommandResponse::Ended(_) => {
                    self.result = ChannelMessageResult::Close;
                    self.set_response_args(resp.to_args());
                }
            },
            Err(reason) => {
                self.set_response_args(ChannelCommandResponse::Err(reason).to_args());
            }
        };
    }

    fn set_response_args(&mut self, args: ChannelCommandResponseArgs) {
        self.response_args = Some(args);
    }

    // Serve response messages on socket
    fn send_reponse_messages(&mut self) {
        match &self.response_args {
            Some(response_args) => {
                if !response_args.0.is_empty() {
                    if let Some(ref values) = response_args.1 {
                        let values_string = values.join(" ");

                        write!(
                            self.stream,
                            "{} {}{}",
                            response_args.0, values_string, LINE_FEED
                        )
                        .expect("write failed");

                        debug!(
                            "wrote response with values: {} ({})",
                            response_args.0, values_string
                        );
                    } else {
                        write!(self.stream, "{}{}", response_args.0, LINE_FEED)
                            .expect("write failed");

                        debug!("wrote response with no values: {}", response_args.0);
                    }
                }
            }
            None => {
                debug!("try to send empty message");
            }
        }
    }

    // Measure and log time it took to execute command
    // Notice: this is critical as to raise developer awareness on the performance bits when \
    // altering commands-related code, or when making changes to underlying store executors.
    fn print_elapsed_time(&self) {
        let command_took = self.command_start.elapsed();

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
    }

    // Update performance measures
    // Notice: commands that take 0ms are not accounted for there (ie. those are usually \
    //   commands that do no work or I/O; they would make statistics less accurate)
    fn update_statistics(&self) {
        let command_took_millis = self.command_start.elapsed().as_millis() as u32;

        if command_took_millis > *COMMAND_LATENCY_WORST.read().unwrap() {
            *COMMAND_LATENCY_WORST.write().unwrap() = command_took_millis;
        }
        if command_took_millis > 0
            && (*COMMAND_LATENCY_BEST.read().unwrap() == 0
                || command_took_millis < *COMMAND_LATENCY_BEST.read().unwrap())
        {
            *COMMAND_LATENCY_BEST.write().unwrap() = command_took_millis;
        }

        // Increment total commands
        *COMMANDS_TOTAL.write().unwrap() += 1;
    }
}

impl ChannelMessageUtils {
    pub fn extract(message: &str) -> (String, SplitWhitespace) {
        // Extract command name and arguments
        let mut parts = message.split_whitespace();
        let command = parts.next().unwrap_or("").to_uppercase();

        debug!("will dispatch search command: {}", command);

        (command, parts)
    }
}

impl ChannelMessageMode for ChannelMessageModeSearch {
    fn handle(message: &str) -> Result<ChannelCommandResponse, ChannelCommandError> {
        gen_channel_message_mode_handle!(message, COMMANDS_MODE_SEARCH, {
            "QUERY" => ChannelCommandSearch::dispatch_query,
            "SUGGEST" => ChannelCommandSearch::dispatch_suggest,
            "HELP" => ChannelCommandSearch::dispatch_help,
        })
    }
}

impl ChannelMessageMode for ChannelMessageModeIngest {
    fn handle(message: &str) -> Result<ChannelCommandResponse, ChannelCommandError> {
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
    fn handle(message: &str) -> Result<ChannelCommandResponse, ChannelCommandError> {
        gen_channel_message_mode_handle!(message, COMMANDS_MODE_CONTROL, {
            "TRIGGER" => ChannelCommandControl::dispatch_trigger,
            "INFO" => ChannelCommandControl::dispatch_info,
            "HELP" => ChannelCommandControl::dispatch_help,
        })
    }
}

// tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_message_context_can_be_initialized() {
        let mut fake_tcp: Vec<u8> = vec![];
        assert_eq!(
            ChannelMessage::new(&mut fake_tcp, &(b"a").clone()).message,
            String::from("a")
        );
    }
}
