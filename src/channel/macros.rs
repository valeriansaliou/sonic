// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Sonic OSS License v1.0 (SOSSL v1.0)

#[macro_export]
macro_rules! gen_channel_message_mode_handle {
    ($message:ident, $commands:ident, { $($external:expr => $internal:expr),+, }) => {{
        let (command, parts) = ChannelMessage::extract($message);

        if command.is_empty() == true || $commands.contains(&command.as_str()) == true {
            match command.as_str() {
                "" => Ok(vec![ChannelCommandResponse::Void]),
                $(
                    $external => $internal(parts),
                )+
                "PING" => ChannelCommandBase::dispatch_ping(parts),
                "QUIT" => ChannelCommandBase::dispatch_quit(parts),
                _ => Ok(vec![ChannelCommandResponse::Err(
                    ChannelCommandError::InternalError,
                )]),
            }
        } else {
            Ok(vec![ChannelCommandResponse::Err(
                ChannelCommandError::UnknownCommand,
            )])
        }
    }};
}
