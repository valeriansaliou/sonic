// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

//! Utility functions.

pub(crate) mod itertools;
pub(crate) mod serde;

pub(super) fn split_leading_digits(s: &str) -> (&str, &str) {
    let split_index = (s.char_indices())
        .find_map(|(i, c)| (!c.is_ascii_digit()).then_some(i))
        .unwrap_or(s.len()); // If all are digits

    s.split_at(split_index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_leading_digits() {
        assert_eq!(split_leading_digits("10MB"), ("10", "MB"));
        assert_eq!(split_leading_digits("10"), ("10", ""));
        assert_eq!(split_leading_digits("MB"), ("", "MB"));
        assert_eq!(split_leading_digits(""), ("", ""));
        assert_eq!(split_leading_digits("10Mb/s"), ("10", "Mb/s"));
        assert_eq!(split_leading_digits("10MB123"), ("10", "MB123"));
    }
}
