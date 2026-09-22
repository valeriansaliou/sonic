// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

pub trait Join {
    fn join(self, separator: &str) -> String;
}

impl<'a, I> Join for I
where
    I: Iterator<Item = &'a str>,
{
    fn join(self, separator: &str) -> String {
        self.fold(String::new(), |mut result, s| {
            if !result.is_empty() {
                result.push_str(separator);
            }
            result.push_str(s);
            result
        })
    }
}
