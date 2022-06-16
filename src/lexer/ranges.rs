// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::fmt;
use whatlang::{detect_script, Script};

struct LexerRange;

#[derive(PartialEq, Debug)]
pub struct LexerRegexRange(&'static [(char, char)]);

const RANGE_LATIN: &[(char, char)] = &[('\u{0000}', '\u{024F}')];
const RANGE_CYRILLIC: &[(char, char)] = &[('\u{0400}', '\u{052F}')];
const RANGE_ARABIC: &[(char, char)] = &[('\u{0600}', '\u{06FF}'), ('\u{0750}', '\u{077F}')];
const RANGE_ARMENIAN: &[(char, char)] = &[('\u{0530}', '\u{058F}')];
const RANGE_DEVANAGARI: &[(char, char)] = &[('\u{0900}', '\u{097F}')];
const RANGE_HIRAGANA: &[(char, char)] = &[('\u{3040}', '\u{309F}')];
const RANGE_KATAKANA: &[(char, char)] = &[('\u{30A0}', '\u{30FF}'), ('\u{31F0}', '\u{31FF}')];
const RANGE_ETHIOPIC: &[(char, char)] = &[('\u{1200}', '\u{139F}'), ('\u{2D80}', '\u{2DDF}')];
const RANGE_HEBREW: &[(char, char)] = &[('\u{0590}', '\u{05FF}')];
const RANGE_BENGALI: &[(char, char)] = &[('\u{0980}', '\u{09FF}')];
const RANGE_GEORGIAN: &[(char, char)] = &[('\u{10A0}', '\u{10FF}'), ('\u{2D00}', '\u{2D2F}')];
const RANGE_MANDARIN: &[(char, char)] = &[
    ('\u{4E00}', '\u{9FFF}'),
    ('\u{3400}', '\u{4DBF}'),
    ('\u{20000}', '\u{2A6DF}'),
    ('\u{2A700}', '\u{2CEAF}'),
];
const RANGE_HANGUL: &[(char, char)] = &[('\u{1100}', '\u{11FF}'), ('\u{3130}', '\u{318F}')];
const RANGE_GREEK: &[(char, char)] = &[('\u{0370}', '\u{03FF}'), ('\u{1F00}', '\u{1FFF}')];
const RANGE_KANNADA: &[(char, char)] = &[('\u{0C80}', '\u{0CFF}')];
const RANGE_TAMIL: &[(char, char)] = &[('\u{0B80}', '\u{0BFF}')];
const RANGE_THAI: &[(char, char)] = &[('\u{0E00}', '\u{0E7F}')];
const RANGE_GUJARATI: &[(char, char)] = &[('\u{0A80}', '\u{0AFF}')];
const RANGE_GURMUKHI: &[(char, char)] = &[('\u{0A00}', '\u{0A7F}')];
const RANGE_TELUGU: &[(char, char)] = &[('\u{0C00}', '\u{0C7F}')];
const RANGE_MALAYALAM: &[(char, char)] = &[('\u{0D00}', '\u{0D7F}')];
const RANGE_ORIYA: &[(char, char)] = &[('\u{0B00}', '\u{0B7F}')];
const RANGE_MYANMAR: &[(char, char)] = &[('\u{1000}', '\u{109F}')];
const RANGE_SINHALA: &[(char, char)] = &[('\u{0D80}', '\u{0DFF}')];
const RANGE_KHMER: &[(char, char)] = &[('\u{1780}', '\u{17FF}'), ('\u{19E0}', '\u{19FF}')];

impl LexerRange {
    pub fn from(text: &str) -> Option<&'static [(char, char)]> {
        detect_script(text).map(|script| match script {
            Script::Latin => RANGE_LATIN,
            Script::Cyrillic => RANGE_CYRILLIC,
            Script::Arabic => RANGE_ARABIC,
            Script::Armenian => RANGE_ARMENIAN,
            Script::Devanagari => RANGE_DEVANAGARI,
            Script::Hiragana => RANGE_HIRAGANA,
            Script::Katakana => RANGE_KATAKANA,
            Script::Ethiopic => RANGE_ETHIOPIC,
            Script::Hebrew => RANGE_HEBREW,
            Script::Bengali => RANGE_BENGALI,
            Script::Georgian => RANGE_GEORGIAN,
            Script::Mandarin => RANGE_MANDARIN,
            Script::Hangul => RANGE_HANGUL,
            Script::Greek => RANGE_GREEK,
            Script::Kannada => RANGE_KANNADA,
            Script::Tamil => RANGE_TAMIL,
            Script::Thai => RANGE_THAI,
            Script::Gujarati => RANGE_GUJARATI,
            Script::Gurmukhi => RANGE_GURMUKHI,
            Script::Telugu => RANGE_TELUGU,
            Script::Malayalam => RANGE_MALAYALAM,
            Script::Oriya => RANGE_ORIYA,
            Script::Myanmar => RANGE_MYANMAR,
            Script::Sinhala => RANGE_SINHALA,
            Script::Khmer => RANGE_KHMER,
        })
    }
}

impl LexerRegexRange {
    pub fn from(text: &str) -> Option<Self> {
        LexerRange::from(text).map(LexerRegexRange)
    }

    pub fn write_to<W: fmt::Write>(&self, formatter: &mut W) -> Result<(), fmt::Error> {
        // Format range to regex range
        formatter.write_char('[')?;

        for range in self.0 {
            write!(
                formatter,
                "\\x{{{:X}}}-\\x{{{:X}}}",
                range.0 as u32, range.1 as u32
            )?;
        }

        formatter.write_char(']')?;

        Ok(())
    }
}

impl Default for LexerRegexRange {
    fn default() -> Self {
        LexerRegexRange(RANGE_LATIN)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_gives_ranges() {
        assert_eq!(LexerRange::from("fox"), Some(RANGE_LATIN));
        assert_eq!(LexerRange::from("快狐跨懒狗"), Some(RANGE_MANDARIN));
        assert_eq!(LexerRange::from("Доброе утро."), Some(RANGE_CYRILLIC));
    }

    #[test]
    fn it_gives_regex_range() {
        assert_eq!(
            LexerRegexRange::from("fox"),
            Some(LexerRegexRange(RANGE_LATIN))
        );
    }
}

#[cfg(all(feature = "benchmark", test))]
mod benches {
    extern crate test;

    use super::*;
    use test::Bencher;

    #[bench]
    fn bench_give_ranges_latin(b: &mut Bencher) {
        b.iter(|| LexerRange::from("fox"));
    }

    #[bench]
    fn bench_give_ranges_mandarin(b: &mut Bencher) {
        b.iter(|| LexerRange::from("快狐跨懒狗"));
    }

    #[bench]
    fn bench_give_ranges_cyrillic(b: &mut Bencher) {
        b.iter(|| LexerRange::from("Доброе утро."));
    }

    #[bench]
    fn bench_give_regex_range_latin(b: &mut Bencher) {
        b.iter(|| LexerRegexRange::from("fox"));
    }
}
