// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use hashbrown::HashMap;
use rand::RngExt;
use rand::distr::Alphanumeric;
use sonic::lexer::preprocessor::Preprocessor;
use sonic::lexer::to_rework::TokenLexerMode;
use sonic::store::StoreItemBuilder;
use std::fmt;
use std::path::Path;
use std::str::{self, SplitWhitespace};
use std::sync::LazyLock;
use std::vec::Vec;

use sonic::Executor;
use sonic::executor::{
    ListMetaData, PushOptions, QueryGenericLang, QueryOptions, QuerySearchLimit, QuerySearchOffset,
};

use super::format::unescape;
use super::message::{
    ChannelMessageModeControl, ChannelMessageModeIngest, ChannelMessageModeSearch,
};
use super::statistics::ChannelStatistics;
use crate::util::itertools::Itertools as _;

#[derive(PartialEq)]
pub enum ChannelCommandError {
    UnknownCommand,
    Unsupported,
    NotFound,
    QueryError,
    InternalError,
    ShuttingDown,
    PolicyReject(&'static str),
    InvalidFormat(&'static str),
    InvalidArgument(String),
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

pub struct QueryMetaData {
    pub limit: QuerySearchLimit,
    pub offset: QuerySearchOffset,
    pub lang: Option<QueryGenericLang>,
    #[cfg(feature = "experimental-api")]
    pub snippets: Option<sonic::executor::QuerySnippetsConfig>,
}

pub const EVENT_ID_SIZE: usize = 8;

const TEXT_PART_BOUNDARY: char = '"';
const TEXT_PART_ESCAPE: char = '\\';
const META_PART_GROUP_OPEN: char = '(';
const META_PART_GROUP_CLOSE: char = ')';

static BACKUP_KV_PATH: &str = "kv";
static BACKUP_FST_PATH: &str = "fst";
static BACKUP_OBJECT_PATH: &str = "object";

pub static COMMANDS_MODE_SEARCH: &[&str] = &["QUERY", "SUGGEST", "LIST", "PING", "HELP", "QUIT"];
pub static COMMANDS_MODE_INGEST: &[&str] = &[
    "PUSH", "POP", "COUNT", "COUNTC", "COUNTB", "COUNTO", "FLUSHC", "FLUSHB", "FLUSHO", "PING",
    "HELP", "QUIT",
];
#[rustfmt::skip]
pub static COMMANDS_MODE_CONTROL: &[&str] = &[
    "TRIGGER", "INFO",
    #[cfg(feature = "experimental-api")] "INFO_RESET",
    #[cfg(feature = "experimental-api")] "CONFIG",
    "PING", "HELP", "QUIT"
];
#[rustfmt::skip]
pub static CONTROL_TRIGGER_ACTIONS: &[&str] = &[
    "consolidate", "backup", "restore",
    #[cfg(feature = "experimental-api")] "flush",
    #[cfg(feature = "experimental-api")] "compact",
];

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
                            tracing::debug!("parsed meta part as: {key:?} = {value:?}");

                            Some(Ok((key, value)))
                        } else {
                            tracing::info!(
                                "parsed meta part, but it contains reserved characters: {key:?} = {value:?}"
                            );

                            Some(Err((key, value)))
                        };
                    }
                } else {
                    let (key, value) = (part, "");
                    tracing::debug!("parsed meta part as: {key:?} = {value:?}");
                    return Some(Ok((key, value)));
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

    pub fn commit_ok_operation(operation: impl FnOnce() -> Result<(), ()>) -> ChannelResult {
        match operation() {
            Ok(()) => Ok(vec![ChannelCommandResponse::Ok]),
            Err(()) => Err(ChannelCommandError::QueryError),
        }
    }

    pub fn commit_result_operation(
        operation: impl FnOnce() -> Result<Option<String>, ()>,
    ) -> ChannelResult {
        match operation() {
            Ok(Some(result_inner)) => Ok(vec![ChannelCommandResponse::Result(result_inner)]),
            Ok(None) => Err(ChannelCommandError::InternalError),
            Err(()) => Err(ChannelCommandError::QueryError),
        }
    }

    pub fn commit_pending_operation(
        query_type: &'static str,
        query_id: &str,
        operation: impl FnOnce() -> Result<Option<String>, ()>,
    ) -> ChannelResult {
        // Idea: this could be made asynchronous in the future, if there are some latency issues \
        //   on large Sonic deployments. The idea would be to have a number of worker threads for \
        //   the whole running daemon, and channel threads dispatching work to those threads. This \
        //   way Sonic can be up-scaled to N CPUs instead of 1 CPU per channel connection. Now on, \
        //   the only way to scale Sonic executors to multiple CPUs is opening multiple parallel \
        //   Sonic Channel connections and dispatching work evenly to each connection. It does not \
        //   prevent scaling Sonic vertically, but could be made simpler for the Sonic Channel \
        //   consumer via a worker thread pool.

        match operation() {
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
    pub fn dispatch_query(
        mut parts: SplitWhitespace,
        ctx: &ChannelMessageModeSearch,
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
                let mut query_params = QueryMetaData {
                    limit: ctx.search_config.query_limit_default,
                    offset: 0,
                    lang: None,
                    #[cfg(feature = "experimental-api")]
                    snippets: None,
                };

                // Parse meta parts (meta comes after text; extract meta parts second)
                let mut last_meta_err = None;

                while let Some(meta_result) = ChannelCommandBase::parse_next_meta_parts(&mut parts)
                {
                    #[cfg(feature = "experimental-api")]
                    if matches!(meta_result, Ok(("WITH", _))) {
                        // NOTE: Just ignoring `WITH` is far from being the best way
                        //   to handle this, but the parsing code is so convoluted
                        //   I(@RemiBardon) don’t want to deal with it now.
                        continue;
                    }

                    if let Err(parse_err) = Self::handle_query_meta(meta_result, &mut query_params)
                    {
                        last_meta_err = Some(parse_err);
                    }
                }

                let QueryMetaData {
                    limit,
                    offset,
                    lang,
                    #[cfg(feature = "experimental-api")]
                    snippets,
                } = query_params;

                if let Some(err) = last_meta_err {
                    Err(err)
                } else if limit < 1 || limit > ctx.search_config.query_limit_maximum {
                    Err(ChannelCommandError::PolicyReject(
                        "LIMIT out of minimum/maximum bounds",
                    ))
                } else {
                    tracing::debug!(
                        "will search for #{event_id} with text: {text:?}, limit: {limit}, offset: {offset}, locale: <{lang:?}>"
                    );

                    let (collection, bucket) = StoreItemBuilder::from_depth_2(collection, bucket)
                        .map_err(|error| {
                        ChannelCommandError::InvalidArgument(format!("{error:?}"))
                    })?;

                    // Commit 'search' query
                    ChannelCommandBase::commit_pending_operation("QUERY", &event_id, move || {
                        let should_cleanup =
                            TokenLexerMode::from_query_lang(&lang).should_cleanup();
                        let preprocessor = Preprocessor::new(
                            *ctx.tokenization_config,
                            *ctx.normalization_config,
                            ctx.stopwords_config.clone(),
                            should_cleanup,
                            should_cleanup,
                        );
                        let lang = lang.and_then(QueryGenericLang::into_lang_opt);
                        let text_lexed = preprocessor.preprocess(&text, lang);

                        let query_options = QueryOptions { limit, offset };

                        #[cfg(feature = "experimental-api")]
                        if let Some(snippets) = snippets {
                            return ctx
                                .executor
                                .query_with_snippets(
                                    collection,
                                    bucket,
                                    text_lexed,
                                    query_options,
                                    lang,
                                    &snippets,
                                )
                                .and_then(|results| match serde_json::to_string(&results) {
                                    Ok(string) => Ok(Some(string)),
                                    Err(error) => {
                                        tracing::error!(
                                            "Failed serializing QUERY snippets: {error}"
                                        );
                                        Err(())
                                    }
                                });
                        }

                        ctx.executor
                            .query(collection, bucket, text_lexed, query_options)
                            .map(|mut results| {
                                if results.len() == 0 {
                                    None
                                } else {
                                    Some(results.join(" "))
                                }
                            })
                    })
                }
            }
            _ => Err(ChannelCommandError::InvalidFormat(
                "QUERY <collection> <bucket> \"<terms>\" [LIMIT(<count>)]? [OFFSET(<count>)]? \
                 [LANG(<locale>)]?",
            )),
        }
    }

    pub fn dispatch_suggest(
        mut parts: SplitWhitespace,
        ctx: &ChannelMessageModeSearch,
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
                    "dispatching search suggest #{} on collection: {} and bucket: {}",
                    event_id,
                    collection,
                    bucket
                );

                // Define suggest parameters
                let mut suggest_limit = ctx.search_config.suggest_limit_default;

                // Parse meta parts (meta comes after text; extract meta parts second)
                let mut last_meta_err = None;

                while let Some(meta_result) = ChannelCommandBase::parse_next_meta_parts(&mut parts)
                {
                    match Self::handle_suggest_meta(meta_result) {
                        Ok(Some(suggest_limit_parsed)) => suggest_limit = suggest_limit_parsed,
                        Err(parse_err) => last_meta_err = Some(parse_err),
                        _ => {}
                    }
                }

                if let Some(err) = last_meta_err {
                    Err(err)
                } else if suggest_limit < 1
                    || suggest_limit > ctx.search_config.suggest_limit_maximum
                {
                    Err(ChannelCommandError::PolicyReject(
                        "LIMIT out of minimum/maximum bounds",
                    ))
                } else {
                    tracing::debug!(
                        "will suggest for #{} with text: {}, limit: {}",
                        event_id,
                        text,
                        suggest_limit
                    );

                    let (collection, bucket) = StoreItemBuilder::from_depth_2(collection, bucket)
                        .map_err(|error| {
                        ChannelCommandError::InvalidArgument(format!("{error:?}"))
                    })?;

                    // Commit 'suggest' query
                    ChannelCommandBase::commit_pending_operation("SUGGEST", &event_id, move || {
                        let preprocessor = Preprocessor::new(
                            *ctx.tokenization_config,
                            *ctx.normalization_config,
                            ctx.stopwords_config.clone(),
                            false,
                            false,
                        );
                        let text_lexed = preprocessor.preprocess(&text, None);

                        ctx.executor
                            .suggest(collection, bucket, text_lexed, suggest_limit)
                            .map(|results| results.map(|mut results| results.join(" ")))
                    })
                }
            }
            _ => Err(ChannelCommandError::InvalidFormat(
                "SUGGEST <collection> <bucket> \"<word>\" [LIMIT(<count>)]?",
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
                    let (collection, bucket) = StoreItemBuilder::from_depth_2(collection, bucket)
                        .map_err(|error| {
                        ChannelCommandError::InvalidArgument(format!("{error:?}"))
                    })?;

                    // Commit 'list' query
                    ChannelCommandBase::commit_pending_operation("LIST", &event_id, move || {
                        ctx.executor
                            .list(collection, bucket, list_limit, list_offset)
                            .map(|results| results.join(" "))
                            .map(Some)
                    })
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
        meta: &mut QueryMetaData,
    ) -> Result<(), ChannelCommandError> {
        match meta_result {
            Ok((meta_key, meta_value)) => {
                tracing::debug!("handle query meta: {} = {}", meta_key, meta_value);

                match meta_key {
                    "LIMIT" => {
                        // 'LIMIT(<count>)' where 0 <= <count> < 2^16
                        if let Ok(query_limit_parsed) = meta_value.parse::<QuerySearchLimit>() {
                            meta.limit = query_limit_parsed;
                            Ok(())
                        } else {
                            Err(ChannelCommandBase::make_error_invalid_meta_value(
                                meta_key, meta_value,
                            ))
                        }
                    }
                    "OFFSET" => {
                        // 'OFFSET(<count>)' where 0 <= <count> < 2^32
                        if let Ok(query_offset_parsed) = meta_value.parse::<QuerySearchOffset>() {
                            meta.offset = query_offset_parsed;
                            Ok(())
                        } else {
                            Err(ChannelCommandBase::make_error_invalid_meta_value(
                                meta_key, meta_value,
                            ))
                        }
                    }
                    "LANG" => {
                        // 'LANG(<locale>)' where <locale> ∈ ISO 639-3
                        if let Some(query_lang_parsed) = QueryGenericLang::from_value(meta_value) {
                            meta.lang = Some(query_lang_parsed);
                            Ok(())
                        } else {
                            Err(ChannelCommandBase::make_error_invalid_meta_value(
                                meta_key, meta_value,
                            ))
                        }
                    }
                    #[cfg(feature = "experimental-api")]
                    "SNIPPETS" => {
                        // 'SNIPPETS[(<window_count>[, <window_size>[, <max_window_padding>]?]?)]?'
                        let mut config = sonic::executor::QuerySnippetsConfig {
                            window_count: 1,
                            window_size: 42,
                            max_window_padding: 41,
                        };

                        if !meta_value.is_empty() {
                            let mut args = meta_value.splitn(3, ",");

                            let error = |error| {
                                tracing::error!("Command argument parsing error: {error}");
                                Err(ChannelCommandBase::make_error_invalid_meta_value(
                                    meta_key, meta_value,
                                ))
                            };

                            if let Some(arg) = args.next() {
                                config.window_count = arg.parse().or_else(error)?;
                            }
                            if let Some(arg) = args.next() {
                                config.window_size = arg.parse().or_else(error)?;
                                config.max_window_padding = config.window_size.saturating_sub(1);
                            }
                            if let Some(arg) = args.next() {
                                config.max_window_padding = arg.parse().or_else(error)?;
                            }
                        }

                        meta.snippets = Some(config);
                        Ok(())
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

    fn handle_suggest_meta(
        meta_result: MetaPartsResult,
    ) -> Result<Option<QuerySearchLimit>, ChannelCommandError> {
        match meta_result {
            Ok((meta_key, meta_value)) => {
                tracing::debug!("handle suggest meta: {} = {}", meta_key, meta_value);

                match meta_key {
                    "LIMIT" => {
                        // 'LIMIT(<count>)' where 0 <= <count> < 2^16
                        if let Ok(suggest_limit_parsed) = meta_value.parse::<QuerySearchLimit>() {
                            Ok(Some(suggest_limit_parsed))
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
                let mut push_params = (None, PushOptions::default());

                // Parse meta parts (meta comes after text; extract meta parts second)
                let mut last_meta_err = None;

                while let Some(meta_result) = ChannelCommandBase::parse_next_meta_parts(&mut parts)
                {
                    if let Err(parse_err) = Self::handle_push_meta(meta_result, &mut push_params) {
                        last_meta_err = Some(parse_err);
                    }
                }

                let (push_lang, push_options) = push_params;

                if let Some(err) = last_meta_err {
                    Err(err)
                } else {
                    tracing::debug!(
                        "will push for text: {} with hinted locale: <{:?}>",
                        text,
                        push_lang
                    );

                    let (collection, bucket, oid) = StoreItemBuilder::from_depth_3(
                        collection, bucket, object,
                    )
                    .map_err(|error| ChannelCommandError::InvalidArgument(format!("{error:?}")))?;

                    // Commit 'push' query
                    ChannelCommandBase::commit_ok_operation(move || {
                        let should_cleanup =
                            TokenLexerMode::from_query_lang(&push_lang).should_cleanup();
                        let preprocessor = Preprocessor::new(
                            *ctx.tokenization_config,
                            *ctx.normalization_config,
                            ctx.stopwords_config.clone(),
                            should_cleanup,
                            should_cleanup,
                        );
                        let text_lexed = preprocessor
                            .preprocess(&text, push_lang.and_then(QueryGenericLang::into_lang_opt));

                        ctx.executor
                            .push(collection, bucket, oid, text_lexed, push_options)
                    })
                }
            }
            #[cfg(feature = "experimental-api")]
            _ => Err(ChannelCommandError::InvalidFormat(
                "PUSH <collection> <bucket> <object> \"<text>\" [LANG(<locale>)]? [NEW]? [INCOMPLETE]? [CAPACITY(<len>)]?",
            )),
            #[cfg(not(feature = "experimental-api"))]
            _ => Err(ChannelCommandError::InvalidFormat(
                "PUSH <collection> <bucket> <object> \"<text>\" [LANG(<locale>)]?",
            )),
        }
    }

    pub fn dispatch_pop(_parts: SplitWhitespace, _ctx: &ChannelMessageModeIngest) -> ChannelResult {
        Err(ChannelCommandError::Unsupported)
    }

    pub fn dispatch_count(
        mut parts: SplitWhitespace,
        ctx: &ChannelMessageModeIngest,
    ) -> ChannelResult {
        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(collection), Some(bucket), Some(object), None) => {
                tracing::debug!(
                    collection,
                    bucket,
                    object,
                    "dispatching ingest count in object"
                );

                let (collection, bucket, oid) = StoreItemBuilder::from_depth_3(
                    collection, bucket, object,
                )
                .map_err(|error| ChannelCommandError::InvalidArgument(format!("{error:?}")))?;

                // Make 'count' query
                ChannelCommandBase::commit_result_operation(move || {
                    ctx.executor
                        .counto(collection, bucket, oid)
                        .map(|count| Some(count.to_string()))
                })
            }
            (Some(collection), Some(bucket), None, None) => {
                tracing::debug!(collection, bucket, "dispatching ingest count in bucket");

                let (collection, bucket) = StoreItemBuilder::from_depth_2(collection, bucket)
                    .map_err(|error| ChannelCommandError::InvalidArgument(format!("{error:?}")))?;

                // Make 'count' query
                ChannelCommandBase::commit_result_operation(move || {
                    ctx.executor
                        .legacy_countb(collection, bucket)
                        .map(|count| Some(count.to_string()))
                })
            }
            (Some(collection), None, None, None) => {
                tracing::debug!(collection, "dispatching ingest count in collection");

                let collection = StoreItemBuilder::from_depth_1(collection)
                    .map_err(|error| ChannelCommandError::InvalidArgument(format!("{error:?}")))?;

                // Make 'count' query
                ChannelCommandBase::commit_result_operation(move || {
                    ctx.executor
                        .countc(collection)
                        .map(|count| Some(count.to_string()))
                })
            }
            _ => Err(ChannelCommandError::InvalidFormat(
                "COUNT <collection> [<bucket> [<object>]?]?",
            )),
        }
    }

    pub fn dispatch_countc(
        mut parts: SplitWhitespace,
        ctx: &ChannelMessageModeIngest,
    ) -> ChannelResult {
        match (parts.next(), parts.next()) {
            (Some(collection), None) => {
                tracing::debug!(collection, "dispatching ingest count in collection");

                let collection = StoreItemBuilder::from_depth_1(collection)
                    .map_err(|error| ChannelCommandError::InvalidArgument(format!("{error:?}")))?;

                // Make 'count' query
                ChannelCommandBase::commit_result_operation(move || {
                    ctx.executor
                        .countc(collection)
                        .map(|count| Some(count.to_string()))
                })
            }
            _ => Err(ChannelCommandError::InvalidFormat("COUNTC <collection>")),
        }
    }

    pub fn dispatch_countb(
        mut parts: SplitWhitespace,
        ctx: &ChannelMessageModeIngest,
    ) -> ChannelResult {
        match (parts.next(), parts.next(), parts.next()) {
            (Some(collection), Some(bucket), None) => {
                tracing::debug!(collection, bucket, "dispatching ingest count in bucket");

                let (collection, bucket) = StoreItemBuilder::from_depth_2(collection, bucket)
                    .map_err(|error| ChannelCommandError::InvalidArgument(format!("{error:?}")))?;

                // Make 'count' query
                ChannelCommandBase::commit_result_operation(move || {
                    ctx.executor
                        .countb(collection, bucket)
                        .map(|count| Some(count.to_string()))
                })
            }
            _ => Err(ChannelCommandError::InvalidFormat(
                "COUNTB <collection> <bucket>",
            )),
        }
    }

    pub fn dispatch_counto(
        mut parts: SplitWhitespace,
        ctx: &ChannelMessageModeIngest,
    ) -> ChannelResult {
        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(collection), Some(bucket), Some(object), None) => {
                tracing::debug!(
                    collection,
                    bucket,
                    object,
                    "dispatching ingest count in object"
                );

                let (collection, bucket, oid) = StoreItemBuilder::from_depth_3(
                    collection, bucket, object,
                )
                .map_err(|error| ChannelCommandError::InvalidArgument(format!("{error:?}")))?;

                // Make 'count' query
                ChannelCommandBase::commit_result_operation(move || {
                    ctx.executor
                        .counto(collection, bucket, oid)
                        .map(|count| Some(count.to_string()))
                })
            }
            _ => Err(ChannelCommandError::InvalidFormat(
                "COUNTO <collection> <bucket> <object>",
            )),
        }
    }

    pub fn dispatch_flushc(
        mut parts: SplitWhitespace,
        ctx: &ChannelMessageModeIngest,
    ) -> ChannelResult {
        match (parts.next(), parts.next()) {
            (Some(collection), None) => {
                tracing::debug!(collection, "dispatching ingest flush collection");

                let collection = StoreItemBuilder::from_depth_1(collection)
                    .map_err(|error| ChannelCommandError::InvalidArgument(format!("{error:?}")))?;

                // Make 'flushc' query
                ChannelCommandBase::commit_result_operation(move || {
                    ctx.executor
                        .flushc(collection)
                        .map(|count| Some(count.to_string()))
                })
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
                tracing::debug!(collection, bucket, "dispatching ingest flush bucket");

                let (collection, bucket) = StoreItemBuilder::from_depth_2(collection, bucket)
                    .map_err(|error| ChannelCommandError::InvalidArgument(format!("{error:?}")))?;

                // Make 'flushb' query
                ChannelCommandBase::commit_result_operation(move || {
                    ctx.executor
                        .flushb(collection, bucket)
                        .map(|count| Some(count.to_string()))
                })
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
                    collection,
                    bucket,
                    object,
                    "dispatching ingest flush object"
                );

                let (collection, bucket, oid) = StoreItemBuilder::from_depth_3(
                    collection, bucket, object,
                )
                .map_err(|error| ChannelCommandError::InvalidArgument(format!("{error:?}")))?;

                // Make 'flusho' query
                ChannelCommandBase::commit_result_operation(move || {
                    ctx.executor
                        .flusho(collection, bucket, oid)
                        .map(|count| Some(count.to_string()))
                })
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
        options: &mut (Option<QueryGenericLang>, PushOptions),
    ) -> Result<(), ChannelCommandError> {
        match meta_result {
            Ok((meta_key, meta_value)) => {
                tracing::debug!("handle push meta: {} = {}", meta_key, meta_value);

                match meta_key {
                    "LANG" => {
                        // 'LANG(<locale>)' where <locale> ∈ ISO 639-3
                        if let Some(query_lang_parsed) = QueryGenericLang::from_value(meta_value) {
                            options.0 = Some(query_lang_parsed);
                            Ok(())
                        } else {
                            Err(ChannelCommandBase::make_error_invalid_meta_value(
                                meta_key, meta_value,
                            ))
                        }
                    }
                    #[cfg(feature = "experimental-api")]
                    "NEW" => {
                        if meta_value.is_empty() {
                            options.1.assume_new = true;
                            Ok(())
                        } else {
                            Err(ChannelCommandBase::make_error_invalid_meta_value(
                                meta_key, meta_value,
                            ))
                        }
                    }
                    #[cfg(feature = "experimental-api")]
                    "INCOMPLETE" => {
                        if meta_value.is_empty() {
                            options.1.is_incomplete = true;
                            Ok(())
                        } else {
                            Err(ChannelCommandBase::make_error_invalid_meta_value(
                                meta_key, meta_value,
                            ))
                        }
                    }
                    #[cfg(feature = "experimental-api")]
                    "CAPACITY" => match meta_value.parse::<usize>() {
                        Ok(capacity) => {
                            options.1.capacity = Some(capacity);
                            Ok(())
                        }
                        Err(_error) => Err(ChannelCommandBase::make_error_invalid_meta_value(
                            meta_key, meta_value,
                        )),
                    },
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
                    kv_pool,
                    fst_pool,
                    object_store_pool,
                    ..
                } = &ctx.executor;

                match action_key_lower.as_str() {
                    "consolidate" => {
                        if data_part.is_none() {
                            // Force a FST consolidate
                            fst_pool.consolidate(true, |_| true);

                            Ok(vec![ChannelCommandResponse::Ok])
                        } else {
                            Err(ChannelCommandError::InvalidFormat("TRIGGER consolidate"))
                        }
                    }
                    #[cfg(feature = "experimental-api")]
                    "flush" => {
                        if data_part.is_none() {
                            // Force a KV flush
                            kv_pool.flush(true, |_| true);
                            object_store_pool.flush(true, |_| true);

                            Ok(vec![ChannelCommandResponse::Ok])
                        } else {
                            Err(ChannelCommandError::InvalidFormat("TRIGGER flush"))
                        }
                    }
                    #[cfg(feature = "experimental-api")]
                    "compact" => {
                        let collections = if let Some(data_part) = data_part {
                            let mut collections = Vec::new();

                            for collection_name in data_part.split_ascii_whitespace() {
                                let part = StoreItemBuilder::from_depth_1(collection_name)
                                    .map_err(|error| {
                                        ChannelCommandError::InvalidArgument(format!("{error:?}"))
                                    })?;
                                collections.push(part);
                            }

                            Some(collections)
                        } else {
                            None
                        };

                        let mut errors = Vec::new();

                        macro_rules! extend_errors {
                            ($store_kind:literal, $res:expr) => {
                                errors.extend(
                                    $res.err()
                                        .into_iter()
                                        .flatten()
                                        .map(|(store_id, error)| ($store_kind, store_id, error)),
                                );
                            };
                        }

                        // Force a KV store compaction
                        extend_errors!("KV", kv_pool.compact(collections.as_deref()));

                        // Force an Object store compaction
                        extend_errors!("Object", object_store_pool.compact(collections.as_deref()));

                        if errors.is_empty() {
                            Ok(vec![ChannelCommandResponse::Ok])
                        } else {
                            for (store_kind, store_id, error) in errors {
                                tracing::error!(
                                    "Error while compacting {store_kind} store {store_id}: {error}"
                                );
                            }

                            Err(ChannelCommandError::InternalError)
                        }
                    }
                    "backup" => {
                        match (data_part, last_part) {
                            (Some(path), None) => {
                                // Proceed KV + FST backup
                                let path = Path::new(path);

                                if kv_pool.backup(&path.join(BACKUP_KV_PATH)).is_ok()
                                    && fst_pool.backup(&path.join(BACKUP_FST_PATH)).is_ok()
                                    && object_store_pool
                                        .backup(&path.join(BACKUP_OBJECT_PATH))
                                        .is_ok()
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
                                    && object_store_pool
                                        .restore(&path.join(BACKUP_OBJECT_PATH))
                                        .is_ok()
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
                let ChannelStatistics {
                    uptime,
                    clients_connected,
                    commands_new,
                    commands_total,
                    command_latency_best,
                    command_latency_worst,
                    kv_open_count,
                    fst_open_count,
                    fst_consolidate_count,
                } = ChannelStatistics::gather(&ctx.executor.kv_pool, &ctx.executor.fst_pool);

                Ok(vec![ChannelCommandResponse::Result(format!(
                    "uptime({uptime}) clients_connected({clients_connected}) \
                    commands_new({commands_new}) commands_total({commands_total}) \
                    command_latency_best({command_latency_best}) command_latency_worst({command_latency_worst}) \
                    kv_open_count({kv_open_count}) \
                    fst_open_count({fst_open_count}) fst_consolidate_count({fst_consolidate_count})"
                ))])
            }
            _ => Err(ChannelCommandError::InvalidFormat("INFO")),
        }
    }

    #[cfg(feature = "experimental-api")]
    pub fn dispatch_info_reset(
        mut parts: SplitWhitespace,
        ctx: &ChannelMessageModeControl,
    ) -> ChannelResult {
        match parts.next() {
            None => {
                ChannelStatistics::reset(&ctx.executor);

                Ok(vec![ChannelCommandResponse::Ok])
            }
            _ => Err(ChannelCommandError::InvalidFormat("INFO")),
        }
    }

    #[cfg(feature = "experimental-api")]
    pub fn dispatch_config(
        mut parts: SplitWhitespace,
        ctx: &ChannelMessageModeControl,
    ) -> ChannelResult {
        const FORMAT: &str = "CONFIG <collection> (SET <key=value>...|RESET [key]...)";

        let Some(collection) = parts.next() else {
            return Err(ChannelCommandError::InvalidFormat(FORMAT));
        };
        let collection = StoreItemBuilder::from_depth_1(collection)
            .map_err(|error| ChannelCommandError::InvalidArgument(format!("{error:?}")))?;

        tracing::debug!(?collection, "dispatching config command");

        match parts.next() {
            Some("SET") => config_set(parts, ctx, collection),
            Some("RESET") => config_reset(parts, ctx, collection),
            Some(action) => {
                tracing::warn!("Unknown CONFIG action: {action}");
                Err(ChannelCommandError::NotFound)
            }
            None => Err(ChannelCommandError::InvalidFormat(FORMAT)),
        }
    }

    pub fn dispatch_help(
        parts: SplitWhitespace,
        _ctx: &ChannelMessageModeControl,
    ) -> ChannelResult {
        ChannelCommandBase::generic_dispatch_help(parts, &*MANUAL_MODE_CONTROL)
    }
}

#[cfg(feature = "experimental-api")]
#[derive(Debug, Default, Clone, Copy)]
struct DynamicConfig {
    bulk_load_preset_options: Option<BulkLoadPresetOptions>,
    sonic: DynamicConfigSonic,
    rocksdb: DynamicConfigRocksDb,
}

#[cfg(feature = "experimental-api")]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct DynamicConfigSonic {
    disable_janitor_tasks: Option<bool>,
    disable_fst_consolidate_task: Option<bool>,
    disable_kv_flush_task: Option<bool>,
}

#[cfg(feature = "experimental-api")]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct DynamicConfigRocksDb {
    disable_auto_compactions: Option<bool>,
    unordered_write: Option<bool>,
    memtable: Option<RocksDbMemtable>,
}

#[cfg(feature = "experimental-api")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RocksDbMemtable {
    Default,
    Vector,
}

#[cfg(feature = "experimental-api")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum BulkLoadConstraint {
    DiskSpace,
    CpuTime,
}

#[cfg(feature = "experimental-api")]
const MEGA: u8 = 6;
#[cfg(feature = "experimental-api")]
const TERA: u8 = 12;

#[cfg(feature = "experimental-api")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(serde::Deserialize)]
struct BulkLoadPresetOptions {
    cores: u16,

    #[serde(with = "crate::util::serde::byte_amount")]
    l2_cache_size: crate::util::serde::byte_amount::ByteAmount<MEGA, u32>,

    fd_limit: u64,

    #[serde(with = "crate::util::serde::byte_amount")]
    total_bytes: crate::util::serde::byte_amount::ByteAmount<TERA, u64>,

    #[serde(default, with = "sonic::util::serde::none_string_as_none")]
    constraint: Option<BulkLoadConstraint>,

    #[serde(default)]
    write_ahead_log: Option<bool>,
}

#[cfg(feature = "experimental-api")]
impl sonic::executor::DynamicConfig for DynamicConfig {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn are_janitor_tasks_enabled(&self) -> bool {
        !matches!(self.sonic.disable_janitor_tasks, Some(true))
    }

    fn is_fst_consolidate_task_enabled(&self) -> bool {
        !matches!(self.sonic.disable_fst_consolidate_task, Some(true))
    }

    fn is_kv_flush_task_enabled(&self) -> bool {
        !matches!(self.sonic.disable_kv_flush_task, Some(true))
    }

    fn update_kv_store_options(
        &self,
        options: &mut sonic::rocksdb::Options,
        config: &sonic::config::RocksDbDatabaseConfig,
    ) {
        use crate::util::u32_to_usize;

        let Self {
            bulk_load_preset_options,
            rocksdb,
            sonic: _,
        } = self;

        if let Some(BulkLoadPresetOptions {
            cores,
            l2_cache_size,
            fd_limit,
            total_bytes,
            constraint,
            write_ahead_log: _,
        }) = bulk_load_preset_options
        {
            options.set_disable_auto_compactions(true);
            options.set_unordered_write(true);

            let write_buffer_size: u32 = l2_cache_size.value() / 2;
            options.set_write_buffer_size(u32_to_usize(write_buffer_size));

            let max_l0_files = (fd_limit - 48)
                // Add room for error. Ideally this would be ~1.5 but we’re
                // working with integers so 2 will do.
                .saturating_div(2)
                .max(1);
            options.set_max_open_files(20);
            let min_write_buffer_number_to_merge: i32 = i32::try_from(
                ((total_bytes.value() / max_l0_files) / u64::from(write_buffer_size))
                    .saturating_mul(sonic::store::kv::AVERAGE_MERGE_RATIO as u64),
            )
            .unwrap_or(i32::MAX);
            options.set_min_write_buffer_number_to_merge(min_write_buffer_number_to_merge);
            options.set_max_write_buffer_number(min_write_buffer_number_to_merge.saturating_mul(2));
            options.increase_parallelism(i32::from(*cores));
            options.set_max_background_jobs(i32::from(*cores) + config.max_flushes.unwrap_or(1));
            options.set_max_subcompactions(u32::from(*cores));

            let compression_type = sonic::store::rocksdb::apply_compress_fallback(
                &config.compression_type,
                &config.compress,
            );
            let default_compression =
                compression_type.unwrap_or(sonic::rocksdb::DBCompressionType::None);

            match constraint {
                Some(BulkLoadConstraint::DiskSpace) => {
                    options.set_compression_per_level(&[
                        sonic::rocksdb::DBCompressionType::Zstd,
                        default_compression,
                        default_compression,
                        default_compression,
                        default_compression,
                    ]);
                }
                Some(BulkLoadConstraint::CpuTime) => {
                    options.set_min_level_to_compress(1);
                }
                None => {
                    options.set_compression_per_level(&[
                        sonic::rocksdb::DBCompressionType::Lz4,
                        default_compression,
                        default_compression,
                        default_compression,
                        default_compression,
                    ]);
                }
            }
        }

        let DynamicConfigRocksDb {
            disable_auto_compactions,
            unordered_write,
            memtable,
        } = rocksdb;

        macro_rules! if_some {
            ($opts:ident.$set_fn:ident($value:expr)) => {
                if let Some(value) = $value {
                    $opts.$set_fn(*value);
                }
            };
        }

        if_some!(options.set_disable_auto_compactions(disable_auto_compactions));

        if_some!(options.set_unordered_write(unordered_write));

        match memtable {
            Some(RocksDbMemtable::Vector) => {
                // Use the vector-based memtable instead of the default skiplist.
                options.set_memtable_factory(sonic::rocksdb::MemtableFactory::Vector);

                // Vector memtables don't support concurrent inserts, so this must be false.
                options.set_allow_concurrent_memtable_write(false);
            }
            None | Some(RocksDbMemtable::Default) => {}
        }
    }

    fn update_kv_store_cf_options(
        &self,
        _cf_name: &'static str,
        options: &mut sonic::rocksdb::Options,
        config: &sonic::config::RocksDbDatabaseConfig,
    ) {
        self.update_kv_store_options(options, config);
    }

    fn update_object_store_options(
        &self,
        options: &mut sonic::rocksdb::Options,
        config: &sonic::config::RocksDbDatabaseConfig,
    ) {
        let Self {
            bulk_load_preset_options,
            sonic: _,
            rocksdb,
        } = self;

        if let Some(BulkLoadPresetOptions {
            cores,
            l2_cache_size: _,
            fd_limit: _,
            total_bytes: _,
            constraint: _,
            write_ahead_log: _,
        }) = bulk_load_preset_options
        {
            options.set_disable_auto_compactions(true);
            options.set_unordered_write(true);

            options.set_max_open_files(20);
            options.set_write_buffer_size(64 * 1024 * 1024); // 64MiB
            options.set_min_write_buffer_number_to_merge(4); // L0 SST ⪅ 256MiB
            options.set_max_write_buffer_number(8); // Do not stop writes while a flush is in progress (max 512MiB RAM usage).
            options.increase_parallelism(i32::from(*cores));
            options.set_max_background_jobs(i32::from(*cores) + config.max_flushes.unwrap_or(1));
            options.set_max_subcompactions(u32::from(*cores));
        }

        let DynamicConfigRocksDb {
            disable_auto_compactions,
            unordered_write,
            memtable,
        } = rocksdb;

        macro_rules! if_some {
            ($opts:ident.$set_fn:ident($value:expr)) => {
                if let Some(value) = $value {
                    $opts.$set_fn(*value);
                }
            };
        }

        if_some!(options.set_disable_auto_compactions(disable_auto_compactions));

        if_some!(options.set_unordered_write(unordered_write));

        match memtable {
            Some(RocksDbMemtable::Vector) => {
                // Use the vector-based memtable instead of the default skiplist.
                options.set_memtable_factory(sonic::rocksdb::MemtableFactory::Vector);

                // Vector memtables don't support concurrent inserts, so this must be false.
                options.set_allow_concurrent_memtable_write(false);
            }
            None | Some(RocksDbMemtable::Default) => {}
        }
    }

    fn update_write_options_builder(
        &self,
        builder: &mut sonic::store::rocksdb::RocksDbWriteOptionsBuilder,
    ) {
        if let Some(opts) = self.bulk_load_preset_options {
            if let Some(write_ahead_log) = opts.write_ahead_log {
                builder.is_wal_enabled = write_ahead_log;
            }
        }
    }
}

#[cfg(feature = "experimental-api")]
// TODO: Use proper parsing to add support for quoted values (with spaces).
fn config_set(
    parts: SplitWhitespace,
    ctx: &ChannelMessageModeControl,
    collection: sonic::store::StoreItemPart,
) -> ChannelResult {
    use std::sync::Arc;

    use sonic::store::CollectionHash;

    fn parse_bool(str: &str) -> Option<bool> {
        match str {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    }

    // TODO: Maybe replace this by a proper deserializer? (If so, make sure to support duplicate keys)
    fn merge(
        config: &mut DynamicConfig,
        key: &str,
        value: &str,
    ) -> Result<(), ChannelCommandError> {
        let invalid_meta_value =
            || ChannelCommandError::InvalidMetaValue((key.to_owned(), value.to_owned()));

        // WARN: Think about updating `CONFIG RESET` when adding new keys here!
        match key {
            "preset.bulk_load" => {
                let options =
                    serde_json::from_str::<BulkLoadPresetOptions>(value).map_err(|error| {
                        tracing::warn!("{error}");
                        invalid_meta_value()
                    })?;

                config.bulk_load_preset_options = Some(options);

                // FIX: Do not disable janitor tasks during a bulk ingestion,
                //   to prevent memory footprint to keep increasing. Dropping
                //   the stores only cause cheap flush-to-disk operations, so
                //   it’s okay running those while ingesting.
                // config.sonic.disable_janitor_tasks = Some(true);

                // However, keep individual consolidate/flush tasks disabled
                // to only run it if the store has been inactive for a while
                // (unlikely to be reused).
                config.sonic.disable_fst_consolidate_task = Some(true);
                config.sonic.disable_kv_flush_task = Some(true);

                // TODO: increase `inactive_after` so we don’t janitor as often.
            }
            "sonic.disable_janitor_tasks" => {
                config.sonic.disable_janitor_tasks =
                    Some(parse_bool(value).ok_or_else(invalid_meta_value)?);
            }
            "sonic.disable_fst_consolidate_task" => {
                config.sonic.disable_fst_consolidate_task =
                    Some(parse_bool(value).ok_or_else(invalid_meta_value)?);
            }
            "sonic.disable_kv_flush_task" => {
                config.sonic.disable_kv_flush_task =
                    Some(parse_bool(value).ok_or_else(invalid_meta_value)?);
            }
            "sonic.disable_all_tasks" => {
                let val = Some(parse_bool(value).ok_or_else(invalid_meta_value)?);
                config.sonic.disable_janitor_tasks = val;
                config.sonic.disable_fst_consolidate_task = val;
                config.sonic.disable_kv_flush_task = val;
            }
            "rocksdb.disable_auto_compactions" => {
                config.rocksdb.disable_auto_compactions =
                    Some(parse_bool(value).ok_or_else(invalid_meta_value)?);
            }
            "rocksdb.unordered_write" => {
                config.rocksdb.unordered_write =
                    Some(parse_bool(value).ok_or_else(invalid_meta_value)?);
            }
            "rocksdb.memtable" => match value {
                "default" => config.rocksdb.memtable = Some(RocksDbMemtable::Default),
                "vector" => config.rocksdb.memtable = Some(RocksDbMemtable::Vector),
                _ => return Err(invalid_meta_value()),
            },
            key => {
                tracing::warn!("Unknown dynamic configuration key: {key:?}");
                return Err(ChannelCommandError::NotFound);
            }
        }

        Ok(())
    }

    let mut config = (ctx.executor.dynamic_conf_store.read())
        .get(&CollectionHash::from_part(collection))
        .map_or_else(DynamicConfig::default, |boxed| {
            // SAFETY: Sonic never writes any other value; we know it’s our type.
            *boxed.as_any().downcast_ref::<DynamicConfig>().unwrap()
        });

    for part in parts {
        match part.split_once(|c| matches!(c, '=' | '(')) {
            Some((key, mut value)) => {
                // This isn’t very correct, but for now it’s a good way to check
                // if the syntax is `key=value` or `fn(value)`. No value should
                // end with `)` (for string values it would be `"`) so it’s a
                // good discriminator.
                if value.ends_with(')') {
                    value = &value[..value.len() - 1];
                }

                merge(&mut config, key, value)?
            }

            // If key is passed alone, consider it a boolean.
            None => merge(&mut config, part, "true")?,
        }
    }

    // NOTE: `set_dynamic_conf` updates `dynamic_conf_store` on success.
    match ctx.executor.set_dynamic_conf(collection, Arc::new(config)) {
        Ok(()) => Ok(vec![ChannelCommandResponse::Ok]),
        Err(error) => {
            tracing::error!("Error setting dynamic config: {error}");
            Err(ChannelCommandError::InternalError)
        }
    }
}

#[cfg(feature = "experimental-api")]
fn config_reset(
    parts: SplitWhitespace,
    ctx: &ChannelMessageModeControl,
    collection: sonic::store::StoreItemPart,
) -> ChannelResult {
    use std::sync::Arc;

    let mut parts = parts.peekable();
    let new_conf = if parts.peek().is_some() {
        use sonic::store::CollectionHash;

        let mut new_conf = (ctx.executor.dynamic_conf_store.read())
            .get(&CollectionHash::from_part(collection))
            .map_or_else(DynamicConfig::default, |boxed| {
                // SAFETY: Sonic never writes any other value; we know it’s our type.
                *boxed.as_any().downcast_ref::<DynamicConfig>().unwrap()
            });

        macro_rules! match_reset {
            ($key:ident => $($val:literal => $block:block,)* $($($path:ident).+),+ $(,)?) => {
                match $key {
                    $(stringify!($($path).+) => new_conf.$($path).+ = None,)+
                    $($val => $block,)*
                    key => {
                        tracing::warn!("Unknown dynamic configuration key: {key:?}");
                        return Err(ChannelCommandError::NotFound);
                    }
                }
            };
        }

        for key in parts {
            match_reset!(key =>
                "preset.bulk_load" => {
                    new_conf.bulk_load_preset_options = None;
                    new_conf.sonic.disable_janitor_tasks = None;
                    new_conf.sonic.disable_fst_consolidate_task = None;
                    new_conf.sonic.disable_kv_flush_task = None;
                },
                "sonic.disable_all_tasks" => {
                    new_conf.sonic.disable_janitor_tasks = None;
                    new_conf.sonic.disable_fst_consolidate_task = None;
                    new_conf.sonic.disable_kv_flush_task = None;
                },
                sonic.disable_janitor_tasks,
                sonic.disable_fst_consolidate_task,
                sonic.disable_kv_flush_task,
                rocksdb.disable_auto_compactions,
                rocksdb.unordered_write,
                rocksdb.memtable,
            );
        }

        new_conf
    } else {
        // If no argument was provided, reset the whole configuration.
        DynamicConfig::default()
    };

    match ctx
        .executor
        .set_dynamic_conf(collection, Arc::new(new_conf))
    {
        Ok(()) => Ok(vec![ChannelCommandResponse::Ok]),
        Err(error) => {
            tracing::error!("Error resetting dynamic config: {error:?}");
            Err(ChannelCommandError::InternalError)
        }
    }
}

impl fmt::Display for ChannelCommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            ChannelCommandError::UnknownCommand => write!(f, "unknown_command"),
            ChannelCommandError::Unsupported => write!(f, "unsupported"),
            ChannelCommandError::NotFound => write!(f, "not_found"),
            ChannelCommandError::QueryError => write!(f, "query_error"),
            ChannelCommandError::InternalError => write!(f, "internal_error"),
            ChannelCommandError::ShuttingDown => write!(f, "shutting_down"),
            ChannelCommandError::PolicyReject(reason) => write!(f, "policy_reject({})", reason),
            ChannelCommandError::InvalidFormat(format) => write!(f, "invalid_format({})", format),
            ChannelCommandError::InvalidArgument(reason) => {
                write!(f, "invalid_argument({})", reason)
            }
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
