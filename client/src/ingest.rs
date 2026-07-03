// Sonic
//
// Fast, lightweight and schema-less ingest backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::channel::{ChannelMode, SonicChannel};
use crate::options::Lang;
use crate::util::errors::io_error_invalid_data;
use crate::util::{impl_channel_structs, impl_fns, make_command};

// NOTE: Shorter type aliases.
use self::IngestMode as Mode;
use self::IngestModeDiscriminant as Discriminant;

impl_channel_structs!(Ingest("ingest"):
    SonicChannelIngest / SonicChannelIngestBlocking / SonicChannelIngestAsync
);

enum IngestMode {}

/// Disciminants for all possible Sonic messages (response lines) when in
/// Ingest mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum IngestModeDiscriminant {
    Pong,
    Ok,
    Result,
    Ended,
}

impl crate::channel::Discriminant for IngestModeDiscriminant {
    #[inline]
    fn has_payload(&self) -> bool {
        false
    }
}

impl ChannelMode for IngestMode {
    type Discriminant = IngestModeDiscriminant;

    fn name() -> &'static str {
        "ingest"
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

// MARK: PUSH

pub trait PushOption: std::fmt::Display + Sync {}

impl<'a> PushOption for Lang<'a> {}

impl_fns!(
    #[doc = "Time complexity: O(1)."]
    #[inline]
    fn push(
        &self,
        collection: impl AsRef<str>,
        bucket: impl AsRef<str>,
        object: impl AsRef<str>,
        text: impl AsRef<str>,
    ) -> std::io::Result<()> {
        self.push_with_options(collection, bucket, object, text, &[])
    }
);

impl_fns!(
    #[doc = "Time complexity: O(1)."]
    fn push_with_options<'a>(
        &self,
        collection: impl AsRef<str>,
        bucket: impl AsRef<str>,
        object: impl AsRef<str>,
        text: impl AsRef<str>,
        options: &[&'a dyn PushOption],
    ) -> std::io::Result<()> {
        self.inner.send_buffered(
            make_command!("PUSH {} {} {}", collection, bucket, object; text: text; options: options),
            Discriminant::Ok,
            |_acc, _data| Ok(())
        )
    }
);

// MARK: POP

impl_fns!(
    #[doc = "Time complexity: O(1)."]
    fn pop(
        &self,
        collection: impl AsRef<str>,
        bucket: impl AsRef<str>,
        object: impl AsRef<str>,
        text: impl AsRef<str>,
    ) -> std::io::Result<usize> {
        self.inner.send_buffered(
            make_command!(
                "POP {} {} {}",
                collection,
                bucket,
                object;
                text: text
            ),
            Discriminant::Result,
            |acc, data| {
                data.parse::<usize>()
                    .map(|n| acc + n)
                    .map_err(io_error_invalid_data)
            },
        )
    }
);

// MARK: COUNT

impl_fns!(
    #[doc = "Time complexity: O(1)."]
    fn countc(&self, collection: impl AsRef<str>) -> std::io::Result<usize> {
        self.inner.send(
            make_command!("COUNT {}", collection),
            Discriminant::Result,
            |data| data.parse().map_err(io_error_invalid_data),
        )
    }
);

impl_fns!(
    #[doc = "Time complexity: O(1)."]
    fn countb(
        &self,
        collection: impl AsRef<str>,
        bucket: impl AsRef<str>,
    ) -> std::io::Result<usize> {
        self.inner.send(
            make_command!("COUNT {} {}", collection, bucket),
            Discriminant::Result,
            |data| data.parse().map_err(io_error_invalid_data),
        )
    }
);

impl_fns!(
    #[doc = "Time complexity: O(1)."]
    fn counto(
        &self,
        collection: impl AsRef<str>,
        bucket: impl AsRef<str>,
        object: impl AsRef<str>,
    ) -> std::io::Result<usize> {
        self.inner.send(
            make_command!("COUNT {} {} {}", collection, bucket, object),
            Discriminant::Result,
            |data| data.parse().map_err(io_error_invalid_data),
        )
    }
);

// MARK: FLUSH*

impl_fns!(
    #[doc = "Time complexity: O(1)."]
    fn flushc(&self, collection: impl AsRef<str>) -> std::io::Result<()> {
        self.inner.send(
            make_command!("FLUSHC {}", collection),
            Discriminant::Ok,
            |_data| Ok(()),
        )
    }
);

impl_fns!(
    #[doc = "Time complexity: O(N) where N is the number of bucket objects."]
    fn flushb(&self, collection: impl AsRef<str>, bucket: impl AsRef<str>) -> std::io::Result<()> {
        self.inner.send(
            make_command!("FLUSHB {} {}", collection, bucket),
            Discriminant::Ok,
            |_data| Ok(()),
        )
    }
);

impl_fns!(
    #[doc = "Time complexity: O(1)."]
    fn flusho(
        &self,
        collection: impl AsRef<str>,
        bucket: impl AsRef<str>,
        object: impl AsRef<str>,
    ) -> std::io::Result<()> {
        self.inner.send(
            make_command!("FLUSHO {} {} {}", collection, bucket, object),
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
