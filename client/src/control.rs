// Sonic
//
// Fast, lightweight and schema-less control backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::channel::{ChannelMode, SonicChannel};
use crate::util::errors::io_error_invalid_data;
use crate::util::{impl_channel_structs, impl_fns, make_command};

// NOTE: Shorter type aliases.
use self::ControlMode as Mode;
pub use self::ControlModeDiscriminant as Discriminant;

impl_channel_structs!(Control("control"):
    SonicChannelControl / SonicChannelControlBlocking / SonicChannelControlAsync
);

enum ControlMode {}

/// Disciminants for all possible Sonic messages (response lines) when in
/// Control mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ControlModeDiscriminant {
    Pong,
    Ok,
    Result,
    Ended,
}

impl crate::channel::Discriminant for ControlModeDiscriminant {}

impl ChannelMode for ControlMode {
    type Discriminant = ControlModeDiscriminant;

    fn name() -> &'static str {
        "control"
    }

    fn parse<'a>(
        discriminant: &'a str,
        rest: &'a str,
    ) -> std::io::Result<(Self::Discriminant, &'a str)> {
        match discriminant {
            "PONG" => Ok((Discriminant::Pong, rest)),
            "OK" => Ok((Discriminant::Ok, rest)),
            "RESULT" => Ok((Discriminant::Result, rest)),
            "ENDED" => Ok((Discriminant::Ended, rest)),
            "ERR" => Err(std::io::Error::other(rest)),
            s => Err(io_error_invalid_data(format!(
                "Unknown line discriminant: {s:?}"
            ))),
        }
    }
}

// MARK: TRIGGER

impl_fns!(
    #[doc = "Time complexity: O(1)."]
    fn trigger_consolidate(&self) -> std::io::Result<()> {
        self.inner.send(
            make_command!("TRIGGER consolidate"),
            Discriminant::Ok,
            |_data| Ok(()),
        )
    }
);

impl_fns!(
    #[doc = "Time complexity: O(1)."]
    fn trigger_backup(&self, path: impl AsRef<str>) -> std::io::Result<()> {
        self.inner.send(
            make_command!("TRIGGER backup {}", path),
            Discriminant::Ok,
            |_data| Ok(()),
        )
    }
);

impl_fns!(
    #[doc = "Time complexity: O(1)."]
    fn trigger_restore(&self, path: impl AsRef<str>) -> std::io::Result<()> {
        self.inner.send(
            make_command!("TRIGGER restore {}", path),
            Discriminant::Ok,
            |_data| Ok(()),
        )
    }
);

// MARK: INFO

impl_fns!(
    #[doc = "Time complexity: O(1)."]
    fn info(&self) -> std::io::Result<String> {
        self.inner
            .send(make_command!("INFO"), Discriminant::Result, |s| {
                Ok(s.to_owned())
            })
    }
);

// MARK: CONFIG

#[cfg(feature = "experimental-api")]
impl_fns!(
    #[doc = "Time complexity: O(1)."]
    fn config_set(
        &self,
        collection: impl AsRef<str>,
        configs: &[impl AsRef<str>],
    ) -> std::io::Result<()> {
        // Construct args list.
        let mut args = String::with_capacity(
            configs.iter().map(|s| s.as_ref().len()).sum::<usize>() + configs.len(),
        );
        for config in configs {
            args.push(' ');
            args.push_str(config.as_ref());
        }

        // Drop the leading space (purely aesthetic for `make_command`).
        let args = &args[1..];

        self.inner.send(
            make_command!("CONFIG {} SET {}", collection, args),
            Discriminant::Ok,
            |_data| Ok(()),
        )
    }
);

#[cfg(feature = "experimental-api")]
impl_fns!(
    #[doc = "Time complexity: O(1)."]
    fn config_reset(
        &self,
        collection: impl AsRef<str>,
        configs: &[impl AsRef<str>],
    ) -> std::io::Result<()> {
        // Construct args list.
        let mut args = String::with_capacity(
            configs.iter().map(|s| s.as_ref().len()).sum::<usize>() + configs.len(),
        );
        for config in configs {
            args.push(' ');
            args.push_str(config.as_ref());
        }

        // Drop the leading space (purely aesthetic for `make_command`).
        let args = &args[1..];

        self.inner.send(
            make_command!("CONFIG {} RESET {}", collection, args),
            Discriminant::Ok,
            |_data| Ok(()),
        )
    }
);

#[cfg(feature = "experimental-api")]
impl_fns!(
    #[doc = "Time complexity: O(1)."]
    fn config_reset_all(&self, collection: impl AsRef<str>) -> std::io::Result<()> {
        self.inner.send(
            make_command!("CONFIG {} RESET", collection),
            Discriminant::Ok,
            |_data| Ok(()),
        )
    }
);

// MARK: PING

impl_fns!(
    #[doc = "Time complexity: O(1)."]
    fn ping(&self) -> std::io::Result<()> {
        self.inner
            .send(make_command!("PING"), Discriminant::Pong, |_data| Ok(()))
    }
);

// MARK: QUIT

impl_fns!(
    #[doc = "Time complexity: O(1)."]
    fn quit(&mut self) -> std::io::Result<()> {
        let res = (self.inner).send(make_command!("QUIT"), Discriminant::Ended, |_data| Ok(()));

        // NOTE: We mark closed even though the call should fail, because
        //   `Drop` would do the same anyway.
        self.inner.mark_closed();

        res
    }
);

// MARK: Raw

impl_fns!(
    #[doc = "Send a raw command."]
    fn send<T>(
        &self,
        command: crate::Command,
        discriminant: self::Discriminant,
        parse: impl Fn(&str) -> std::io::Result<T> + Send + 'static,
    ) -> std::io::Result<T> {
        self.inner.send(command, discriminant, parse)
    }
    where T: Send + 'static,
);
