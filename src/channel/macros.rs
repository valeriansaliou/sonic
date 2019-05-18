// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

#[macro_export]
macro_rules! gen_channel_message_mode_handle {
    ($message:ident, $commands:ident, { $($external:expr => $internal:expr),+, }) => {{
        let (command, parts) = ChannelMessageUtils::extract($message);

        if command.is_empty() == true || $commands.contains(&command.as_str()) == true {
            match command.as_str() {
                "" => Ok(ChannelCommandResponse::Void),
                $(
                    $external => $internal(parts),
                )+
                "PING" => ChannelCommandBase::dispatch_ping(parts),
                "QUIT" => ChannelCommandBase::dispatch_quit(parts),
                _ => Ok(ChannelCommandResponse::Err(
                    ChannelCommandError::InternalError,
                )),
            }
        } else {
            Ok(ChannelCommandResponse::Err(
                ChannelCommandError::UnknownCommand,
            ))
        }
    }};
}
