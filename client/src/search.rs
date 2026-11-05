// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::channel::{ChannelMode, SonicChannel};
use crate::util::errors::io_error_invalid_data;
use crate::util::{impl_channel_structs, impl_fns, make_command};
use crate::{events, options};

// NOTE: Shorter type aliases.
use self::SearchMode as Mode;
pub use self::SearchModeDiscriminant as Discriminant;

impl_channel_structs!(Search("search"):
    SonicChannelSearch / SonicChannelSearchBlocking / SonicChannelSearchAsync
);

enum SearchMode {}

/// Disciminants for all possible Sonic messages (response lines) when in
/// Search mode.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SearchModeDiscriminant {
    Pong,
    Pending,
    EventQuery(Box<str>),
    EventSuggest(Box<str>),
    EventList(Box<str>),
    Ended,
}

impl crate::channel::Discriminant for SearchModeDiscriminant {}

impl ChannelMode for SearchMode {
    type Discriminant = SearchModeDiscriminant;

    fn name() -> &'static str {
        "search"
    }

    fn parse<'a>(
        discriminant: &'a str,
        rest: &'a str,
    ) -> std::io::Result<(Self::Discriminant, &'a str)> {
        match discriminant {
            "PONG" => Ok((Discriminant::Pong, rest)),
            "PENDING" => Ok((Discriminant::Pending, rest)),
            "EVENT" => {
                let Some((discriminant, rest)) = rest.split_once(' ') else {
                    return Err(io_error_invalid_data("'EVENT' missing discriminant"));
                };

                let discriminant = match discriminant {
                    "QUERY" => Discriminant::EventQuery,
                    "SUGGEST" => Discriminant::EventSuggest,
                    "LIST" => Discriminant::EventList,
                    s => {
                        return Err(io_error_invalid_data(format!(
                            "Unknown 'EVENT' discriminant: {s:?}"
                        )));
                    }
                };

                let Some((id, rest)) = rest.split_once(' ') else {
                    return Err(io_error_invalid_data("'EVENT' missing identifier"));
                };

                Ok((discriminant(Box::from(id)), rest))
            }
            "ENDED" => Ok((Discriminant::Ended, rest)),
            "ERR" => Err(std::io::Error::other(rest)),
            s => Err(io_error_invalid_data(format!(
                "Unknown line discriminant: {s:?}"
            ))),
        }
    }
}

// MARK: QUERY

pub trait QueryOption: std::fmt::Display + Sync {}

impl QueryOption for options::Limit {}
impl<'a> QueryOption for options::Lang<'a> {}
impl QueryOption for options::Offset {}

impl_fns!(
    #[doc = "Time complexity: O(1) if enough exact word matches or O(N)"]
    #[doc = "if not enough exact matches where N is the number of"]
    #[doc = "alternate words tried, in practice it approaches O(1)."]
    #[inline]
    #[doc(alias = "search")]
    pub fn query(
        &self,
        collection: impl AsRef<str>,
        bucket: impl AsRef<str>,
        text: impl AsRef<str>,
    ) -> std::io::Result<Vec<Box<str>>> {
        self.query_with_options(collection, bucket, text, &[])
    }
);

impl_fns!(
    #[doc = "Time complexity: O(1) if enough exact word matches or O(N)"]
    #[doc = "if not enough exact matches where N is the number of"]
    #[doc = "alternate words tried, in practice it approaches O(1)."]
    #[doc(alias = "search_with_options")]
    pub fn query_with_options<'a>(
        &self,
        collection: impl AsRef<str>,
        bucket: impl AsRef<str>,
        text: impl AsRef<str>,
        options: &[&'a dyn QueryOption],
    ) -> std::io::Result<Vec<Box<str>>> {
        self.inner.send_async(
            make_command!("QUERY {} {}", collection, bucket; text: text; options: options),
            Discriminant::Pending,
            |id| Discriminant::EventQuery(Box::from(id)),
            |data| Ok(events::parse_string_vec_naive(data)),
        )
    }
);

#[cfg(all(feature = "experimental-api", feature = "snippets"))]
#[derive(Debug, Clone, Copy, Default)]
pub struct QuerySnippetsConfig {
    /// Maximum number of snippets per object.
    pub window_count: Option<usize>,

