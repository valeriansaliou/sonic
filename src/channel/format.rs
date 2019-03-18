// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::collections::VecDeque;

pub fn unescape(text: &str) -> String {
    let mut queue: VecDeque<_> = String::from(text).chars().collect();
    let mut unescaped = String::new();

    while let Some(character) = queue.pop_front() {
        if character != '\\' {
            unescaped.push(character);

            continue;
        }

        match queue.pop_front() {
            Some('n') => unescaped.push('\n'),
            Some('\"') => unescaped.push('\"'),
            _ => unescaped.push(character),
        };
    }

    unescaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_unescapes_command_text() {
        assert_eq!(unescape(r#"hello world!"#), r#"hello world!"#.to_string());
        assert_eq!(
            unescape(r#"i'm so good at this"#),
            r#"i'm so good at this"#.to_string()
        );
        assert_eq!(
            unescape(r#"look at \\\\"\\\" me i'm \\"\"trying to hack you\""#),
            r#"look at \\"\" me i'm \""trying to hack you""#.to_string()
        );
    }
}
