// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

#[macro_export]
macro_rules! gen_channel_message_mode_handle {
    ($message:ident, $commands:ident, { $($external:expr => $internal:expr),+, }) => {{
        let mut parts: SplitWhitespace = $message.split_whitespace();
        let command = parts.next().unwrap_or("").to_uppercase();

        debug!("will dispatch search command: {}", command);

        if command.is_empty() == true || $commands.contains(&command.as_str()) == true {
            match command.as_str() {
                "" => ChannelResult::Sync(Ok(ChannelCommandResponse::Void)),
                $(
                    $external => $internal(parts),
                )+
                "PING" => ChannelCommandBase::dispatch_ping(parts),
                "QUIT" => ChannelCommandBase::dispatch_quit(parts),
                _ => ChannelResult::Sync(Ok(ChannelCommandResponse::Err(
                    ChannelCommandError::InternalError,
                ))),
            }
        } else {
            ChannelResult::Sync(Ok(ChannelCommandResponse::Err(
                ChannelCommandError::UnknownCommand,
            )))
        }
    }};
}
