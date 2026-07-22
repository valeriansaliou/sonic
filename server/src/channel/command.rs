// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use base64::Engine;
use hashbrown::HashMap;
use rand::RngExt;
use rand::distr::Alphanumeric;
use std::fmt;
use std::path::Path;
use std::str::{self, SplitWhitespace};
use std::sync::LazyLock;
use std::time::Instant;
use std::vec::Vec;

use sonic::query::{
    ListMetaData, QueryGenericLang, QueryMetaData, QuerySearchLimit, QuerySearchOffset,
};
use sonic::store::operation::StoreOperationDispatch;
use sonic::{Executor, Query};

use super::format::unescape;
use super::message::{
    ChannelMessageModeControl, ChannelMessageModeIngest, ChannelMessageModeSearch,
};
use super::profile::{self, IngestCommandProfile};
use super::statistics::ChannelStatistics;

#[derive(PartialEq)]
pub enum ChannelCommandError {
    UnknownCommand,
    NotFound,
    QueryError,
    InternalError,
    ShuttingDown,
    PolicyReject(&'static str),
    InvalidFormat(&'static str),
    InvalidMetaKey((String, String)),
    InvalidMetaValue((String, String)),
}

#[derive(PartialEq)]
pub enum ChannelCommandResponse {
    Void,
    Ok,
    Pong,
    Pending(String),
    Result(String),
    Event(&'static str, String, String),
    Ended(&'static str),
    Err(ChannelCommandError),
}

pub struct ChannelCommandBase;
pub struct ChannelCommandSearch;
pub struct ChannelCommandIngest;
pub struct ChannelCommandControl;

pub type ChannelCommandResponseArgs = (&'static str, Option<Vec<String>>);

type ChannelResult = Result<Vec<ChannelCommandResponse>, ChannelCommandError>;
type MetaPartsResult<'a> = Result<(&'a str, &'a str), (&'a str, &'a str)>;

pub const EVENT_ID_SIZE: usize = 8;

const TEXT_PART_BOUNDARY: char = '"';
const TEXT_PART_ESCAPE: char = '\\';
const META_PART_GROUP_OPEN: char = '(';
const META_PART_GROUP_CLOSE: char = ')';

static BACKUP_KV_PATH: &str = "kv";
static BACKUP_FST_PATH: &str = "fst";

pub static COMMANDS_MODE_SEARCH: [&str; 6] = ["QUERY", "QUERYDOCS", "LIST", "PING", "HELP", "QUIT"];
pub static COMMANDS_MODE_INGEST: [&str; 15] = [
    "PUSH",
    "UPSERT",
    "UPSERTBATCH",
    "POP",
    "COUNT",
    "DUMP",
    "BUCKETS",
    "EXPORT",
    "IMPORT",
    "FLUSHC",
    "FLUSHB",
    "FLUSHO",
    "PING",
    "HELP",
    "QUIT",
];
pub static COMMANDS_MODE_CONTROL: [&str; 6] = ["TRIGGER", "INFO", "STATS", "PING", "HELP", "QUIT"];
pub static CONTROL_TRIGGER_ACTIONS: [&str; 3] = ["consolidate", "backup", "restore"];

// Wire-streamed dump pagination bounds; kept small enough that a page always fits within the \
//   ordinary command response buffer, unlike `UPSERTBATCH` which gets a dedicated bulk buffer.
const DUMP_LIMIT_DEFAULT: QuerySearchLimit = 1_000;
const DUMP_LIMIT_MAXIMUM: QuerySearchLimit = 10_000;

static MANUAL_MODE_SEARCH: LazyLock<HashMap<&str, Vec<&str>>> =
    LazyLock::new(|| HashMap::from_iter([("commands", COMMANDS_MODE_SEARCH.to_vec())]));
static MANUAL_MODE_INGEST: LazyLock<HashMap<&str, Vec<&str>>> =
    LazyLock::new(|| HashMap::from_iter([("commands", COMMANDS_MODE_INGEST.to_vec())]));
static MANUAL_MODE_CONTROL: LazyLock<HashMap<&str, Vec<&str>>> =
    LazyLock::new(|| HashMap::from_iter([("commands", COMMANDS_MODE_CONTROL.to_vec())]));

impl ChannelCommandResponse {
    pub fn to_args(&self) -> ChannelCommandResponseArgs {
        // Convert internal response to channel response arguments; this either gives 'RESPONSE' \
        //   or 'RESPONSE <value:1> <value:2> <..>' whether there are values or not.
        match *self {
            ChannelCommandResponse::Void => ("", None),
            ChannelCommandResponse::Ok => ("OK", None),
            ChannelCommandResponse::Pong => ("PONG", None),
            ChannelCommandResponse::Pending(ref id) => ("PENDING", Some(vec![id.to_owned()])),
            ChannelCommandResponse::Result(ref id) => ("RESULT", Some(vec![id.to_owned()])),
            ChannelCommandResponse::Event(ref query, ref id, ref payload) => (
                "EVENT",
                Some(vec![query.to_string(), id.to_owned(), payload.to_owned()]),
            ),
            ChannelCommandResponse::Ended(reason) => ("ENDED", Some(vec![reason.to_owned()])),
            ChannelCommandResponse::Err(ref reason) => ("ERR", Some(vec![reason.to_string()])),
        }
    }
}

impl ChannelCommandBase {
    pub fn dispatch_ping(mut parts: SplitWhitespace) -> ChannelResult {
        match parts.next() {
            None => Ok(vec![ChannelCommandResponse::Pong]),
            _ => Err(ChannelCommandError::InvalidFormat("PING")),
        }
    }

    pub fn dispatch_quit(mut parts: SplitWhitespace) -> ChannelResult {
        match parts.next() {
            None => Ok(vec![ChannelCommandResponse::Ended("quit")]),
            _ => Err(ChannelCommandError::InvalidFormat("QUIT")),
        }
    }

    pub fn generic_dispatch_help(
        mut parts: SplitWhitespace,
        manuals: &HashMap<&str, Vec<&str>>,
    ) -> ChannelResult {
        match (parts.next(), parts.next()) {
            (None, _) => {
                let manual_list = manuals.keys().map(|k| k.to_owned()).collect::<Vec<&str>>();

                Ok(vec![ChannelCommandResponse::Result(format!(
                    "manuals({})",
                    manual_list.join(", ")
                ))])
            }
            (Some(manual_key), next_part) => {
                if next_part.is_none() {
                    if let Some(manual_data) = manuals.get(manual_key) {
                        Ok(vec![ChannelCommandResponse::Result(format!(
                            "{}({})",
                            manual_key,
                            manual_data.join(", ")
                        ))])
                    } else {
                        Err(ChannelCommandError::NotFound)
                    }
                } else {
                    Err(ChannelCommandError::InvalidFormat("HELP [<manual>]?"))
                }
            }
        }
    }

    pub fn parse_text_parts(parts: &mut SplitWhitespace) -> Option<String> {
        // Parse text parts and nest them together
        let mut text_raw = String::new();

        for text_part in parts {
            if !text_raw.is_empty() {
                text_raw.push(' ');
            }

            text_raw.push_str(text_part);

            // End reached? (ie. got boundary character)
            let text_part_bytes = text_part.as_bytes();
            let text_part_bound = text_part_bytes.len();

            if text_raw.len() > 1
                && text_part_bytes[text_part_bound - 1] as char == TEXT_PART_BOUNDARY
            {
                // Count the total amount of escape characters before escape (check if escape \
                //   characters are also being escaped, or not)
                let mut count_escapes = 0;

                if text_part_bound > 1 {
                    for index in (0..text_part_bound - 1).rev() {
                        if text_part_bytes[index] as char != TEXT_PART_ESCAPE {
                            break;
                        }

                        count_escapes += 1
                    }
                }

                // Boundary is not escaped, we can stop there.
                if count_escapes == 0 || (count_escapes % 2 == 0) {
                    break;
                }
            }
        }

        // Ensure parsed text parts are valid
        let text_bytes = text_raw.as_bytes();
        let text_bytes_len = text_bytes.len();

        if text_raw.is_empty()
            || text_bytes_len < 2
            || text_bytes[0] as char != TEXT_PART_BOUNDARY
            || text_bytes[text_bytes_len - 1] as char != TEXT_PART_BOUNDARY
        {
            tracing::info!("could not properly parse text parts: {:?}", text_raw);

            None
        } else {
            tracing::debug!(
                "parsed text parts (still needs post-processing): {:?}",
                text_raw
            );

            // Return inner text (without boundary characters)
            match str::from_utf8(&text_bytes[1..text_bytes_len - 1]) {
                Ok(text_inner) => {
                    let text_inner_string = unescape(text_inner.trim());

                    tracing::debug!(
                        "parsed text parts (post-processed): {:?}",
                        text_inner_string
                    );

                    // Text must not be empty
                    if !text_inner_string.is_empty() {
                        Some(text_inner_string)
                    } else {
                        None
                    }
                }
                Err(err) => {
                    tracing::info!(
                        "could not type-cast post-processed text parts: {:?} because: {}",
                        text_raw,
                        err
                    );

                    None
                }
            }
        }
    }

    pub fn parse_next_meta_parts<'a>(
        parts: &'a mut SplitWhitespace,
    ) -> Option<MetaPartsResult<'a>> {
        if let Some(part) = parts.next() {
            // Parse meta (with format: 'KEY(VALUE)'; no '(' or ')' is allowed in KEY and VALUE)
            if !part.is_empty() {
                if let Some(index_open) = part.find(META_PART_GROUP_OPEN) {
                    let (key_bound_start, key_bound_end) = (0, index_open);
                    let (value_bound_start, value_bound_end) = (index_open + 1, part.len() - 1);

                    if part.as_bytes()[value_bound_end] as char == META_PART_GROUP_CLOSE {
                        let (key, value) = (
                            &part[key_bound_start..key_bound_end],
                            &part[value_bound_start..value_bound_end],
                        );

                        // Ensure final key and value do not contain reserved syntax characters
                        return if !key.contains(META_PART_GROUP_OPEN)
                            && !key.contains(META_PART_GROUP_CLOSE)
                            && !value.contains(META_PART_GROUP_OPEN)
                            && !value.contains(META_PART_GROUP_CLOSE)
                        {
                            tracing::debug!("parsed meta part as: {} = {}", key, value);

                            Some(Ok((key, value)))
                        } else {
                            tracing::info!(
                                "parsed meta part, but it contains reserved characters: {} = {}",
                                key,
                                value
                            );

                            Some(Err((key, value)))
                        };
                    }
                }
            }

            tracing::info!("could not parse meta part: {}", part);

            Some(Err(("?", part)))
        } else {
            None
        }
    }

    pub fn make_error_invalid_meta_key(meta_key: &str, meta_value: &str) -> ChannelCommandError {
        ChannelCommandError::InvalidMetaKey((meta_key.to_owned(), meta_value.to_owned()))
    }

    pub fn make_error_invalid_meta_value(meta_key: &str, meta_value: &str) -> ChannelCommandError {
        ChannelCommandError::InvalidMetaValue((meta_key.to_owned(), meta_value.to_owned()))
    }

    pub fn commit_ok_operation(query: Query, executor: &Executor) -> ChannelResult {
        match StoreOperationDispatch::dispatch(query, executor) {
            Ok(_) => Ok(vec![ChannelCommandResponse::Ok]),
            Err(()) => Err(ChannelCommandError::QueryError),
        }
    }

    pub fn commit_result_operation(query: Query, executor: &Executor) -> ChannelResult {
        match StoreOperationDispatch::dispatch(query, executor) {
            Ok(Some(result_inner)) => Ok(vec![ChannelCommandResponse::Result(result_inner)]),
            Ok(None) => Err(ChannelCommandError::InternalError),
            Err(()) => Err(ChannelCommandError::QueryError),
        }
    }

    pub fn commit_pending_operation(
        query_type: &'static str,
        query_id: &str,
        query: Query,
        executor: &Executor,
    ) -> ChannelResult {
        // Idea: this could be made asynchronous in the future, if there are some latency issues \
        //   on large Sonic deployments. The idea would be to have a number of worker threads for \
        //   the whole running daemon, and channel threads dispatching work to those threads. This \
        //   way Sonic can be up-scaled to N CPUs instead of 1 CPU per channel connection. Now on, \
        //   the only way to scale Sonic executors to multiple CPUs is opening multiple parallel \
        //   Sonic Channel connections and dispatching work evenly to each connection. It does not \
        //   prevent scaling Sonic vertically, but could be made simpler for the Sonic Channel \
        //   consumer via a worker thread pool.

        match StoreOperationDispatch::dispatch(query, executor) {
            Ok(results) if query_type == "QUERYDOCS" => {
                let documents: Vec<sonic::store::document::StoreDocument> =
                    serde_json::from_str(&results.unwrap_or_else(|| "[]".to_owned()))
                        .map_err(|_| ChannelCommandError::InternalError)?;
                let mut responses = Vec::with_capacity(documents.len() + 2);
                responses.push(ChannelCommandResponse::Pending(query_id.to_string()));
                for document in documents {
                    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
                        serde_json::to_vec(&document)
                            .map_err(|_| ChannelCommandError::InternalError)?,
                    );
                    if encoded.len() > 19_900 {
                        return Err(ChannelCommandError::PolicyReject(
                            "stored document exceeds channel response buffer",
                        ));
                    }
                    responses.push(ChannelCommandResponse::Event(
                        query_type,
                        query_id.to_string(),
                        encoded,
                    ));
                }
                responses.push(ChannelCommandResponse::Event(
                    query_type,
                    query_id.to_string(),
                    "DONE".to_owned(),
                ));
                Ok(responses)
            }
            Ok(results) => Ok(vec![
                ChannelCommandResponse::Pending(query_id.to_string()),
                ChannelCommandResponse::Event(
                    query_type,
                    query_id.to_string(),
                    results.unwrap_or_default(),
                ),
            ]),
            Err(()) => Err(ChannelCommandError::QueryError),
        }
    }

    pub fn generate_event_id() -> String {
        rand::rng()
            .sample_iter(&Alphanumeric)
            .take(EVENT_ID_SIZE)
            .map(|value| value as char)
            .collect()
    }
}

impl ChannelCommandSearch {
    pub fn dispatch_query(parts: SplitWhitespace, ctx: &ChannelMessageModeSearch) -> ChannelResult {
        Self::dispatch_query_inner(parts, ctx, false)
    }

    pub fn dispatch_query_documents(
        parts: SplitWhitespace,
        ctx: &ChannelMessageModeSearch,
    ) -> ChannelResult {
        Self::dispatch_query_inner(parts, ctx, true)
    }

    fn dispatch_query_inner(
        mut parts: SplitWhitespace,
        ctx: &ChannelMessageModeSearch,
        documents: bool,
    ) -> ChannelResult {
        match (
            parts.next(),
            parts.next(),
            ChannelCommandBase::parse_text_parts(&mut parts),
        ) {
            (Some(collection), Some(bucket), Some(text)) => {
                // Generate command identifier
                let event_id = ChannelCommandBase::generate_event_id();

                tracing::debug!(
                    "dispatching search query #{} on collection: {} and bucket: {}",
                    event_id,
                    collection,
                    bucket
                );

                // Define query parameters
                let (mut query_limit, mut query_offset, mut query_lang, mut from_ms, mut to_ms) =
                    (ctx.search_config.query_limit_default, 0, None, None, None);

                // Parse meta parts (meta comes after text; extract meta parts second)
                let mut last_meta_err = None;

                while let Some(meta_result) = ChannelCommandBase::parse_next_meta_parts(&mut parts)
                {
                    match Self::handle_query_meta(meta_result) {
                        Ok((Some(query_limit_parsed), None, None, None, None)) => {
                            query_limit = query_limit_parsed
                        }
                        Ok((None, Some(query_offset_parsed), None, None, None)) => {
                            query_offset = query_offset_parsed
                        }
                        Ok((None, None, Some(query_lang_parsed), None, None)) => {
                            query_lang = Some(query_lang_parsed)
                        }
                        Ok((None, None, None, Some(value), None)) => from_ms = Some(value),
                        Ok((None, None, None, None, Some(value))) => to_ms = Some(value),
                        Err(parse_err) => last_meta_err = Some(parse_err),
                        _ => {}
                    }
                }

                if let Some(err) = last_meta_err {
                    Err(err)
                } else if query_limit < 1 || query_limit > ctx.search_config.query_limit_maximum {
                    Err(ChannelCommandError::PolicyReject(
                        "LIMIT out of minimum/maximum bounds",
                    ))
                } else {
                    tracing::debug!(
                        "will search for #{} with text: {}, limit: {}, offset: {}, locale: <{:?}>",
                        event_id,
                        text,
                        query_limit,
                        query_offset,
                        query_lang
                    );

                    let time_range = match (from_ms, to_ms) {
                        (None, None) => None,
                        (from, to) => Some(
                            sonic::query::QueryTimeRange::new(
                                from.unwrap_or(0),
                                to.unwrap_or(u64::MAX),
                            )
                            .map_err(|()| {
                                ChannelCommandError::PolicyReject("FROM must be lower than TO")
                            })?,
                        ),
                    };
                    let query = Query::search_with_range(
                        &event_id,
                        collection,
                        bucket,
                        &text,
                        query_limit,
                        query_offset,
                        query_lang,
                        time_range,
                        *ctx.normalization_config,
                        *ctx.tokenization_config,
                        documents,
                    )
                    .map_err(|()| ChannelCommandError::QueryError)?;

                    // Commit 'search' query
                    ChannelCommandBase::commit_pending_operation(
                        if documents { "QUERYDOCS" } else { "QUERY" },
                        &event_id,
                        query,
                        ctx.executor,
                    )
                }
            }
            _ => Err(ChannelCommandError::InvalidFormat(
                "QUERY[DOCS] <collection> <bucket> \"<terms>\" [LIMIT(<count>)]? \
                 [OFFSET(<count>)]? [LANG(<locale>)]? [FROM(<unix_ms>)]? [TO(<unix_ms>)]?",
            )),
        }
    }

    pub fn dispatch_list(
        mut parts: SplitWhitespace,
        ctx: &ChannelMessageModeSearch,
    ) -> ChannelResult {
        match (parts.next(), parts.next()) {
            (Some(collection), Some(bucket)) => {
                // Generate command identifier
                let event_id = ChannelCommandBase::generate_event_id();

                tracing::debug!(
                    "dispatching search list #{} on collection: {} and bucket: {}",
                    event_id,
                    collection,
                    bucket
                );

                // Define list parameters
                let (mut list_limit, mut list_offset) = (ctx.search_config.list_limit_default, 0);

                // Parse meta parts (meta comes last; extract meta parts second)
                let mut last_meta_err = None;

                while let Some(meta_result) = ChannelCommandBase::parse_next_meta_parts(&mut parts)
                {
                    match Self::handle_list_meta(meta_result) {
                        Ok(metadata) => match metadata {
                            (Some(list_limit_parsed), None) => list_limit = list_limit_parsed,
                            (None, Some(list_offset_parsed)) => list_offset = list_offset_parsed,
                            _ => {}
                        },
                        Err(parse_err) => last_meta_err = Some(parse_err),
                    }
                }

                if let Some(err) = last_meta_err {
                    Err(err)
                } else if list_limit < 1 || list_limit > ctx.search_config.list_limit_maximum {
                    Err(ChannelCommandError::PolicyReject(
                        "LIMIT out of minimum/maximum bounds",
                    ))
                } else {
                    let query = Query::list(&event_id, collection, bucket, list_limit, list_offset)
                        .map_err(|()| ChannelCommandError::QueryError)?;

                    // Commit 'list' query
                    ChannelCommandBase::commit_pending_operation(
                        "LIST",
                        &event_id,
                        query,
                        ctx.executor,
                    )
                }
            }
            _ => Err(ChannelCommandError::InvalidFormat(
                "LIST <collection> <bucket> [LIMIT(<count>)]? [OFFSET(<count>)]?",
            )),
        }
    }

    pub fn dispatch_help(parts: SplitWhitespace, _ctx: &ChannelMessageModeSearch) -> ChannelResult {
        ChannelCommandBase::generic_dispatch_help(parts, &*MANUAL_MODE_SEARCH)
    }

    fn handle_query_meta(
        meta_result: MetaPartsResult,
    ) -> Result<QueryMetaData, ChannelCommandError> {
        match meta_result {
            Ok((meta_key, meta_value)) => {
                tracing::debug!("handle query meta: {} = {}", meta_key, meta_value);

                match meta_key {
                    "LIMIT" => {
                        // 'LIMIT(<count>)' where 0 <= <count> < 2^16
                        if let Ok(query_limit_parsed) = meta_value.parse::<QuerySearchLimit>() {
                            Ok((Some(query_limit_parsed), None, None, None, None))
                        } else {
                            Err(ChannelCommandBase::make_error_invalid_meta_value(
                                meta_key, meta_value,
                            ))
                        }
                    }
                    "OFFSET" => {
                        // 'OFFSET(<count>)' where 0 <= <count> < 2^32
                        if let Ok(query_offset_parsed) = meta_value.parse::<QuerySearchOffset>() {
                            Ok((None, Some(query_offset_parsed), None, None, None))
                        } else {
                            Err(ChannelCommandBase::make_error_invalid_meta_value(
                                meta_key, meta_value,
                            ))
                        }
                    }
                    "LANG" => {
                        // 'LANG(<locale>)' where <locale> ∈ ISO 639-3
                        if let Some(query_lang_parsed) = QueryGenericLang::from_value(meta_value) {
                            Ok((None, None, Some(query_lang_parsed), None, None))
                        } else {
                            Err(ChannelCommandBase::make_error_invalid_meta_value(
                                meta_key, meta_value,
                            ))
                        }
                    }
                    "FROM" => meta_value
                        .parse::<u64>()
                        .map(|value| (None, None, None, Some(value), None))
                        .map_err(|_| {
                            ChannelCommandBase::make_error_invalid_meta_value(meta_key, meta_value)
                        }),
                    "TO" => meta_value
                        .parse::<u64>()
                        .map(|value| (None, None, None, None, Some(value)))
                        .map_err(|_| {
                            ChannelCommandBase::make_error_invalid_meta_value(meta_key, meta_value)
                        }),
                    _ => Err(ChannelCommandBase::make_error_invalid_meta_key(
                        meta_key, meta_value,
                    )),
                }
            }
            Err(err) => Err(ChannelCommandBase::make_error_invalid_meta_key(
                err.0, err.1,
            )),
        }
    }

    fn handle_list_meta(meta_result: MetaPartsResult) -> Result<ListMetaData, ChannelCommandError> {
        match meta_result {
            Ok((meta_key, meta_value)) => {
                tracing::debug!("handle list meta: {} = {}", meta_key, meta_value);

                match meta_key {
                    "LIMIT" => {
                        // 'LIMIT(<count>)' where 0 <= <count> < 2^16
                        if let Ok(list_limit_parsed) = meta_value.parse::<QuerySearchLimit>() {
                            Ok((Some(list_limit_parsed), None))
                        } else {
                            Err(ChannelCommandBase::make_error_invalid_meta_value(
                                meta_key, meta_value,
                            ))
                        }
                    }
                    "OFFSET" => {
                        // 'OFFSET(<count>)' where 0 <= <count> < 2^32
                        if let Ok(list_offset_parsed) = meta_value.parse::<QuerySearchOffset>() {
                            Ok((None, Some(list_offset_parsed)))
                        } else {
                            Err(ChannelCommandBase::make_error_invalid_meta_value(
                                meta_key, meta_value,
                            ))
                        }
                    }
                    _ => Err(ChannelCommandBase::make_error_invalid_meta_key(
                        meta_key, meta_value,
                    )),
                }
            }
            Err(err) => Err(ChannelCommandBase::make_error_invalid_meta_key(
                err.0, err.1,
            )),
        }
    }
}

impl ChannelCommandIngest {
    pub fn dispatch_push(
        mut parts: SplitWhitespace,
        ctx: &ChannelMessageModeIngest,
    ) -> ChannelResult {
        match (
            parts.next(),
            parts.next(),
            parts.next(),
            ChannelCommandBase::parse_text_parts(&mut parts),
        ) {
            (Some(collection), Some(bucket), Some(object), Some(text)) => {
                tracing::debug!(
                    "dispatching ingest push in collection: {}, bucket: {} and object: {}",
                    collection,
                    bucket,
                    object
                );
                tracing::debug!("ingest push has text: {}", text);

                // Define push parameters
                let mut push_lang = None;

                // Parse meta parts (meta comes after text; extract meta parts second)
                let mut last_meta_err = None;

                while let Some(meta_result) = ChannelCommandBase::parse_next_meta_parts(&mut parts)
                {
                    match Self::handle_push_meta(meta_result) {
                        Ok(Some(push_lang_parsed)) => push_lang = Some(push_lang_parsed),
                        Err(parse_err) => last_meta_err = Some(parse_err),
                        _ => {}
                    }
                }

                if let Some(err) = last_meta_err {
                    Err(err)
                } else {
                    tracing::debug!(
                        "will push for text: {} with hinted locale: <{:?}>",
                        text,
                        push_lang
                    );

                    #[rustfmt::skip]
                    let query = Query::push(
                        collection, bucket, object, &text, push_lang,
                        *ctx.normalization_config,
                        *ctx.tokenization_config,
                    )
                    .map_err(|()| ChannelCommandError::QueryError)?;

                    // Commit 'push' query
                    ChannelCommandBase::commit_ok_operation(query, ctx.executor)
                }
            }
            _ => Err(ChannelCommandError::InvalidFormat(
                "PUSH <collection> <bucket> <object> \"<text>\" [LANG(<locale>)]?",
            )),
        }
    }

    pub fn dispatch_upsert(
        mut parts: SplitWhitespace,
        ctx: &ChannelMessageModeIngest,
    ) -> ChannelResult {
        let (Some(collection), Some(bucket), Some(object), Some(text)) = (
            parts.next(),
            parts.next(),
            parts.next(),
            ChannelCommandBase::parse_text_parts(&mut parts),
        ) else {
            return Err(ChannelCommandError::InvalidFormat(
                "UPSERT <collection> <bucket> <object> \"<text>\" TS(<unix_ms>) \
                 [META(<base64url-json>)]? [LANG(<locale>)]?",
            ));
        };
        let mut timestamp_ms = None;
        let mut metadata = serde_json::json!({});
        let mut lang = None;
        while let Some(meta_result) = ChannelCommandBase::parse_next_meta_parts(&mut parts) {
            let (key, value) = meta_result.map_err(|(key, value)| {
                ChannelCommandBase::make_error_invalid_meta_key(key, value)
            })?;
            match key {
                "TS" => {
                    timestamp_ms = Some(value.parse::<u64>().map_err(|_| {
                        ChannelCommandBase::make_error_invalid_meta_value(key, value)
                    })?);
                }
                "META" => {
                    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
                        .decode(value)
                        .map_err(|_| {
                            ChannelCommandBase::make_error_invalid_meta_value(key, value)
                        })?;
                    metadata = serde_json::from_slice(&decoded).map_err(|_| {
                        ChannelCommandBase::make_error_invalid_meta_value(key, value)
                    })?;
                    if !metadata.is_object() {
                        return Err(ChannelCommandBase::make_error_invalid_meta_value(
                            key, value,
                        ));
                    }
                }
                "LANG" => {
                    lang = Some(QueryGenericLang::from_value(value).ok_or_else(|| {
                        ChannelCommandBase::make_error_invalid_meta_value(key, value)
                    })?);
                }
                _ => {
                    return Err(ChannelCommandBase::make_error_invalid_meta_key(key, value));
                }
            }
        }
        let timestamp_ms = timestamp_ms.ok_or(ChannelCommandError::PolicyReject(
            "UPSERT requires an explicit TS",
        ))?;
        let query = Query::upsert(
            collection,
            bucket,
            object,
            &text,
            timestamp_ms,
            metadata,
            lang,
            *ctx.normalization_config,
            *ctx.tokenization_config,
        )
        .map_err(|()| ChannelCommandError::QueryError)?;
        ChannelCommandBase::commit_ok_operation(query, ctx.executor)
    }

    pub fn dispatch_upsert_batch(
        mut parts: SplitWhitespace,
        ctx: &ChannelMessageModeIngest,
    ) -> ChannelResult {
        let (Some(collection), Some(mode), Some(payload), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(ChannelCommandError::InvalidFormat(
                "UPSERTBATCH <collection> <fresh|upsert> <base64-zstd-ndjson>",
            ));
        };
        let fresh = match mode {
            "fresh" => true,
            "upsert" => false,
            _ => {
                return Err(ChannelCommandError::InvalidMetaValue((
                    "mode".to_owned(),
                    mode.to_owned(),
                )));
            }
        };
        let total_started = Instant::now();
        let base64_started = Instant::now();
        let compressed = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| ChannelCommandError::QueryError)?;
        let base64_decode = base64_started.elapsed();
        let decompress_started = Instant::now();
        let decoded = zstd::stream::decode_all(compressed.as_slice())
            .map_err(|_| ChannelCommandError::QueryError)?;
        let decompress = decompress_started.elapsed();
        let json_started = Instant::now();
        let mut records = Vec::new();
        for line in decoded.split(|byte| *byte == b'\n') {
            if !line.is_empty() {
                records.push(
                    serde_json::from_slice(line).map_err(|_| ChannelCommandError::QueryError)?,
                );
            }
        }
        let json_decode = json_started.elapsed();
        let profiling = profile::enabled();
        let (result, executor_profile) = if profiling {
            let (result, executor_profile) = ctx
                .executor
                .upsert_batch_profiled(collection, records, fresh)
                .map_err(|()| ChannelCommandError::QueryError)?;
            (result, Some(executor_profile))
        } else {
            let result = ctx
                .executor
                .upsert_batch(collection, records, fresh)
                .map_err(|()| ChannelCommandError::QueryError)?;
            (result, None)
        };
        let total = total_started.elapsed();
        if let Some(executor_profile) = executor_profile {
            profile::record(&IngestCommandProfile {
                timestamp_ms: IngestCommandProfile::timestamp_ms(),
                payload_bytes: payload.len(),
                compressed_bytes: compressed.len(),
                decoded_bytes: decoded.len(),
                command_total_us: total.as_micros(),
                base64_decode_us: base64_decode.as_micros(),
                decompress_us: decompress.as_micros(),
                json_decode_us: json_decode.as_micros(),
                executor: executor_profile,
            });
        }
        Ok(vec![ChannelCommandResponse::Result(format!(
            "{} {}",
            result.written, result.rejected
        ))])
    }

    pub fn dispatch_pop(
        mut parts: SplitWhitespace,
        ctx: &ChannelMessageModeIngest,
    ) -> ChannelResult {
        match (
            parts.next(),
            parts.next(),
            parts.next(),
            ChannelCommandBase::parse_text_parts(&mut parts),
            parts.next(),
        ) {
            (Some(collection), Some(bucket), Some(object), Some(text), None) => {
                tracing::debug!(
                    "dispatching ingest pop in collection: {}, bucket: {} and object: {}",
                    collection,
                    bucket,
                    object
                );
                tracing::debug!("ingest pop has text: {}", text);

                let query = Query::pop(
                    collection,
                    bucket,
                    object,
                    &text,
                    *ctx.normalization_config,
                    *ctx.tokenization_config,
                )
                .map_err(|()| ChannelCommandError::QueryError)?;

                // Make 'pop' query
                ChannelCommandBase::commit_result_operation(query, ctx.executor)
            }
            _ => Err(ChannelCommandError::InvalidFormat(
                "POP <collection> <bucket> <object> \"<text>\"",
            )),
        }
    }

    pub fn dispatch_dump(
        mut parts: SplitWhitespace,
        ctx: &ChannelMessageModeIngest,
    ) -> ChannelResult {
        match (parts.next(), parts.next()) {
            (Some(collection), Some(bucket)) => {
                let (mut limit, mut offset) = (DUMP_LIMIT_DEFAULT, 0);
                let mut last_meta_err = None;

                while let Some(meta_result) = ChannelCommandBase::parse_next_meta_parts(&mut parts)
                {
                    match ChannelCommandSearch::handle_list_meta(meta_result) {
                        Ok((Some(limit_parsed), None)) => limit = limit_parsed,
                        Ok((None, Some(offset_parsed))) => offset = offset_parsed,
                        Ok(_) => {}
                        Err(parse_err) => last_meta_err = Some(parse_err),
                    }
                }

                if let Some(err) = last_meta_err {
                    return Err(err);
                }
                if limit < 1 || limit > DUMP_LIMIT_MAXIMUM {
                    return Err(ChannelCommandError::PolicyReject(
                        "LIMIT out of minimum/maximum bounds",
                    ));
                }

                let records = ctx
                    .executor
                    .dump_bucket(collection, bucket, u64::from(offset), u64::from(limit))
                    .map_err(|()| ChannelCommandError::QueryError)?;

                let payload = Self::encode_dump_payload(&records)
                    .map_err(|()| ChannelCommandError::QueryError)?;

                Ok(vec![ChannelCommandResponse::Result(payload)])
            }
            _ => Err(ChannelCommandError::InvalidFormat(
                "DUMP <collection> <bucket> [LIMIT(<count>)]? [OFFSET(<count>)]?",
            )),
        }
    }

    pub fn dispatch_buckets(
        mut parts: SplitWhitespace,
        ctx: &ChannelMessageModeIngest,
    ) -> ChannelResult {
        match parts.next() {
            Some(collection) => {
                let (mut limit, mut offset) = (DUMP_LIMIT_DEFAULT, 0);
                let mut last_meta_err = None;

                while let Some(meta_result) = ChannelCommandBase::parse_next_meta_parts(&mut parts)
                {
                    match ChannelCommandSearch::handle_list_meta(meta_result) {
                        Ok((Some(limit_parsed), None)) => limit = limit_parsed,
                        Ok((None, Some(offset_parsed))) => offset = offset_parsed,
                        Ok(_) => {}
                        Err(parse_err) => last_meta_err = Some(parse_err),
                    }
                }

                if let Some(err) = last_meta_err {
                    return Err(err);
                }
                if limit < 1 || limit > DUMP_LIMIT_MAXIMUM {
                    return Err(ChannelCommandError::PolicyReject(
                        "LIMIT out of minimum/maximum bounds",
                    ));
                }

                let buckets = ctx
                    .executor
                    .list_buckets(collection, u64::from(offset), u64::from(limit))
                    .map_err(|()| ChannelCommandError::QueryError)?;

                Ok(vec![ChannelCommandResponse::Result(buckets.join(" "))])
            }
            None => Err(ChannelCommandError::InvalidFormat(
                "BUCKETS <collection> [LIMIT(<count>)]? [OFFSET(<count>)]?",
            )),
        }
    }

    fn encode_dump_payload(
        records: &[sonic::store::document::StoreDocumentRecord],
    ) -> Result<String, ()> {
        let mut ndjson = Vec::new();
        for record in records {
            serde_json::to_writer(&mut ndjson, record).map_err(|_| ())?;
            ndjson.push(b'\n');
        }
        let compressed = zstd::stream::encode_all(ndjson.as_slice(), 1).map_err(|_| ())?;
        Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(compressed))
    }

