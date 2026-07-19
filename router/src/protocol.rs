// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Crisp IM SAS
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::collections::BTreeMap;
use std::str::FromStr;

use base64::Engine;

use crate::error::{RouterError, RouterResult};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChannelMode {
    Search,
    Ingest,
    Control,
}

impl ChannelMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Search => "search",
            Self::Ingest => "ingest",
            Self::Control => "control",
        }
    }
}

impl FromStr for ChannelMode {
    type Err = RouterError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "search" => Ok(Self::Search),
            "ingest" => Ok(Self::Ingest),
            "control" => Ok(Self::Control),
            _ => Err(RouterError::code("invalid_mode")),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RoutedCommand {
    Local(&'static str),
    Broadcast,
    BroadcastList,
    Bucket {
        collection: String,
        bucket: String,
        writing: bool,
    },
    Batch(BatchCommand),
    Reject(&'static str),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchCommand {
    pub collection: String,
    pub mode: String,
    pub records: Vec<BatchRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchRecord {
    pub bucket: String,
    pub encoded: Vec<u8>,
}

impl BatchCommand {
    pub fn parse(line: &str) -> RouterResult<Self> {
        let parts = line.split_whitespace().collect::<Vec<_>>();
        let [command, collection, mode, payload] = parts.as_slice() else {
            return Err(RouterError::code("invalid_format"));
        };

        if *command != "UPSERTBATCH" || !matches!(*mode, "fresh" | "upsert") {
            return Err(RouterError::code("invalid_format"));
        }

        let compressed = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| RouterError::code("invalid_batch_base64"))?;
        let decoded = zstd::stream::decode_all(compressed.as_slice())
            .map_err(|_| RouterError::code("invalid_batch_zstd"))?;

        let mut records = Vec::new();

        for encoded in decoded.split(|byte| *byte == b'\n') {
            if encoded.is_empty() {
                continue;
            }

            let value: serde_json::Value = serde_json::from_slice(encoded)
                .map_err(|_| RouterError::code("invalid_batch_json"))?;
            let bucket = value
                .get("bucket")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| RouterError::code("batch_bucket_missing"))?;

            records.push(BatchRecord {
                bucket: bucket.to_owned(),
                encoded: encoded.to_vec(),
            });
        }

        Ok(Self {
            collection: (*collection).to_owned(),
            mode: (*mode).to_owned(),
            records,
        })
    }

    pub fn encode_groups(
        &self,
        groups: BTreeMap<String, Vec<&BatchRecord>>,
    ) -> RouterResult<BTreeMap<String, String>> {
        groups
            .into_iter()
            .map(|(backend, records)| {
                let mut ndjson = Vec::new();
                for record in records {
                    ndjson.extend_from_slice(&record.encoded);
                    ndjson.push(b'\n');
                }

                let compressed = zstd::stream::encode_all(ndjson.as_slice(), 1)?;
                let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(compressed);

                Ok((
                    backend,
                    format!("UPSERTBATCH {} {} {}", self.collection, self.mode, payload),
                ))
            })
            .collect()
    }
}

pub fn classify(mode: ChannelMode, line: &str) -> RoutedCommand {
    let parts = line.split_whitespace().collect::<Vec<_>>();
    let Some(command) = parts.first().copied() else {
        return RoutedCommand::Local("");
    };

    match command {
        "PING" if parts.len() == 1 => return RoutedCommand::Local("PONG"),
        "QUIT" if parts.len() == 1 => return RoutedCommand::Local("ENDED quit"),
        "HELP" => return RoutedCommand::Local(help(mode)),
        _ => {}
    }

    match mode {
        ChannelMode::Search => classify_bucket(&parts, &["QUERY", "QUERYDOCS", "LIST"], false),
        ChannelMode::Ingest => classify_ingest(line, &parts),
        ChannelMode::Control => match parts.as_slice() {
            ["TRIGGER", "consolidate"] | ["TRIGGER", "backup", _] | ["TRIGGER", "restore", _] => {
                RoutedCommand::Broadcast
            }
            _ => RoutedCommand::Reject("router_global_command"),
        },
    }
}

fn classify_ingest(line: &str, parts: &[&str]) -> RoutedCommand {
    if parts.first() == Some(&"UPSERTBATCH") {
        return match BatchCommand::parse(line) {
            Ok(batch) => RoutedCommand::Batch(batch),
            Err(_) => RoutedCommand::Reject("invalid_format"),
        };
    }

    let command = parts.first().copied().unwrap_or_default();

    if matches!(command, "PUSH" | "UPSERT" | "POP" | "FLUSHB" | "FLUSHO") {
        return classify_bucket(parts, &[command], true);
    }

    if command == "COUNT" && parts.len() >= 3 {
        return classify_bucket(parts, &["COUNT"], false);
    }

    // DUMP always targets one bucket (with optional LIMIT/OFFSET meta parts trailing), so it \
    //   routes like any other bucket-scoped command.
    if command == "DUMP" && parts.len() >= 3 {
        return classify_bucket(parts, &["DUMP"], false);
    }

    // FLUSHC and bucket-less COUNT target a whole collection, which may be sharded across \
    //   every backend, so they are broadcast to all of them rather than routed to a single one.
    if matches!(command, "FLUSHC" | "COUNT") && parts.len() == 2 {
        return RoutedCommand::Broadcast;
    }

    // BUCKETS enumerates bucket names cluster-wide; unlike FLUSHC/COUNT it aggregates a list \
    //   (with de-duplication) rather than a numeric sum, so it gets its own broadcast kind.
    if command == "BUCKETS" && parts.len() >= 2 {
        return RoutedCommand::BroadcastList;
    }

    match command {
        "EXPORT" | "IMPORT" => RoutedCommand::Reject("router_collection_scope"),
        _ => RoutedCommand::Reject("unknown_command"),
    }
}

fn classify_bucket(parts: &[&str], allowed: &[&str], writing: bool) -> RoutedCommand {
    if parts.len() < 3 || !allowed.contains(&parts[0]) {
        return RoutedCommand::Reject("invalid_format");
    }

    RoutedCommand::Bucket {
        collection: parts[1].to_owned(),
        bucket: parts[2].to_owned(),
        writing,
    }
}

fn help(mode: ChannelMode) -> &'static str {
    match mode {
        ChannelMode::Search => "RESULT commands(QUERY, QUERYDOCS, LIST, PING, HELP, QUIT)",
        ChannelMode::Ingest => {
            "RESULT commands(PUSH, UPSERT, UPSERTBATCH, POP, COUNT, DUMP, BUCKETS, FLUSHB, FLUSHC, FLUSHO, PING, HELP, QUIT)"
        }
        ChannelMode::Control => "RESULT commands(PING, HELP, QUIT)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_bucket_command_without_parsing_text() {
        assert_eq!(
            classify(
                ChannelMode::Ingest,
                r#"PUSH messages bucket:1 object "some text""#
            ),
            RoutedCommand::Bucket {
                collection: "messages".to_owned(),
                bucket: "bucket:1".to_owned(),
                writing: true,
            }
        );
    }

    #[test]
    fn broadcasts_collection_wide_flush_and_count() {
        assert_eq!(
            classify(ChannelMode::Ingest, "FLUSHC messages"),
            RoutedCommand::Broadcast
        );
        assert_eq!(
            classify(ChannelMode::Ingest, "COUNT messages"),
            RoutedCommand::Broadcast
        );
    }

    #[test]
    fn routes_bucket_scoped_count_to_a_single_backend() {
        assert_eq!(
            classify(ChannelMode::Ingest, "COUNT messages bucket:1"),
            RoutedCommand::Bucket {
                collection: "messages".to_owned(),
                bucket: "bucket:1".to_owned(),
                writing: false,
            }
        );
    }

    #[test]
    fn routes_dump_to_a_single_backend_without_writing() {
        assert_eq!(
            classify(ChannelMode::Ingest, "DUMP messages bucket:1 LIMIT(100)"),
            RoutedCommand::Bucket {
                collection: "messages".to_owned(),
                bucket: "bucket:1".to_owned(),
                writing: false,
            }
        );
    }

    #[test]
    fn broadcasts_bucket_enumeration_as_a_list() {
        assert_eq!(
            classify(ChannelMode::Ingest, "BUCKETS messages"),
            RoutedCommand::BroadcastList
        );
        assert_eq!(
            classify(ChannelMode::Ingest, "BUCKETS messages LIMIT(100) OFFSET(200)"),
            RoutedCommand::BroadcastList
        );
    }

    #[test]
    fn rejects_collection_wide_scoped_commands() {
        assert_eq!(
            classify(ChannelMode::Ingest, "EXPORT messages /tmp/out.ndjson"),
            RoutedCommand::Reject("router_collection_scope")
        );
    }

    #[test]
    fn decodes_batch_buckets_for_shard_splitting() {
        let ndjson = br#"{"bucket":"bucket:1","oid":"a"}
{"bucket":"bucket:2","oid":"b"}
"#;
        let compressed = zstd::stream::encode_all(ndjson.as_slice(), 1).unwrap();
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(compressed);
        let batch = BatchCommand::parse(&format!("UPSERTBATCH messages upsert {payload}")).unwrap();

        assert_eq!(batch.collection, "messages");
        assert_eq!(batch.records[0].bucket, "bucket:1");
        assert_eq!(batch.records[1].bucket, "bucket:2");
    }

    #[test]
    fn broadcasts_safe_control_triggers_only() {
        assert_eq!(
            classify(ChannelMode::Control, "TRIGGER consolidate"),
            RoutedCommand::Broadcast
        );
        assert_eq!(
            classify(ChannelMode::Control, "INFO"),
            RoutedCommand::Reject("router_global_command")
        );
    }
}
