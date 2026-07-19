// Sonic
//
// Fast, lightweight and schema-less control backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::str::FromStr;

use crate::channel::{ChannelMode, SonicChannel};
use crate::util::errors::io_error_invalid_data;
use crate::util::{impl_channel_structs, impl_fns, make_command};

// NOTE: Shorter type aliases.
use self::ControlMode as Mode;
use self::ControlModeDiscriminant as Discriminant;

impl_channel_structs!(Control("control"):
    SonicChannelControl / SonicChannelControlBlocking / SonicChannelControlAsync
);

enum ControlMode {}

/// Disciminants for all possible Sonic messages (response lines) when in
/// Control mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ControlModeDiscriminant {
    Pong,
    Ok,
    Result,
    Ended,
}

impl crate::channel::Discriminant for ControlModeDiscriminant {
    #[inline]
    fn has_payload(&self) -> bool {
        false
    }
}

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

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct CollectionStats {
    pub collection: String,
    #[serde(default)]
    pub schema_version: u32,
    pub index: ColumnFamilyStats,
    #[serde(default)]
    pub postings: ColumnFamilyStats,
    pub documents: ColumnFamilyStats,
    pub logical: Option<LogicalStats>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ColumnFamilyStats {
    pub live_data_bytes: u64,
    pub sst_bytes: u64,
    pub memtable_bytes: u64,
    pub estimated_keys: u64,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct LogicalStats {
    pub index_key_bytes: u64,
    pub index_value_bytes: u64,
    pub document_key_bytes: u64,
    pub document_encoded_bytes: u64,
    pub document_text_bytes: u64,
    pub document_metadata_bytes: u64,
    pub document_count: u64,
    pub term_postings: PostingStats,
    pub time_postings: PostingStats,
    pub families: Vec<IndexFamilyStats>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct PostingStats {
    pub fragments: u64,
    pub sparse_fragments: u64,
    pub dense_fragments: u64,
    pub encoded_bytes: u64,
    pub associations: u64,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct IndexFamilyStats {
    pub index: u8,
    pub name: String,
    pub keys: u64,
    pub key_bytes: u64,
    pub value_bytes: u64,
}

impl_fns!(
    #[doc = "Returns physical and optional deep logical collection statistics."]
    fn stats(&self, collection: impl AsRef<str>, deep: bool) -> std::io::Result<CollectionStats> {
        let command = if deep {
            make_command!("STATS {} DEEP", collection)
        } else {
            make_command!("STATS {}", collection)
        };
        self.inner.send(command, Discriminant::Result, |data| {
            serde_json::from_str(data).map_err(io_error_invalid_data)
        })
    }
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerStats {
    pub uptime: u32,
    pub clients_connected: u32,
    pub commands_total: u32,
    pub command_latency_best: u32,
    pub command_latency_worst: u32,
    pub kv_open_count: u32,
    pub fst_open_count: u32,
    pub fst_consolidate_count: u32,
}

impl std::str::FromStr for ServerStats {
    type Err = std::io::Error;

    /// ```
    /// use std::str::FromStr as _;
    ///
    /// use sonic_client::control::ServerStats;
    ///
    /// // Parsing works.
    /// assert_eq!(
    ///     ServerStats::from_str("uptime(20868) clients_connected(1) commands_total(189) command_latency_best(1) command_latency_worst(6) kv_open_count(0) fst_open_count(0) fst_consolidate_count(0)").unwrap(),
    ///     ServerStats {
    ///         uptime: 20868,
    ///         clients_connected: 1,
    ///         commands_total: 189,
    ///         command_latency_best: 1,
    ///         command_latency_worst: 6,
    ///         kv_open_count: 0,
    ///         fst_open_count: 0,
    ///         fst_consolidate_count: 0,
    ///     }
    /// );
    ///
    /// // Missing keys raise errors.
    /// assert!(ServerStats::from_str("uptime(20868)").is_err());
    ///
    /// // Unknown keys do not raise errors.
    /// assert_eq!(
    ///     ServerStats::from_str("uptime(20868) clients_connected(1) commands_total(189) command_latency_best(1) command_latency_worst(6) kv_open_count(0) fst_open_count(0) fst_consolidate_count(0) foo(bar)").unwrap(),
    ///     ServerStats {
    ///         uptime: 20868,
    ///         clients_connected: 1,
    ///         commands_total: 189,
    ///         command_latency_best: 1,
    ///         command_latency_worst: 6,
    ///         kv_open_count: 0,
    ///         fst_open_count: 0,
    ///         fst_consolidate_count: 0,
    ///     }
    /// );
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut uptime: Option<u32> = None;
        let mut clients_connected: Option<u32> = None;
        let mut commands_total: Option<u32> = None;
        let mut command_latency_best: Option<u32> = None;
        let mut command_latency_worst: Option<u32> = None;
        let mut kv_open_count: Option<u32> = None;
        let mut fst_open_count: Option<u32> = None;
        let mut fst_consolidate_count: Option<u32> = None;

        for arg in s.split(' ') {
            let Some(stripped) = arg.strip_suffix(')') else {
                return Err(io_error_invalid_data(format!(
                    "Arg does not end with ')': {arg:?}"
                )));
            };

            let Some((key, value)) = stripped.split_once('(') else {
                return Err(io_error_invalid_data(format!(
                    "Arg does not contain '(': {arg:?}"
                )));
            };

            /// Parses the given value using `FromStr` and stores the result in
            /// the given optional. If a value was already present, print a
            /// warning (or panic in debug mode) as this shouldn’t happen.
            macro_rules! update {
                ($store:ident with $value:ident) => {{
                    let new_value = $value.parse().map_err(io_error_invalid_data)?;
                    let old_value = $store.replace(new_value);

                    if let Some(old_value) = old_value {
                        eprintln!("{key:?} was provided multiple times, using new value (old: {old_value}, new: {new_value}).");
                    }
                }};
            }

            match (key, value) {
                ("uptime", v) => update!(uptime with v),
                ("clients_connected", v) => update!(clients_connected with v),
                ("commands_total", v) => update!(commands_total with v),
                ("command_latency_best", v) => update!(command_latency_best with v),
                ("command_latency_worst", v) => update!(command_latency_worst with v),
                ("kv_open_count", v) => update!(kv_open_count with v),
                ("fst_open_count", v) => update!(fst_open_count with v),
                ("fst_consolidate_count", v) => update!(fst_consolidate_count with v),
                _ => eprintln!("Unknown info: {arg:?}"),
            }
        }

        macro_rules! info_not_found {
            ($key:expr) => {
                io_error_invalid_data(format!("Key {key:?} not found in {s:?}", key = $key))
            };
        }

        macro_rules! ensure_defined {
            ($val:ident) => {
                let Some($val) = $val else {
                    return Err(info_not_found!(stringify!($val)));
                };
            };
        }

        ensure_defined!(uptime);
        ensure_defined!(clients_connected);
        ensure_defined!(commands_total);
        ensure_defined!(command_latency_best);
        ensure_defined!(command_latency_worst);
        ensure_defined!(kv_open_count);
        ensure_defined!(fst_open_count);
        ensure_defined!(fst_consolidate_count);

        Ok(Self {
            uptime,
            clients_connected,
            commands_total,
            command_latency_best,
            command_latency_worst,
            kv_open_count,
            fst_open_count,
            fst_consolidate_count,
        })
    }
}

impl_fns!(
    #[doc = "Time complexity: O(1)."]
    fn info(&self) -> std::io::Result<ServerStats> {
        self.inner.send(
            make_command!("INFO"),
            Discriminant::Result,
            ServerStats::from_str,
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