    pub fn dispatch_export(
        mut parts: SplitWhitespace,
        ctx: &ChannelMessageModeIngest,
    ) -> ChannelResult {
        let Some(collection) = parts.next() else {
            return Err(ChannelCommandError::InvalidFormat(
                "EXPORT <collection> [<bucket>]? <path>",
            ));
        };
        let (bucket, path) = match (parts.next(), parts.next(), parts.next()) {
            (Some(path), None, None) => (None, path),
            (Some(bucket), Some(path), None) => (Some(bucket), path),
            _ => {
                return Err(ChannelCommandError::InvalidFormat(
                    "EXPORT <collection> [<bucket>]? <path>",
                ));
            }
        };
        ctx.executor
            .export_documents(collection, bucket, Path::new(path))
            .map(|count| vec![ChannelCommandResponse::Result(count.to_string())])
            .map_err(|()| ChannelCommandError::QueryError)
    }

    pub fn dispatch_import(
        mut parts: SplitWhitespace,
        ctx: &ChannelMessageModeIngest,
    ) -> ChannelResult {
        let (Some(collection), Some(path), None) = (parts.next(), parts.next(), parts.next())
        else {
            return Err(ChannelCommandError::InvalidFormat(
                "IMPORT <collection> <path>",
            ));
        };
        ctx.executor
            .import_documents(collection, Path::new(path))
            .map(|count| vec![ChannelCommandResponse::Result(count.to_string())])
            .map_err(|()| ChannelCommandError::QueryError)
    }

