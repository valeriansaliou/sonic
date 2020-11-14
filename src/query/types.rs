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

impl QueryGenericLang {
    pub fn from_value(value: &str) -> Option<QueryGenericLang> {
        if value == "none" {
            Some(QueryGenericLang::Disabled)
        } else {
            Lang::from_code(value).map(QueryGenericLang::Enabled)
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
