// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use std::str::SplitWhitespace;
use std::vec::Vec;

#[derive(PartialEq)]
pub enum ChannelCommandError {
    UnknownCommand,
    InvalidFormat(&'static str),
}

#[derive(PartialEq)]
pub enum ChannelCommandResponse {
    Void,
    Nil,
    Ok,
    Pong,
    Pending(String),
    Result(String),
    Ended(&'static str),
    Err(ChannelCommandError),
}

pub struct ChannelCommandBase;
pub struct ChannelCommandSearch;
pub struct ChannelCommandIngest;

pub const SEARCH_QUERY_ID_SIZE: usize = 8;

type ChannelResult = Result<ChannelCommandResponse, ChannelCommandError>;

impl ChannelCommandError {
    pub fn to_string(&self) -> String {
        match *self {
            ChannelCommandError::UnknownCommand => String::from("unknown_command"),
            ChannelCommandError::InvalidFormat(format) => format!("invalid_format({})", format),
        }
    }
}

impl ChannelCommandResponse {
    pub fn to_args(&self) -> (&'static str, Option<Vec<String>>) {
        // Convert internal response to channel response arguments; this either gives 'RESPONSE' \
        //   or 'RESPONSE <value:1> <value:2> <..>' whether there are values or not.
        match *self {
            ChannelCommandResponse::Void => ("", None),
            ChannelCommandResponse::Nil => ("NIL", None),
            ChannelCommandResponse::Ok => ("OK", None),
            ChannelCommandResponse::Pong => ("PONG", None),
            ChannelCommandResponse::Pending(ref id) => ("PENDING", Some(vec![id.to_owned()])),
            ChannelCommandResponse::Result(ref id) => ("RESULT", Some(vec![id.to_owned()])),
            ChannelCommandResponse::Ended(reason) => ("ENDED", Some(vec![reason.to_owned()])),
            ChannelCommandResponse::Err(ref reason) => ("ERR", Some(vec![reason.to_string()])),
        }
    }
}

impl ChannelCommandBase {
    pub fn dispatch_ping(mut parts: SplitWhitespace) -> ChannelResult {
        match parts.next() {
            None => Ok(ChannelCommandResponse::Pong),
            _ => Err(ChannelCommandError::InvalidFormat("PING"))
        }
    }

    pub fn dispatch_quit(mut parts: SplitWhitespace) -> ChannelResult {
        match parts.next() {
            None => Ok(ChannelCommandResponse::Ended("quit")),
            _ => Err(ChannelCommandError::InvalidFormat("QUIT"))
        }
    }
}

impl ChannelCommandSearch {
    pub fn dispatch_query(mut parts: SplitWhitespace) -> ChannelResult {
        match (parts.next(), parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(collection), Some(bucket), Some(terms), meta_part, None) => {
                // Generate command identifier
                let query_id = Self::generate_identifier();

                debug!(
                    "dispatching search query #{} on collection: {} and bucket: {} with terms: {}",
                    query_id, collection, bucket, terms
                );

                // TODO: dispatch async query
                // TODO: for now, block thread and dispatch async result immediately after writing the \
                //   "pending" section, and mark a TODO for later to make things really async and multi-\
                //   threaded.

                // Append command meta?
                if let Some(meta) = meta_part {
                    debug!("got meta on search query #{}: {}", query_id, meta);

                    // TODO: parse 'meta' using a common parser that is capable of splitting X(Y)
                }

                Ok(ChannelCommandResponse::Pending(query_id))
            }
            _ => Err(ChannelCommandError::InvalidFormat(
                "QUERY <collection> <bucket> <terms> [LIMIT(<count>)]?",
            )),
        }
    }

    fn generate_identifier() -> String {
        thread_rng()
            .sample_iter(&Alphanumeric)
            .take(SEARCH_QUERY_ID_SIZE)
            .collect()
    }
}

impl ChannelCommandIngest {
    pub fn dispatch_push(mut parts: SplitWhitespace) -> ChannelResult {
        // TODO: support for text data with spaces (recursive scan of parts?)

        match (parts.next(), parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(collection), Some(bucket), Some(object), Some(text), None) => {
                debug!(
                    "dispatching ingest push in collection: {}, bucket: {} and object: {}",
                    collection, bucket, object
                );
                debug!("ingest has text: {}", text);

                // TODO: validate push parts
                // TODO: push op

                Ok(ChannelCommandResponse::Ok)
            }
            _ => Err(ChannelCommandError::InvalidFormat(
                "PUSH <collection> <bucket> <object> <text>",
            )),
        }
    }

    pub fn dispatch_pop(mut parts: SplitWhitespace) -> ChannelResult {
        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(collection), Some(bucket), Some(object), None) => {
                let count = 0;

                debug!(
                    "dispatching ingest pop in collection: {}, bucket: {} and object: {}",
                    collection, bucket, object
                );

                // TODO: validate pop parts
                // TODO: pop op

                Ok(ChannelCommandResponse::Result(count.to_string()))
            }
            _ => Err(ChannelCommandError::InvalidFormat(
                "POP <collection> <bucket> <object>",
            )),
        }
    }

    pub fn dispatch_count(mut parts: SplitWhitespace) -> ChannelResult {
        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(collection), Some(bucket), object_part, None) => {
                let count = 0;

                debug!(
                    "dispatching ingest count in collection: {}, bucket: {}", collection, bucket
                );

                // Count in object?
                if let Some(object) = object_part {
                    debug!("got ingest count object: {}", object);

                    // TODO
                }

                // TODO: validate count parts
                // TODO: count op

                Ok(ChannelCommandResponse::Result(count.to_string()))
            }
            _ => Err(ChannelCommandError::InvalidFormat(
                "COUNT <collection> <bucket> [<object>]?",
            )),
        }
    }

    pub fn dispatch_flushc(mut parts: SplitWhitespace) -> ChannelResult {
        match (parts.next(), parts.next()) {
            (Some(collection), None) => {
                let count = 0;

                debug!("dispatching ingest flush collection in collection: {}", collection);

                // TODO: validate parts
                // TODO: count op

                Ok(ChannelCommandResponse::Result(count.to_string()))
            }
            _ => Err(ChannelCommandError::InvalidFormat("FLUSHC <collection>"))
        }
    }

    pub fn dispatch_flushb(mut parts: SplitWhitespace) -> ChannelResult {
        match (parts.next(), parts.next(), parts.next()) {
            (Some(collection), Some(bucket), None) => {
                let count = 0;

                debug!(
                    "dispatching ingest flush bucket in collection: {}, bucket: {}",
                    collection, bucket
                );

                // TODO: validate parts
                // TODO: count op

                Ok(ChannelCommandResponse::Result(count.to_string()))
            }
            _ => Err(ChannelCommandError::InvalidFormat("FLUSHB <collection> <bucket>"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_matches_command_response_string() {
        assert_eq!(ChannelCommandResponse::Nil.to_str(), "NIL");
        assert_eq!(ChannelCommandResponse::Ok.to_str(), "OK");
        assert_eq!(ChannelCommandResponse::Pong.to_str(), "PONG");
        assert_eq!(ChannelCommandResponse::Ended.to_str(), "ENDED");
        assert_eq!(ChannelCommandResponse::Err.to_str(), "ERR");
    }
}