    pub fn dispatch_count(
        mut parts: SplitWhitespace,
        ctx: &ChannelMessageModeIngest,
    ) -> ChannelResult {
        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(collection), bucket_part, object_part, None) => {
                tracing::debug!("dispatching ingest count in collection: {}", collection);

                let query = Query::count(collection, bucket_part, object_part)
                    .map_err(|()| ChannelCommandError::QueryError)?;

                // Make 'count' query
                ChannelCommandBase::commit_result_operation(query, ctx.executor)
            }
            _ => Err(ChannelCommandError::InvalidFormat(
                "COUNT <collection> [<bucket> [<object>]?]?",
            )),
        }
    }

    pub fn dispatch_flushc(
        mut parts: SplitWhitespace,
        ctx: &ChannelMessageModeIngest,
    ) -> ChannelResult {
        match (parts.next(), parts.next()) {
            (Some(collection), None) => {
                tracing::debug!(
                    "dispatching ingest flush collection in collection: {}",
                    collection
                );

                let query =
                    Query::flushc(collection).map_err(|()| ChannelCommandError::QueryError)?;

                // Make 'flushc' query
                ChannelCommandBase::commit_result_operation(query, ctx.executor)
            }
            _ => Err(ChannelCommandError::InvalidFormat("FLUSHC <collection>")),
        }
    }

    pub fn dispatch_flushb(
        mut parts: SplitWhitespace,
        ctx: &ChannelMessageModeIngest,
    ) -> ChannelResult {
        match (parts.next(), parts.next(), parts.next()) {
            (Some(collection), Some(bucket), None) => {
                tracing::debug!(
                    "dispatching ingest flush bucket in collection: {}, bucket: {}",
                    collection,
                    bucket
                );

                let query = Query::flushb(collection, bucket)
                    .map_err(|()| ChannelCommandError::QueryError)?;

                // Make 'flushb' query
                ChannelCommandBase::commit_result_operation(query, ctx.executor)
            }
            _ => Err(ChannelCommandError::InvalidFormat(
                "FLUSHB <collection> <bucket>",
            )),
        }
    }

    pub fn dispatch_flusho(
        mut parts: SplitWhitespace,
        ctx: &ChannelMessageModeIngest,
    ) -> ChannelResult {
        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(collection), Some(bucket), Some(object), None) => {
                tracing::debug!(
                    "dispatching ingest flush object in collection: {}, bucket: {}, object: {}",
                    collection,
                    bucket,
                    object
                );

                let query = Query::flusho(collection, bucket, object)
                    .map_err(|()| ChannelCommandError::QueryError)?;

                // Make 'flusho' query
                ChannelCommandBase::commit_result_operation(query, ctx.executor)
            }
            _ => Err(ChannelCommandError::InvalidFormat(
                "FLUSHO <collection> <bucket> <object>",
            )),
        }
    }

    pub fn dispatch_help(parts: SplitWhitespace, _ctx: &ChannelMessageModeIngest) -> ChannelResult {
        ChannelCommandBase::generic_dispatch_help(parts, &*MANUAL_MODE_INGEST)
    }

    fn handle_push_meta(
        meta_result: MetaPartsResult,
    ) -> Result<Option<QueryGenericLang>, ChannelCommandError> {
        match meta_result {
            Ok((meta_key, meta_value)) => {
                tracing::debug!("handle push meta: {} = {}", meta_key, meta_value);

                match meta_key {
                    "LANG" => {
                        // 'LANG(<locale>)' where <locale> ∈ ISO 639-3
                        if let Some(query_lang_parsed) = QueryGenericLang::from_value(meta_value) {
                            Ok(Some(query_lang_parsed))
                        } else {
                            Err(ChannelCommandBase::make_error_invalid_meta_value(
                                meta_key, meta_value,
                            ))
                        }
                    }
                    _ => Err(ChannelCommandBase::make_error_invalid_meta_key(
                        meta_key, meta_value,
                    )),
                }
            }
            Err(err) => Err(ChannelCommandBase::make_error_invalid_meta_key(
                err.0, err.1,
            )),
        }
    }
}