    /// Maximum number of tokens (~words) a snippet can contain.
    ///
    /// Examples:
    ///
    /// - `window_size = 2` -> `snippet = "foo bar"`
    /// - `window_size = 3` -> `snippet = "foo bar baz"`
    pub window_size: Option<usize>,

    /// Maximum number of non-matching tokens used to pad snippets so they don’t
    /// start and end with matching tokens.
    ///
    /// Examples (`text = "xxx xxx xxx foo xxx foo xxx xxx xxx"`, `query = "foo"`, `window_size = 7`):
    ///
    /// - `max_window_padding = 4` -> `snippet = "xxx xxx foo xxx foo xxx xxx"` (padding is balanced if even)
    /// - `max_window_padding = 1` -> `snippet = "xxx foo xxx foo"` (leading is favored if padding is odd)
    pub max_window_padding: Option<usize>,

    /// Ordering of same-score snippets when associated to fragments.
    ///
    /// When an object has multiple fragments, Sonic uses BM25-like sorting to
    /// choose the snippets. However, when fragments have the same score they
    /// end up in their reverse ingestion order (default Sonic behavior).
    ///
    /// While it makes sense in most systems, where users expect the most recent
    /// data to be returned first (e.g. for a chat-like dataset), it feels off
    /// for a knowledge base / wiki / blog, etc. (where earlier sections are
    /// expected to be returned first).
    ///
    /// When [`ordering`][Self::ordering] is set to `Some(QuerySnippetsOrdering::IngestionOrder)`,
    /// same-score fragments are returned in the order they were ingested in.
    #[cfg(feature = "experimental-api")]
    pub ordering: Option<QuerySnippetsOrdering>,
}

#[cfg(all(feature = "experimental-api", feature = "snippets"))]
#[derive(Debug, Clone, Copy)]
pub enum QuerySnippetsOrdering {
    ReverseIngestionOrder,
    IngestionOrder,
}

#[cfg(all(feature = "experimental-api", feature = "snippets"))]
impl std::fmt::Display for QuerySnippetsConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let QuerySnippetsConfig {
            window_count,
            window_size,
            max_window_padding,
            #[cfg(feature = "experimental-api")]
            ordering,
        } = self;

        write!(f, " WITH SNIPPETS").unwrap();

        if matches!(ordering, &Some(QuerySnippetsOrdering::IngestionOrder)) {
            write!(f, "_REV").unwrap();
        }

        if let Some(window_count) = window_count {
            write!(f, "({window_count}").unwrap();

            if let Some(window_size) = window_size {
                write!(f, ", {window_size}").unwrap();

                if let Some(max_window_padding) = max_window_padding {
                    write!(f, ", {max_window_padding}").unwrap();
                }
            }

            write!(f, ")").unwrap();
        }

        Ok(())
    }
}

#[cfg(all(feature = "experimental-api", feature = "snippets"))]
#[derive(Debug, Clone, PartialEq)]
#[derive(serde::Deserialize)]
pub struct MatchWithSnippets {
    pub oid: Box<str>,
    pub snippets: Vec<Snippet>,
}

#[cfg(all(feature = "experimental-api", feature = "snippets"))]
#[derive(Debug, Clone, PartialEq)]
#[derive(serde::Deserialize)]
pub struct Snippet {
    #[serde(default)]
    pub fragment: Option<Box<str>>,
    pub text: Box<str>,
    pub matches: Vec<MatchRange>,
    pub score: f32,
    pub range: MatchRange,
}

#[cfg(all(feature = "experimental-api", feature = "snippets"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(serde::Deserialize)]
pub struct MatchRange {
    pub start: usize,
    pub end: usize,
}

impl_fns!(
    #[cfg(all(feature = "experimental-api", feature = "snippets"))]
    #[inline]
    pub fn query_snippets(
        &self,
        collection: impl AsRef<str>,
        bucket: impl AsRef<str>,
        text: impl AsRef<str>,
        config: QuerySnippetsConfig,
    ) -> std::io::Result<Vec<MatchWithSnippets>> {
        self.query_snippets_with_options(collection, bucket, text, config, &[])
    }
);

