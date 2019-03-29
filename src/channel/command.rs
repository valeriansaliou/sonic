// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use hashbrown::HashMap;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use std::str::{self, SplitWhitespace};
use std::vec::Vec;

use super::format::unescape;
use crate::query::builder::{QueryBuilder, QueryBuilderResult};
use crate::query::types::{QueryIngestLang, QuerySearchLang, QuerySearchLimit, QuerySearchOffset};
use crate::store::fst::StoreFSTPool;
use crate::store::operation::StoreOperationDispatch;
use crate::APP_CONF;

#[derive(PartialEq)]
pub enum ChannelCommandError {
    UnknownCommand,
    NotFound,
    QueryError,
    InternalError,
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

type ChannelResult = Result<Vec<ChannelCommandResponse>, ChannelCommandError>;
type MetaPartsResult<'a> = Result<(&'a str, &'a str), (&'a str, &'a str)>;

pub const EVENT_ID_SIZE: usize = 8;

const TEXT_PART_BOUNDARY: char = '"';
const TEXT_PART_ESCAPE: char = '\\';
const META_PART_GROUP_OPEN: char = '(';
const META_PART_GROUP_CLOSE: char = ')';

lazy_static! {
    pub static ref COMMANDS_MODE_SEARCH: Vec<&'static str> =
        vec!["QUERY", "SUGGEST", "PING", "HELP", "QUIT"];
    pub static ref COMMANDS_MODE_INGEST: Vec<&'static str> =
        vec!["PUSH", "POP", "COUNT", "FLUSHC", "FLUSHB", "FLUSHO", "PING", "HELP", "QUIT"];
    pub static ref COMMANDS_MODE_CONTROL: Vec<&'static str> =
        vec!["TRIGGER", "PING", "HELP", "QUIT"];
    pub static ref CONTROL_TRIGGER_ACTIONS: Vec<&'static str> = vec!["consolidate"];
    static ref MANUAL_MODE_SEARCH: HashMap<&'static str, &'static Vec<&'static str>> =
        [("commands", &*COMMANDS_MODE_SEARCH)]
            .iter()
            .cloned()
            .collect();
    static ref MANUAL_MODE_INGEST: HashMap<&'static str, &'static Vec<&'static str>> =
        [("commands", &*COMMANDS_MODE_INGEST)]
            .iter()
            .cloned()
            .collect();
    static ref MANUAL_MODE_CONTROL: HashMap<&'static str, &'static Vec<&'static str>> =
        [("commands", &*COMMANDS_MODE_CONTROL)]
            .iter()
            .cloned()
            .collect();
}

impl ChannelCommandError {
    pub fn to_string(&self) -> String {
        match *self {
            ChannelCommandError::UnknownCommand => String::from("unknown_command"),
            ChannelCommandError::NotFound => String::from("not_found"),
            ChannelCommandError::QueryError => String::from("query_error"),
            ChannelCommandError::InternalError => String::from("internal_error"),
            ChannelCommandError::PolicyReject(reason) => format!("policy_reject({})", reason),
            ChannelCommandError::InvalidFormat(format) => format!("invalid_format({})", format),
            ChannelCommandError::InvalidMetaKey(ref data) => {
                format!("invalid_meta_key({}[{}])", data.0, data.1)
            }
            ChannelCommandError::InvalidMetaValue(ref data) => {
                format!("invalid_meta_value({}[{}])", data.0, data.1)
            }
        }
    }
}

impl ChannelCommandResponse {
    pub fn to_args(&self) -> (&'static str, Option<Vec<String>>) {
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
        manuals: &HashMap<&str, &Vec<&str>>,
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
                if next_part.is_none() == true {
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

    pub fn parse_text_parts<'a>(parts: &'a mut SplitWhitespace) -> Option<String> {
        // Parse text parts and nest them together
        let mut text_raw = String::new();

        while let Some(text_part) = parts.next() {
            if text_raw.is_empty() == false {
                text_raw.push_str(" ");
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

        if text_raw.is_empty() == true
            || text_bytes_len < 2
            || text_bytes[0] as char != TEXT_PART_BOUNDARY
            || text_bytes[text_bytes_len - 1] as char != TEXT_PART_BOUNDARY
        {
            info!("could not properly parse text parts: {}", text_raw);

            None
        } else {
            debug!(
                "parsed text parts (still needs post-processing): {}",
                text_raw
            );

            // Return inner text (without boundary characters)
            match str::from_utf8(&text_bytes[1..text_bytes_len - 1]) {
                Ok(text_inner) => {
                    let text_inner_string = unescape(text_inner.trim());

                    debug!("parsed text parts (post-processed): {}", text_inner_string);

                    // Text must not be empty
                    if text_inner_string.is_empty() == false {
                        Some(text_inner_string)
                    } else {
                        None
                    }
                }
                Err(err) => {
                    info!(
                        "could not type-cast post-processed text parts: {} because: {}",
                        text_raw, err
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
            if part.is_empty() == false {
                if let Some(index_open) = part.find(META_PART_GROUP_OPEN) {
                    let (key_bound_start, key_bound_end) = (0, index_open);
                    let (value_bound_start, value_bound_end) = (index_open + 1, part.len() - 1);

                    if part.as_bytes()[value_bound_end] as char == META_PART_GROUP_CLOSE {
                        let (key, value) = (
                            &part[key_bound_start..key_bound_end],
                            &part[value_bound_start..value_bound_end],
                        );

                        // Ensure final key and value do not contain reserved syntax characters
                        return if key.contains(META_PART_GROUP_OPEN) == false
                            && key.contains(META_PART_GROUP_CLOSE) == false
                            && value.contains(META_PART_GROUP_OPEN) == false
                            && value.contains(META_PART_GROUP_CLOSE) == false
                        {
                            debug!("parsed meta part as: {} = {}", key, value);

                            Some(Ok((key, value)))
                        } else {
                            info!(
                                "parsed meta part, but it contains reserved characters: {} = {}",
                                key, value
                            );

                            Some(Err((key, value)))
                        };
                    }
                }
            }

            info!("could not parse meta part: {}", part);

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

    pub fn commit_ok_operation<'a>(query_builder: QueryBuilderResult<'a>) -> ChannelResult {
        query_builder
            .and_then(|query| StoreOperationDispatch::dispatch(query))
            .and_then(|_| Ok(vec![ChannelCommandResponse::Ok]))
            .or(Err(ChannelCommandError::QueryError))
    }

    pub fn commit_result_operation<'a>(query_builder: QueryBuilderResult<'a>) -> ChannelResult {
        query_builder
            .and_then(|query| StoreOperationDispatch::dispatch(query))
            .or(Err(ChannelCommandError::QueryError))
            .and_then(|result| {
                if let Some(result_inner) = result {
                    Ok(vec![ChannelCommandResponse::Result(result_inner)])
                } else {
                    Err(ChannelCommandError::InternalError)
                }
            })
    }

    pub fn commit_pending_operation<'a>(
        query_type: &'static str,
        query_id: &str,
        query_builder: QueryBuilderResult<'a>,
    ) -> ChannelResult {
        // Idea: this could be made asynchronous in the future, if there are some latency issues \
        //   on large Sonic deployments. The idea would be to have a number of worker threads for \
        //   the whole running daemon, and channel threads dispatching work to those threads. This \
        //   way Sonic can be up-scaled to N CPUs instead of 1 CPU per channel connection. Now on, \
        //   the only way to scale Sonic executors to multiple CPUs is opening multiple parallel \
        //   Sonic Channel connections and dispatching work evenly to each connection. It does not \
        //   prevent scaling Sonic vertically, but could be made simpler for the Sonic Channel \
        //   consumer via a worker thread pool.

        query_builder
            .and_then(|query| StoreOperationDispatch::dispatch(query))
            .and_then(|results| {
                Ok(vec![
                    ChannelCommandResponse::Pending(query_id.to_string()),
                    ChannelCommandResponse::Event(
                        query_type,
                        query_id.to_string(),
                        results.unwrap_or(String::new()),
                    ),
                ])
            })
            .or(Err(ChannelCommandError::QueryError))
    }

    pub fn generate_event_id() -> String {
        thread_rng()
            .sample_iter(&Alphanumeric)
            .take(EVENT_ID_SIZE)
            .collect()
    }
}

impl ChannelCommandSearch {
    pub fn dispatch_query(mut parts: SplitWhitespace) -> ChannelResult {
        match (
            parts.next(),
            parts.next(),
            ChannelCommandBase::parse_text_parts(&mut parts),
        ) {
            (Some(collection), Some(bucket), Some(text)) => {
                // Generate command identifier
                let event_id = ChannelCommandBase::generate_event_id();

                debug!(
                    "dispatching search query #{} on collection: {} and bucket: {}",
                    event_id, collection, bucket
                );

                // Define query parameters
                let (mut query_limit, mut query_offset, mut query_lang) =
                    (APP_CONF.channel.search.query_limit_default, 0, None);

                // Parse meta parts (meta comes after text; extract meta parts second)
                let mut last_meta_err = None;

                while let Some(meta_result) = ChannelCommandBase::parse_next_meta_parts(&mut parts)
                {
                    match Self::handle_query_meta(meta_result) {
                        Ok((Some(query_limit_parsed), None, None)) => {
                            query_limit = query_limit_parsed
                        }
                        Ok((None, Some(query_offset_parsed), None)) => {
                            query_offset = query_offset_parsed
                        }
                        Ok((None, None, Some(query_lang_parsed))) => {
                            query_lang = Some(query_lang_parsed)
                        }
                        Err(parse_err) => last_meta_err = Some(parse_err),
                        _ => {}
                    }
                }

                if let Some(err) = last_meta_err {
                    Err(err)
                } else if query_limit < 1
                    || query_limit > APP_CONF.channel.search.query_limit_maximum
                {
                    Err(ChannelCommandError::PolicyReject(
                        "LIMIT out of minimum/maximum bounds",
                    ))
                } else {
                    debug!(
                        "will search for #{} with text: {}, limit: {}, offset: {}, locale: <{:?}>",
                        event_id, text, query_limit, query_offset, query_lang
                    );

                    // Commit 'search' query
                    ChannelCommandBase::commit_pending_operation(
                        "QUERY",
                        &event_id,
                        QueryBuilder::search(
                            &event_id,
                            collection,
                            bucket,
                            &text,
                            query_limit,
                            query_offset,
                            query_lang,
                        ),
                    )
                }
            }
            _ => Err(ChannelCommandError::InvalidFormat(
                "QUERY <collection> <bucket> \"<terms>\" [LIMIT(<count>)]? [OFFSET(<count>)]? \
                 [LANG(<locale>)]?",
            )),
        }
    }

    pub fn dispatch_suggest(mut parts: SplitWhitespace) -> ChannelResult {
        match (
            parts.next(),
            parts.next(),
            ChannelCommandBase::parse_text_parts(&mut parts),
        ) {
            (Some(collection), Some(bucket), Some(text)) => {
                // Generate command identifier
                let event_id = ChannelCommandBase::generate_event_id();

                debug!(
                    "dispatching search suggest #{} on collection: {} and bucket: {}",
                    event_id, collection, bucket
                );

                // Define suggest parameters
                let mut suggest_limit = APP_CONF.channel.search.suggest_limit_default;

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
                    || suggest_limit > APP_CONF.channel.search.suggest_limit_maximum
                {
                    Err(ChannelCommandError::PolicyReject(
                        "LIMIT out of minimum/maximum bounds",
                    ))
                } else {
                    debug!(
                        "will suggest for #{} with text: {}, limit: {}",
                        event_id, text, suggest_limit
                    );

                    // Commit 'suggest' query
                    ChannelCommandBase::commit_pending_operation(
                        "SUGGEST",
                        &event_id,
                        QueryBuilder::suggest(&event_id, collection, bucket, &text, suggest_limit),
                    )
                }
            }
            _ => Err(ChannelCommandError::InvalidFormat(
                "SUGGEST <collection> <bucket> \"<word>\" [LIMIT(<count>)]?",
            )),
        }
    }

    pub fn dispatch_help(parts: SplitWhitespace) -> ChannelResult {
        ChannelCommandBase::generic_dispatch_help(parts, &*MANUAL_MODE_SEARCH)
    }

    fn handle_query_meta(
        meta_result: MetaPartsResult,
    ) -> Result<
        (
            Option<QuerySearchLimit>,
            Option<QuerySearchOffset>,
            Option<QuerySearchLang>,
        ),
        ChannelCommandError,
    > {
        match meta_result {
            Ok((meta_key, meta_value)) => {
                debug!("handle query meta: {} = {}", meta_key, meta_value);

                match meta_key {
                    "LIMIT" => {
                        // 'LIMIT(<count>)' where 0 <= <count> < 2^16
                        if let Ok(query_limit_parsed) = meta_value.parse::<QuerySearchLimit>() {
                            Ok((Some(query_limit_parsed), None, None))
                        } else {
                            Err(ChannelCommandBase::make_error_invalid_meta_value(
                                &meta_key,
                                &meta_value,
                            ))
                        }
                    }
                    "OFFSET" => {
                        // 'OFFSET(<count>)' where 0 <= <count> < 2^32
                        if let Ok(query_offset_parsed) = meta_value.parse::<QuerySearchOffset>() {
                            Ok((None, Some(query_offset_parsed), None))
                        } else {
                            Err(ChannelCommandBase::make_error_invalid_meta_value(
                                &meta_key,
                                &meta_value,
                            ))
                        }
                    }
                    "LANG" => {
                        // 'LANG(<locale>)' where <locale> ∈ ISO 639-3
                        if let Some(query_lang_parsed) = QuerySearchLang::from_code(meta_value) {
                            Ok((None, None, Some(query_lang_parsed)))
                        } else {
                            Err(ChannelCommandBase::make_error_invalid_meta_value(
                                &meta_key,
                                &meta_value,
                            ))
                        }
                    }
                    _ => Err(ChannelCommandBase::make_error_invalid_meta_key(
                        &meta_key,
                        &meta_value,
                    )),
                }
            }
            Err(err) => Err(ChannelCommandBase::make_error_invalid_meta_key(
                &err.0, &err.1,
            )),
        }
    }

    fn handle_suggest_meta(
        meta_result: MetaPartsResult,
    ) -> Result<Option<QuerySearchLimit>, ChannelCommandError> {
        match meta_result {
            Ok((meta_key, meta_value)) => {
                debug!("handle suggest meta: {} = {}", meta_key, meta_value);

                match meta_key {
                    "LIMIT" => {
                        // 'LIMIT(<count>)' where 0 <= <count> < 2^16
                        if let Ok(suggest_limit_parsed) = meta_value.parse::<QuerySearchLimit>() {
                            Ok(Some(suggest_limit_parsed))
                        } else {
                            Err(ChannelCommandBase::make_error_invalid_meta_value(
                                &meta_key,
                                &meta_value,
                            ))
                        }
                    }
                    _ => Err(ChannelCommandBase::make_error_invalid_meta_key(
                        &meta_key,
                        &meta_value,
                    )),
                }
            }
            Err(err) => Err(ChannelCommandBase::make_error_invalid_meta_key(
                &err.0, &err.1,
            )),
        }
    }
}

impl ChannelCommandIngest {
    pub fn dispatch_push(mut parts: SplitWhitespace) -> ChannelResult {
        match (
            parts.next(),
            parts.next(),
            parts.next(),
            ChannelCommandBase::parse_text_parts(&mut parts),
        ) {
            (Some(collection), Some(bucket), Some(object), Some(text)) => {
                debug!(
                    "dispatching ingest push in collection: {}, bucket: {} and object: {}",
                    collection, bucket, object
                );
                debug!("ingest push has text: {}", text);

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
                    debug!(
                        "will push for text: {} with hinted locale: <{:?}>",
                        text, push_lang
                    );

                    // Commit 'push' query
                    ChannelCommandBase::commit_ok_operation(QueryBuilder::push(
                        collection, bucket, object, &text, push_lang,
                    ))
                }
            }
            _ => Err(ChannelCommandError::InvalidFormat(
                "PUSH <collection> <bucket> <object> \"<text>\" [LANG(<locale>)]?",
            )),
        }
    }

    pub fn dispatch_pop(mut parts: SplitWhitespace) -> ChannelResult {
        match (
            parts.next(),
            parts.next(),
            parts.next(),
            ChannelCommandBase::parse_text_parts(&mut parts),
            parts.next(),
        ) {
            (Some(collection), Some(bucket), Some(object), Some(text), None) => {
                debug!(
                    "dispatching ingest pop in collection: {}, bucket: {} and object: {}",
                    collection, bucket, object
                );
                debug!("ingest pop has text: {}", text);

                // Make 'pop' query
                ChannelCommandBase::commit_result_operation(QueryBuilder::pop(
                    collection, bucket, object, &text,
                ))
            }
            _ => Err(ChannelCommandError::InvalidFormat(
                "POP <collection> <bucket> <object> \"<text>\"",
            )),
        }
    }

    pub fn dispatch_count(mut parts: SplitWhitespace) -> ChannelResult {
        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(collection), bucket_part, object_part, None) => {
                debug!("dispatching ingest count in collection: {}", collection);

                // Make 'count' query
                ChannelCommandBase::commit_result_operation(QueryBuilder::count(
                    collection,
                    bucket_part,
                    object_part,
                ))
            }
            _ => Err(ChannelCommandError::InvalidFormat(
                "COUNT <collection> [<bucket> [<object>]?]?",
            )),
        }
    }

    pub fn dispatch_flushc(mut parts: SplitWhitespace) -> ChannelResult {
        match (parts.next(), parts.next()) {
            (Some(collection), None) => {
                debug!(
                    "dispatching ingest flush collection in collection: {}",
                    collection
                );

                // Make 'flushc' query
                ChannelCommandBase::commit_result_operation(QueryBuilder::flushc(collection))
            }
            _ => Err(ChannelCommandError::InvalidFormat("FLUSHC <collection>")),
        }
    }

    pub fn dispatch_flushb(mut parts: SplitWhitespace) -> ChannelResult {
        match (parts.next(), parts.next(), parts.next()) {
            (Some(collection), Some(bucket), None) => {
                debug!(
                    "dispatching ingest flush bucket in collection: {}, bucket: {}",
                    collection, bucket
                );

                // Make 'flushb' query
                ChannelCommandBase::commit_result_operation(QueryBuilder::flushb(
                    collection, bucket,
                ))
            }
            _ => Err(ChannelCommandError::InvalidFormat(
                "FLUSHB <collection> <bucket>",
            )),
        }
    }

    pub fn dispatch_flusho(mut parts: SplitWhitespace) -> ChannelResult {
        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(collection), Some(bucket), Some(object), None) => {
                debug!(
                    "dispatching ingest flush object in collection: {}, bucket: {}, object: {}",
                    collection, bucket, object
                );

                // Make 'flusho' query
                ChannelCommandBase::commit_result_operation(QueryBuilder::flusho(
                    collection, bucket, object,
                ))
            }
            _ => Err(ChannelCommandError::InvalidFormat(
                "FLUSHO <collection> <bucket> <object>",
            )),
        }
    }