impl ChannelCommandControl {
    pub fn dispatch_stats(
        mut parts: SplitWhitespace,
        ctx: &ChannelMessageModeControl,
    ) -> ChannelResult {
        let (Some(collection), deep, None) = (parts.next(), parts.next(), parts.next()) else {
            return Err(ChannelCommandError::InvalidFormat(
                "STATS <collection> [DEEP]?",
            ));
        };
        let deep = match deep {
            None => false,
            Some("DEEP") | Some("deep") => true,
            Some(_) => {
                return Err(ChannelCommandError::InvalidFormat(
                    "STATS <collection> [DEEP]?",
                ));
            }
        };
        let stats = ctx
            .executor
            .stats(collection, deep)
            .map_err(|()| ChannelCommandError::QueryError)?;
        Ok(vec![ChannelCommandResponse::Result(
            serde_json::to_string(&stats).map_err(|_| ChannelCommandError::InternalError)?,
        )])
    }

    pub fn dispatch_trigger(
        mut parts: SplitWhitespace,
        ctx: &ChannelMessageModeControl,
    ) -> ChannelResult {
        match (parts.next(), parts.next(), parts.next()) {
            (None, _, _) => Ok(vec![ChannelCommandResponse::Result(format!(
                "actions({})",
                CONTROL_TRIGGER_ACTIONS.join(", ")
            ))]),
            (Some(action_key), data_part, last_part) => {
                let action_key_lower = action_key.to_lowercase();

                let Executor {
                    kv_pool, fst_pool, ..
                } = &ctx.executor;

                match action_key_lower.as_str() {
                    "consolidate" => {
                        if data_part.is_none() {
                            // Force a FST consolidate
                            fst_pool.consolidate(true);

                            Ok(vec![ChannelCommandResponse::Ok])
                        } else {
                            Err(ChannelCommandError::InvalidFormat("TRIGGER consolidate"))
                        }
                    }
                    "backup" => {
                        match (data_part, last_part) {
                            (Some(path), None) => {
                                // Proceed KV + FST backup
                                let path = Path::new(path);

                                if kv_pool.backup(&path.join(BACKUP_KV_PATH)).is_ok()
                                    && fst_pool.backup(&path.join(BACKUP_FST_PATH)).is_ok()
                                {
                                    Ok(vec![ChannelCommandResponse::Ok])
                                } else {
                                    Err(ChannelCommandError::InternalError)
                                }
                            }
                            _ => Err(ChannelCommandError::InvalidFormat("TRIGGER backup <path>")),
                        }
                    }
                    "restore" => {
                        match (data_part, last_part) {
                            (Some(path), None) => {
                                // Proceed KV + FST restore
                                let path = Path::new(path);

                                if kv_pool.restore(&path.join(BACKUP_KV_PATH)).is_ok()
                                    && fst_pool.restore(&path.join(BACKUP_FST_PATH)).is_ok()
                                {
                                    Ok(vec![ChannelCommandResponse::Ok])
                                } else {
                                    Err(ChannelCommandError::InternalError)
                                }
                            }
                            _ => Err(ChannelCommandError::InvalidFormat("TRIGGER restore <path>")),
                        }
                    }
                    _ => Err(ChannelCommandError::NotFound),
                }
            }
        }
    }

