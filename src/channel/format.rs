// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::collections::VecDeque;

pub fn unescape(text: &str) -> String {
    // Pre-reserve a byte-aware required capacity as to avoid heap resizes (30% performance \
    //   gain relative to initializing this with a zero-capacity)
    let mut unescaped = String::with_capacity(text.as_bytes().len());

    let mut queue: VecDeque<_> = String::from(text).chars().collect();

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
    extern crate test;

    use super::*;
    use test::Bencher;

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

    #[bench]
    fn bench_unescape_command_text(b: &mut Bencher) {
        b.iter(|| unescape(r#"i'm \\"\"trying to hack you\""#));
    }
}
