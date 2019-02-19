// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::str::SplitWhitespace;

use super::handle::ChannelShard;

#[derive(PartialEq)]
pub enum ChannelCommandResponse {
    Void,
    Nil,
    Ok,
    Pong,
    Ended,
    Err,
}

pub struct ChannelCommand;

pub const COMMAND_SIZE: usize = 6;

type ChannelResult = Result<ChannelCommandResponse, Option<()>>;

impl ChannelCommandResponse {
    pub fn to_str(&self) -> &'static str {
        match *self {
            ChannelCommandResponse::Void => "",
            ChannelCommandResponse::Nil => "NIL",
            ChannelCommandResponse::Ok => "OK",
            ChannelCommandResponse::Pong => "PONG",
            ChannelCommandResponse::Ended => "ENDED quit",
            ChannelCommandResponse::Err => "ERR",
        }
    }
}

impl ChannelCommand {
    pub fn dispatch_flush_bucket(
        shard: &ChannelShard,
        mut parts: SplitWhitespace,
    ) -> ChannelResult {
        let bucket = parts.next().unwrap_or("");

        if bucket.is_empty() == false {
            // TODO
        }

        Err(None)
    }

    pub fn dispatch_flush_auth(shard: &ChannelShard, mut parts: SplitWhitespace) -> ChannelResult {
        let auth = parts.next().unwrap_or("");

        if auth.is_empty() == false {
            // TODO
        }

        Err(None)
    }

    pub fn dispatch_ping() -> ChannelResult {
        Ok(ChannelCommandResponse::Pong)
    }

    pub fn dispatch_shard(shard: &mut ChannelShard, mut parts: SplitWhitespace) -> ChannelResult {
        match parts.next().unwrap_or("").parse::<u8>() {
            Ok(shard_to) => {
                *shard = shard_to;

                Ok(ChannelCommandResponse::Ok)
            }
            _ => Err(None),
        }
    }

    pub fn dispatch_quit() -> ChannelResult {
        Ok(ChannelCommandResponse::Ended)
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
        assert_eq!(ChannelCommandResponse::Ended.to_str(), "ENDED quit");
        assert_eq!(ChannelCommandResponse::Err.to_str(), "ERR");
    }
}