impl_fns!(
    #[cfg(all(feature = "experimental-api", feature = "snippets"))]
    pub fn query_snippets_with_options(
        &self,
        collection: impl AsRef<str>,
        bucket: impl AsRef<str>,
        text: impl AsRef<str>,
        config: QuerySnippetsConfig,
        options: &[&dyn QueryOption],
    ) -> std::io::Result<Vec<MatchWithSnippets>> {
        let config = &config.to_string();

        self.inner.send_async(
            make_command!("QUERY {} {}", collection, bucket; text: text; options: options + config),
            Discriminant::Pending,
            |id| Discriminant::EventQuery(Box::from(id)),
            |data| serde_json::from_str(data).map_err(std::io::Error::from),
        )
    }
);

// MARK: SUGGEST

pub trait SuggestOption: std::fmt::Display + Sync {}

impl SuggestOption for options::Limit {}

impl_fns!(
    #[doc = "Time complexity: O(1)."]
    #[inline]
    pub fn suggest(
        &self,
        collection: impl AsRef<str>,
        bucket: impl AsRef<str>,
        word: impl AsRef<str>,
    ) -> std::io::Result<Vec<Box<str>>> {
        self.suggest_with_options(collection, bucket, word, &[])
    }
);

impl_fns!(
    #[doc = "Time complexity: O(1)."]
    pub fn suggest_with_options<'a>(
        &self,
        collection: impl AsRef<str>,
        bucket: impl AsRef<str>,
        word: impl AsRef<str>,
        options: &[&'a dyn SuggestOption],
    ) -> std::io::Result<Vec<Box<str>>> {
        self.inner.send_async(
            make_command!("SUGGEST {} {}", collection, bucket; text: word; options: options),
            Discriminant::Pending,
            |id| Discriminant::EventSuggest(Box::from(id)),
            |data| Ok(events::parse_string_vec_naive(data)),
        )
    }
);

// MARK: LIST

pub trait ListOption: std::fmt::Display + Sync {}

impl ListOption for options::Limit {}
impl ListOption for options::Offset {}

impl_fns!(
    #[doc = "Time complexity: O(N) where N is the number of words"]
    #[doc = "enumerated, within provided limits."]
    #[inline]
    pub fn list(
        &self,
        collection: impl AsRef<str>,
        bucket: impl AsRef<str>,
    ) -> std::io::Result<Vec<Box<str>>> {
        self.list_with_options(collection, bucket, &[])
    }
);

impl_fns!(
    #[doc = "Time complexity: O(N) where N is the number of words"]
    #[doc = "enumerated, within provided limits."]
    #[inline]
    pub fn list_with_options<'a>(
        &self,
        collection: impl AsRef<str>,
        bucket: impl AsRef<str>,
        options: &[&'a dyn ListOption],
    ) -> std::io::Result<Vec<Box<str>>> {
        self.inner.send_async(
            make_command!("LIST {} {}", collection, bucket; options: options),
            Discriminant::Pending,
            |id| Discriminant::EventList(Box::from(id)),
            |data| Ok(events::parse_string_vec_naive(data)),
        )
    }
);

// MARK: PING

impl_fns!(
    #[doc = "Time complexity: O(1)."]
    pub fn ping(&self) -> std::io::Result<()> {
        self.inner
            .send(make_command!("PING"), Discriminant::Pong, |_data| Ok(()))
    }
);

// MARK: QUIT

impl_fns!(
    #[doc = "Time complexity: O(1)."]
    pub fn quit(&mut self) -> std::io::Result<()> {
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
    pub fn send<T>(
        &self,
        command: crate::Command,
        discriminant: self::Discriminant,
        parse: impl Fn(&str) -> std::io::Result<T> + Send + 'static,
    ) -> std::io::Result<T> {
        self.inner.send(command, discriminant, parse)
    }
    where T: Send + 'static,
);

impl_fns!(
    #[doc = "Send a raw async command."]
    pub fn send_async<T>(
        &self,
        command: crate::Command,
        discriminant1: impl Into<self::Discriminant>,
        make_discriminant2: impl FnOnce(&str) -> self::Discriminant + Send + Sync + 'static,
        parse: impl Fn(&str) -> std::io::Result<T> + Send + 'static,
    ) -> std::io::Result<T> {
        self.inner.send_async(command, discriminant1, make_discriminant2, parse)
    }
    where T: Send + 'static,
);
