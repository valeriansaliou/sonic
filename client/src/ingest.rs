// Sonic
//
// Fast, lightweight and schema-less ingest backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::channel::{ChannelMode, SonicChannel};
use crate::options::{Lang, Metadata, Timestamp};
use crate::search::Document;
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

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BulkDocument {
    pub bucket: String,
    #[serde(flatten)]
    pub document: Document,
}

#[derive(Clone, Copy, Debug)]
pub enum BulkMode {
    Fresh,
    Upsert,
}

impl std::fmt::Display for BulkMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Fresh => "fresh",
            Self::Upsert => "upsert",
        })
    }
}

impl AsRef<str> for BulkMode {
    fn as_ref(&self) -> &str {
        match self {
            Self::Fresh => "fresh",
            Self::Upsert => "upsert",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BulkResult {
    pub written: usize,
    pub rejected: usize,
}

impl_fns!(
    #[doc = "Upserts a compressed batch of documents."]
    fn upsert_batch(
        &self,
        collection: impl AsRef<str>,
        mode: BulkMode,
        documents: &[BulkDocument],
    ) -> std::io::Result<BulkResult> {
        use base64::Engine;
        let mut ndjson = Vec::new();
        for document in documents {
            serde_json::to_writer(&mut ndjson, document).map_err(io_error_invalid_data)?;
            ndjson.push(b'\n');
        }
        let compressed = zstd::stream::encode_all(ndjson.as_slice(), 1)?;
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(compressed);
        self.inner.send_bulk(
            make_command!("UPSERTBATCH {} {} {}", collection, mode, payload),
            Discriminant::Result,
            |data| {
                let mut parts = data.split_whitespace();
                let written = parts
                    .next()
                    .ok_or_else(|| io_error_invalid_data("missing written count"))?
                    .parse()
                    .map_err(io_error_invalid_data)?;
                let rejected = parts
                    .next()
                    .ok_or_else(|| io_error_invalid_data("missing rejected count"))?
                    .parse()
                    .map_err(io_error_invalid_data)?;
                if parts.next().is_some() {
                    return Err(io_error_invalid_data("unexpected bulk result data"));
                }
                Ok(BulkResult { written, rejected })
            },
        )
    }
);

pub trait UpsertOption: std::fmt::Display + Sync {}

impl<'a> UpsertOption for Lang<'a> {}
impl UpsertOption for Metadata {}
impl UpsertOption for Timestamp {}

impl_fns!(
    #[doc = "Atomically replaces a stored document and its search index."]
    fn upsert(
        &self,
        collection: impl AsRef<str>,
        bucket: impl AsRef<str>,
        object: impl AsRef<str>,
        text: impl AsRef<str>,
        timestamp_ms: u64,
        metadata: &serde_json::Value,
    ) -> std::io::Result<()> {
        let timestamp = Timestamp(timestamp_ms);
        let metadata = Metadata::new(metadata)?;
        let options: &[&dyn UpsertOption] = &[&timestamp, &metadata];
        self.inner.send(
            make_command!(
                "UPSERT {} {} {}",
                collection,
                bucket,
                object;
                text: text;
                options: options
            ),
            Discriminant::Ok,
            |_data| Ok(()),
        )
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
    #[doc = "Streams one page of a bucket's documents over the wire; paginate with `offset` \
             until fewer than `limit` documents come back."]
    fn dump_bucket(
        &self,
        collection: impl AsRef<str>,
        bucket: impl AsRef<str>,
        limit: u16,
        offset: u32,
    ) -> std::io::Result<Vec<BulkDocument>> {
        use base64::Engine;
        let limit = limit.to_string();
        let offset = offset.to_string();
        self.inner.send(
            make_command!(
                "DUMP {} {} LIMIT({}) OFFSET({})",
                collection,
                bucket,
                limit,
                offset
            ),
            Discriminant::Result,
            |data| {
                let compressed = base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .decode(data)
                    .map_err(io_error_invalid_data)?;
                let decoded = zstd::stream::decode_all(compressed.as_slice())?;
                decoded
                    .split(|byte| *byte == b'\n')
                    .filter(|line| !line.is_empty())
                    .map(|line| serde_json::from_slice(line).map_err(io_error_invalid_data))
                    .collect()
            },
        )
    }
);

impl_fns!(
    #[doc = "Enumerates bucket names for a collection, one page at a time."]
    fn list_buckets(
        &self,
        collection: impl AsRef<str>,
        limit: u16,
        offset: u32,
    ) -> std::io::Result<Vec<String>> {
        let limit = limit.to_string();
        let offset = offset.to_string();
        self.inner.send(
            make_command!("BUCKETS {} LIMIT({}) OFFSET({})", collection, limit, offset),
            Discriminant::Result,
            |data| {
                Ok(data
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect::<Vec<_>>())
            },
        )
    }
);

impl_fns!(
    #[doc = "Exports a bucket document stream to a local server path."]
    fn export_documents(
        &self,
        collection: impl AsRef<str>,
        bucket: Option<&str>,
        path: impl AsRef<str>,
    ) -> std::io::Result<usize> {
        let command = if let Some(bucket) = bucket {
            make_command!("EXPORT {} {} {}", collection, bucket, path)
        } else {
            make_command!("EXPORT {} {}", collection, path)
        };
        self.inner.send(command, Discriminant::Result, |data| {
            data.parse().map_err(io_error_invalid_data)
        })
    }
);

impl_fns!(
    #[doc = "Imports a bucket document stream from a local server path."]
    fn import_documents(
        &self,
        collection: impl AsRef<str>,
        path: impl AsRef<str>,
    ) -> std::io::Result<usize> {
        self.inner.send(
            make_command!("IMPORT {} {}", collection, path),
            Discriminant::Result,
            |data| data.parse().map_err(io_error_invalid_data),
        )
    }
);

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
    fn flushc(&self, collection: impl AsRef<str>) -> std::io::Result<usize> {
        self.inner.send(
            make_command!("FLUSHC {}", collection),
            Discriminant::Result,
            |data| data.parse().map_err(io_error_invalid_data),
        )
    }
);

impl_fns!(
    #[doc = "Time complexity: O(N) where N is the number of bucket objects."]
    fn flushb(
        &self,
        collection: impl AsRef<str>,
        bucket: impl AsRef<str>,
    ) -> std::io::Result<usize> {
        self.inner.send(
            make_command!("FLUSHB {} {}", collection, bucket),
            Discriminant::Result,
            |data| data.parse().map_err(io_error_invalid_data),
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
    ) -> std::io::Result<usize> {
        self.inner.send(
            make_command!("FLUSHO {} {} {}", collection, bucket, object),
            Discriminant::Result,
            |data| data.parse().map_err(io_error_invalid_data),
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