    pub fn dispatch_info(
        mut parts: SplitWhitespace,
        ctx: &ChannelMessageModeControl,
    ) -> ChannelResult {
        match parts.next() {
            None => {
                let statistics =
                    ChannelStatistics::gather(&ctx.executor.kv_pool, &ctx.executor.fst_pool);

                Ok(vec![ChannelCommandResponse::Result(format!(
                    "uptime({}) clients_connected({}) commands_total({}) \
                     command_latency_best({}) command_latency_worst({}) \
                     kv_open_count({}) fst_open_count({}) fst_consolidate_count({})",
                    statistics.uptime,
                    statistics.clients_connected,
                    statistics.commands_total,
                    statistics.command_latency_best,
                    statistics.command_latency_worst,
                    statistics.kv_open_count,
                    statistics.fst_open_count,
                    statistics.fst_consolidate_count
                ))])
            }
            _ => Err(ChannelCommandError::InvalidFormat("INFO")),
        }
    }

    pub fn dispatch_help(
        parts: SplitWhitespace,
        _ctx: &ChannelMessageModeControl,
    ) -> ChannelResult {
        ChannelCommandBase::generic_dispatch_help(parts, &*MANUAL_MODE_CONTROL)
    }
}

impl fmt::Display for ChannelCommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            ChannelCommandError::UnknownCommand => write!(f, "unknown_command"),
            ChannelCommandError::NotFound => write!(f, "not_found"),
            ChannelCommandError::QueryError => write!(f, "query_error"),
            ChannelCommandError::InternalError => write!(f, "internal_error"),
            ChannelCommandError::ShuttingDown => write!(f, "shutting_down"),
            ChannelCommandError::PolicyReject(reason) => write!(f, "policy_reject({})", reason),
            ChannelCommandError::InvalidFormat(format) => write!(f, "invalid_format({})", format),
            ChannelCommandError::InvalidMetaKey(data) => {
                write!(f, "invalid_meta_key({}[{}])", data.0, data.1)
            }
            ChannelCommandError::InvalidMetaValue(data) => {
                write!(f, "invalid_meta_value({}[{}])", data.0, data.1)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_matches_command_response_string() {
        assert_eq!(ChannelCommandResponse::Ok.to_args().0, "OK");
        assert_eq!(ChannelCommandResponse::Pong.to_args().0, "PONG");
        assert_eq!(ChannelCommandResponse::Ended("").to_args().0, "ENDED");
        assert_eq!(
            ChannelCommandResponse::Err(ChannelCommandError::UnknownCommand)
                .to_args()
                .0,
            "ERR"
        );
    }
}
