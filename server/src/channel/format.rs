// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

pub fn unescape(text: &str) -> String {
    // Pre-reserve a byte-aware required capacity as to avoid heap resizes (30% performance \
    //   gain relative to initializing this with a zero-capacity)
    let mut unescaped = String::with_capacity(text.as_bytes().len());
    let mut characters = text.chars();

    while let Some(character) = characters.next() {
        if character == '\\' {
            // Found escaped character
            match characters.next() {
                Some('n') => unescaped.push('\n'),
                Some('\"') => unescaped.push('\"'),
                Some(c) => {
                    // Preserve unknown escape sequences verbatim.
                    unescaped.push(character);
                    unescaped.push(c);
                }
                None => unescaped.push(character),
            };
        } else {
            unescaped.push(character);
        }
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
            r#"look at \\\\"\\" me i'm \\"""trying to hack you""#.to_string()
        );

        // Regression: unknown escape sequences must pass through unchanged.
        assert_eq!(unescape(r"C:\Windows\System32"), r"C:\Windows\System32".to_string());
        assert_eq!(unescape("trailing \\"), "trailing \\".to_string());
    }
}

#[cfg(all(feature = "benchmark", test))]
mod benches {
    extern crate test;

    use super::*;
    use test::Bencher;

    #[bench]
    fn bench_unescape_command_text(b: &mut Bencher) {
        b.iter(|| unescape(r#"i'm \\"\"trying to hack you\""#));
    }
}