    pub fn dispatch_help(parts: SplitWhitespace) -> ChannelResult {
        ChannelCommandBase::generic_dispatch_help(parts, &*MANUAL_MODE_INGEST)
    }

    fn handle_push_meta(
        meta_result: MetaPartsResult,
    ) -> Result<Option<QueryIngestLang>, ChannelCommandError> {
        match meta_result {
            Ok((meta_key, meta_value)) => {
                debug!("handle push meta: {} = {}", meta_key, meta_value);

                match meta_key {
                    "LANG" => {
                        // 'LANG(<locale>)' where <locale> ∈ ISO 639-3
                        if let Some(query_lang_parsed) = QueryIngestLang::from_code(meta_value) {
                            Ok(Some(query_lang_parsed))
                        } else {
                            Err(ChannelCommandBase::make_error_invalid_meta_value(
                                &meta_key,
                                &meta_value,
                            ))
                        }
                    }
                    _ => Err(ChannelCommandBase::make_error_invalid_meta_key(
                        &meta_key,
                        &meta_value,
                    )),
                }
            }
            Err(err) => Err(ChannelCommandBase::make_error_invalid_meta_key(
                &err.0, &err.1,
            )),
        }
    }
}

impl ChannelCommandControl {
    pub fn dispatch_trigger(mut parts: SplitWhitespace) -> ChannelResult {
        match (parts.next(), parts.next()) {
            (None, _) => Ok(vec![ChannelCommandResponse::Result(format!(
                "actions({})",
                CONTROL_TRIGGER_ACTIONS.join(", ")
            ))]),
            (Some(action_key), next_part) => {
                if next_part.is_none() == true {
                    let action_key_lower = action_key.to_lowercase();

                    match action_key_lower.as_str() {
                        "consolidate" => {
                            // Force a FST consolidate
                            StoreFSTPool::consolidate(true);

                            Ok(vec![ChannelCommandResponse::Ok])
                        }
                        _ => Err(ChannelCommandError::NotFound),
                    }
                } else {
                    Err(ChannelCommandError::InvalidFormat("HELP [<action>]?"))
                }
            }
        }
    }

    pub fn dispatch_help(parts: SplitWhitespace) -> ChannelResult {
        ChannelCommandBase::generic_dispatch_help(parts, &*MANUAL_MODE_CONTROL)
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
