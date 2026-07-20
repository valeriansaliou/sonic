// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::str::FromStr;

use crate::logging::*;
use crate::util::errors::io_error_invalid_data;

/// Parses a string slice into substrings by simply splitting on spaces. This
/// would break if results are quoted and contain spaces, but it’s sufficient
/// for situations like parsing `EVENT QUERY` results.
pub fn parse_string_vec_naive(line: &str) -> Vec<Box<str>> {
    if line.is_empty() {
        return Vec::with_capacity(0);
    }

    let item_count = line.as_bytes().iter().filter(|&&b| b == b' ').count() + 1;

    let mut res: Vec<Box<str>> = Vec::with_capacity(item_count);

    for item in line.split(' ') {
        res.push(Box::from(item));
    }

    res
}

// MARK: CONNECTED

pub struct Connected {
    pub server_info: ServerInfo,
}

impl FromStr for Connected {
    type Err = std::io::Error;

    fn from_str(line: &str) -> Result<Self, Self::Err> {
        let Some((discriminant, rest)) = line.split_once(' ') else {
            return Err(io_error_invalid_data("Line missing discriminant"));
        };

        if discriminant != "CONNECTED" {
            return Err(io_error_invalid_data(format!(
                "Incorrect line discriminant: expected \"CONNECTED\", got {discriminant:?}"
            )));
        }

        let server_info = ServerInfo::from_str(rest)
            .inspect_err(|error| log_error!("{error}"))
            .unwrap_or_default();

        Ok(Self { server_info })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInfo {
    pub version: Box<str>,

    /// Unknown data. No real use, but there for future-proofing.
    pub additional_data: Box<str>,
}

impl Default for ServerInfo {
    fn default() -> Self {
        Self {
            version: Box::from("unknown"),
            additional_data: Box::default(),
        }
    }
}

impl std::str::FromStr for ServerInfo {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let Some(rest) = s.strip_prefix('<') else {
            return Err(format!("Missing '<' in {s:?}"));
        };

        let Some((version, rest)) = rest.split_once('>') else {
            return Err(format!("Missing '<' in {s:?}"));
        };

        Ok(Self {
            version: Box::from(version),
            additional_data: Box::from(rest.trim_ascii_start()),
        })
    }
}

// MARK: STARTED

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelInfo {
    pub protocol_version: u8,
    pub buffer_size: usize,
    pub bulk_buffer_size: usize,
}

impl std::str::FromStr for ChannelInfo {
    type Err = std::io::Error;

    /// ```
    /// use std::str::FromStr as _;
    ///
    /// use sonic_client::ChannelInfo;
    ///
    /// // Parsing works.
    /// assert_eq!(
    ///     ChannelInfo::from_str("protocol(1) buffer(20000)").unwrap(),
    ///     ChannelInfo { protocol_version: 1, buffer_size: 20000, bulk_buffer_size: 20000 }
    /// );
    ///
    /// // Missing keys raise errors.
    /// assert!(ChannelInfo::from_str("protocol(1)").is_err());
    /// assert!(ChannelInfo::from_str("buffer(20000)").is_err());
    ///
    /// // Unknown keys do not raise errors.
    /// assert_eq!(
    ///     ChannelInfo::from_str("protocol(1) buffer(20000) foo(bar)").unwrap(),
    ///     ChannelInfo { protocol_version: 1, buffer_size: 20000, bulk_buffer_size: 20000 }
    /// );
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut protocol_version: Option<u8> = None;
        let mut buffer_size: Option<usize> = None;
        let mut bulk_buffer_size: Option<usize> = None;

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
                ("protocol", v) => update!(protocol_version with v),
                ("buffer", v) => update!(buffer_size with v),
                ("bulk_buffer", v) => update!(bulk_buffer_size with v),
                _ => eprintln!("Unknown info: {arg:?}"),
            }
        }

        macro_rules! info_not_found {
            ($key:literal) => {
                io_error_invalid_data(format!("Key {key:?} not found in {s:?}", key = $key))
            };
        }

        let Some(protocol_version) = protocol_version else {
            return Err(info_not_found!("protocol"));
        };

        let Some(buffer_size) = buffer_size else {
            return Err(info_not_found!("buffer"));
        };

        Ok(Self {
            protocol_version,
            buffer_size,
            bulk_buffer_size: bulk_buffer_size.unwrap_or(buffer_size),
        })
    }
}
