// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use whatlang::Lang;

#[derive(Debug, PartialEq)]
pub enum QueryGenericLang {
    Enabled(Lang),
    Disabled,
}

pub type QuerySearchID<'a> = &'a str;
pub type QuerySearchLimit = u16;
pub type QuerySearchOffset = u32;

pub type QueryMatchScore = u16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueryTimeRange {
    pub from_ms: u64,
    pub to_ms: u64,
}

impl QueryTimeRange {
    pub fn new(from_ms: u64, to_ms: u64) -> Result<Self, ()> {
        if from_ms > to_ms {
            return Err(());
        }
        Ok(Self { from_ms, to_ms })
    }
}

pub type QueryMetaData = (
    Option<QuerySearchLimit>,
    Option<QuerySearchOffset>,
    Option<QueryGenericLang>,
    Option<u64>,
    Option<u64>,
);

pub type ListMetaData = (Option<QuerySearchLimit>, Option<QuerySearchOffset>);

impl QueryGenericLang {
    pub fn from_value(value: &str) -> Option<QueryGenericLang> {
        if value == "none" {
            Some(QueryGenericLang::Disabled)
        } else {
            Lang::from_code(value).map(QueryGenericLang::Enabled)
        }
    }

    pub fn into_lang_opt(self) -> Option<Lang> {
        match self {
            Self::Enabled(lang) => Some(lang),
            Self::Disabled => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_parses_generic_lang_from_value() {
        assert_eq!(
            QueryGenericLang::from_value("none"),
            Some(QueryGenericLang::Disabled)
        );
        assert_eq!(
            QueryGenericLang::from_value("fra"),
            Some(QueryGenericLang::Enabled(Lang::Fra))
        );
        assert_eq!(QueryGenericLang::from_value("xxx"), None);
    }
}
