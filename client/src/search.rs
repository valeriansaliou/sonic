// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::channel::{ChannelMode, SonicChannel};
use crate::events;
use crate::options::{Lang, Limit, Offset};
use crate::util::errors::io_error_invalid_data;
use crate::util::{impl_channel_structs, impl_fns, make_command};

// NOTE: Shorter type aliases.
use self::SearchMode as Mode;
use self::SearchModeDiscriminant as Discriminant;

impl_channel_structs!(Search("search"):
    SonicChannelSearch / SonicChannelSearchBlocking / SonicChannelSearchAsync
);

enum SearchMode {}

/// Disciminants for all possible Sonic messages (response lines) when in
/// Search mode.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum SearchModeDiscriminant {
    Pong,
    Pending,
    EventQuery(Box<str>),
    EventSuggest(Box<str>),
    EventList(Box<str>),
    Ended,
}

impl crate::channel::Discriminant for SearchModeDiscriminant {
    fn has_payload(&self) -> bool {
        matches!(
            self,
            Self::EventQuery(_) | Self::EventSuggest(_) | Self::EventList(_)
        )
    }
}

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

impl QueryOption for Limit {}
impl<'a> QueryOption for Lang<'a> {}
impl QueryOption for Offset {}

impl_fns!(
    #[doc = "Time complexity: O(1) if enough exact word matches or O(N)"]
    #[doc = "if not enough exact matches where N is the number of"]
    #[doc = "alternate words tried, in practice it approaches O(1)."]
    #[inline]
    #[doc(alias = "search")]
    fn query(
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
    fn query_with_options<'a>(
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

// MARK: SUGGEST

pub trait SuggestOption: std::fmt::Display + Sync {}

impl SuggestOption for Limit {}

impl_fns!(
    #[doc = "Time complexity: O(1)."]
    #[inline]
    fn suggest(
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
    fn suggest_with_options<'a>(
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

impl ListOption for Limit {}
impl ListOption for Offset {}

impl_fns!(
    #[doc = "Time complexity: O(N) where N is the number of words"]
    #[doc = "enumerated, within provided limits."]
    #[inline]
    fn list(
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
    fn list_with_options<'a>(
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
