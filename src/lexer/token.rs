// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Sonic OSS License v1.0 (SOSSL v1.0)

use hashbrown::HashSet;
use std::time::Instant;
use unicode_segmentation::{UnicodeSegmentation, UnicodeWords};
use whatlang::{
    detect as lang_detect_all, detect_lang as lang_detect, detect_script as script_detect, Lang,
};

use super::stopwords::LexerStopWord;
use crate::store::identifiers::{StoreTermHash, StoreTermHashed};

pub struct TokenLexerBuilder;

pub struct TokenLexer<'a> {
    mode: TokenLexerMode,
    locale: Option<Lang>,
    words: UnicodeWords<'a>,
    yields: HashSet<StoreTermHashed>,
}

#[derive(PartialEq)]
pub enum TokenLexerMode {
    NormalizeAndCleanup,
    NormalizeOnly,
}

static TEXT_LANG_TRUNCATE_OVER_CHARS: usize = 200;
static TEXT_LANG_DETECT_PROCEED_OVER_CHARS: usize = 20;
static TEXT_LANG_DETECT_NGRAM_UNDER_CHARS: usize = 60;

impl TokenLexerBuilder {
    pub fn from(mode: TokenLexerMode, text: &str) -> Result<TokenLexer, ()> {
        // Detect text language? (if current lexer mode asks for a cleanup)
        let locale = if mode == TokenLexerMode::NormalizeAndCleanup {
            debug!("detecting locale from lexer text: {}", text);

            Self::detect_lang(text)
        } else {
            debug!("not detecting locale from lexer text: {}", text);

            // May be 'NormalizeOnly' mode; no need to perform a locale detection
            None
        };

        // Build final token builder iterator
        Ok(TokenLexer::new(mode, text, locale))
    }

    fn detect_lang(text: &str) -> Option<Lang> {
        // Detect only if text is long-enough to allow the text locale detection system to \
        //   function properly
        if text.len() < TEXT_LANG_DETECT_PROCEED_OVER_CHARS {
            return None;
        }

        // Truncate text if necessary, as to avoid the ngram or stopwords detector to be \
        //   ran on more words than those that are enough to reliably detect a locale.
        let safe_text = if text.len() > TEXT_LANG_TRUNCATE_OVER_CHARS {
            debug!(
                "lexer text needs to be truncated, as it is too long ({}/{}): {}",
                text.len(),
                TEXT_LANG_TRUNCATE_OVER_CHARS,
                text
            );

            // Perform an UTF-8 aware truncation
            // Notice: then 'len()' check above was not UTF-8 aware, but is better than \
            //   nothing as it avoids entering the below iterator for small strings.
            // Notice: we fallback on text if the result is 'None'; as if it is 'None' there \
            //   was less characters than the truncate limit in the UTF-8 parsed text. With \
            //   this unwrap-way, we avoid doing a 'text.chars().count()' everytime, which is \
            //   a O(N) operation, and rather guard this block with a 'text.len()' which is \
            //   a O(1) operation but which is not 100% reliable when approaching the truncate \
            //   limit. This is a trade-off, which saves quite a lot CPU cycles at scale.
            text.char_indices()
                .nth(TEXT_LANG_TRUNCATE_OVER_CHARS)
                .map(|(end_index, _)| &text[0..end_index])
                .unwrap_or(text)
        } else {
            text
        };

        debug!("will detect locale for lexer safe text: {}", safe_text);

        // Attempt to detect the locale from text using an hybrid method that maximizes both \
        //   accuracy and performance.
        // Notice: as the 'ngram' method is almost 10x slower than the 'stopwords' method, we \
        //   prefer using the 'stopwords' method on long texts where we can be sure to see quite \
        //   a lot of stopwords which will produce a reliable result. However, for shorter texts \
        //   there are not enough north none stopwords, thus we use the slower 'ngram' method as \
        //   an attempt to extract the locale using trigrams. Still, if either of these methods \
        //   fails at detecting a locale it will try using the other method in fallback as to \
        //   produce the most reliable result while minimizing CPU cycles.
        if safe_text.len() < TEXT_LANG_DETECT_NGRAM_UNDER_CHARS {
            debug!(
                "lexer text is shorter than {} characters, using the slow method",
                TEXT_LANG_DETECT_NGRAM_UNDER_CHARS
            );

            Self::detect_lang_slow(safe_text)
        } else {
            debug!(
                "lexer text is equal or longer than {} characters, using the fast method",
                TEXT_LANG_DETECT_NGRAM_UNDER_CHARS
            );

            Self::detect_lang_fast(safe_text)
        }
    }

    fn detect_lang_slow(safe_text: &str) -> Option<Lang> {
        let ngram_start = Instant::now();

        match lang_detect_all(safe_text) {
            Some(detector) => {
                let ngram_took = ngram_start.elapsed();

                let mut locale = detector.lang();

                info!(
                    "[slow lexer] locale detected from text: {} ({} from {} at {}/1; {}s + {}ms)",
                    safe_text,
                    locale,
                    detector.script(),
                    detector.confidence(),
                    ngram_took.as_secs(),
                    ngram_took.subsec_millis()
                );

                // Confidence is low, try to detect locale from stop-words.
                // Notice: this is a fallback but should not be too reliable for short \
                //   texts.
                if detector.is_reliable() == false {
                    debug!("[slow lexer] trying to detect locale from stopwords instead");

                    // Better alternate locale found?
                    if let Some(alternate_locale) =
                        LexerStopWord::guess_lang(safe_text, detector.script())
                    {
                        info!(
                            "[slow lexer] detected more accurate locale from stopwords: {}",
                            alternate_locale
                        );

                        locale = alternate_locale;
                    }
                }

                Some(locale)
            }
            None => {
                info!(
                    "[slow lexer] no locale could be detected from text: {}",
                    safe_text
                );

                None
            }
        }
    }

    fn detect_lang_fast(safe_text: &str) -> Option<Lang> {
        let stopwords_start = Instant::now();

        match script_detect(safe_text) {
            Some(script) => {
                // Locale found?
                if let Some(locale) = LexerStopWord::guess_lang(safe_text, script) {
                    let stopwords_took = stopwords_start.elapsed();

                    info!(
                        "[fast lexer] locale detected from text: {} ({}; {}s + {}ms)",
                        safe_text,
                        locale,
                        stopwords_took.as_secs(),
                        stopwords_took.subsec_millis()
                    );

                    Some(locale)
                } else {
                    debug!("[fast lexer] trying to detect locale from fallback ngram instead");

                    // No locale found, fallback on slow ngram.
                    lang_detect(safe_text)
                }
            }
            None => {
                info!(
                    "[fast lexer] no script could be detected from text: {}",
                    safe_text
                );

                None
            }
        }
    }
}

impl<'a> TokenLexer<'a> {
    fn new(mode: TokenLexerMode, text: &'a str, locale: Option<Lang>) -> TokenLexer<'a> {
        TokenLexer {
            mode: mode,
            locale: locale,
            words: text.unicode_words(),
            yields: HashSet::new(),
        }
    }
}

impl<'a> Iterator for TokenLexer<'a> {
    type Item = (String, StoreTermHashed);

    // Guarantees provided by the lexer on the output: \
    //   - Text is split per-word in a script-aware way \
    //   - Words are normalized (ie. lower-case) \
    //   - Gibberish words are removed (ie. words that may just be junk) \
    //   - Stop-words are removed
    fn next(&mut self) -> Option<Self::Item> {
        while let Some(word) = self.words.next() {
            // Lower-case word
            // Notice: unfortunately, as Rust is unicode-aware, we need to convert the str slice \
            //   to a heap-indexed String; as lower-cased characters may change in bit size.
            let word = word.to_lowercase();

            // Check if normalized word is a stop-word? (if should normalize and cleanup)
            if self.mode != TokenLexerMode::NormalizeAndCleanup
                || LexerStopWord::is(&word, self.locale) == false
            {
                // Hash the term (this is used by all iterator consumers, as well as internally \
                //   in the iterator to keep track of already-yielded words in a space-optimized \
                //   manner, ie. by using 32-bit unsigned integer hashes)
                let term_hash = StoreTermHash::from(&word);

                // Check if word was not already yielded? (we return unique words)
                if self.yields.contains(&term_hash) == false {
                    debug!("lexer yielded word: {}", word);

                    self.yields.insert(term_hash);

                    return Some((word, term_hash));
                } else {
                    debug!(
                        "lexer did not yield word: {} because: word already yielded",
                        word
                    );
                }
            } else {
                debug!(
                    "lexer did not yield word: {} because: word is a stop-word",
                    word
                );
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_cleans_token_english() {
        let mut token_cleaner = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            "The quick brown fox jumps over the lazy dog!",
        )
        .unwrap();

        assert_eq!(token_cleaner.locale, Some(Lang::Eng));
        assert_eq!(
            token_cleaner.next(),
            Some(("quick".to_string(), 4179131656))
        );
        assert_eq!(
            token_cleaner.next(),
            Some(("brown".to_string(), 1268820067))
        );
        assert_eq!(token_cleaner.next(), Some(("fox".to_string(), 667256324)));
        assert_eq!(token_cleaner.next(), Some(("jumps".to_string(), 633865164)));
        assert_eq!(token_cleaner.next(), Some(("lazy".to_string(), 4130433347)));
        assert_eq!(token_cleaner.next(), Some(("dog".to_string(), 2044924251)));
        assert_eq!(token_cleaner.next(), None);
    }

    #[test]
    fn it_cleans_token_french() {
        let mut token_cleaner = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            "Le vif renard brun saute par dessus le chien paresseux.",
        )
        .unwrap();

        assert_eq!(token_cleaner.locale, Some(Lang::Fra));
        assert_eq!(
            token_cleaner.next(),
            Some(("renard".to_string(), 1635186311))
        );
        assert_eq!(token_cleaner.next(), Some(("brun".to_string(), 2763604928)));
        assert_eq!(
            token_cleaner.next(),
            Some(("saute".to_string(), 1918158211))
        );
        assert_eq!(
            token_cleaner.next(),
            Some(("chien".to_string(), 2177818351))
        );
        assert_eq!(
            token_cleaner.next(),
            Some(("paresseux".to_string(), 1678693110))
        );
        assert_eq!(token_cleaner.next(), None);
    }

    #[test]
    fn it_cleans_token_chinese() {
        let mut token_cleaner = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            "快狐跨懒狗快狐跨懒狗",
        )
        .unwrap();

        assert_eq!(token_cleaner.locale, Some(Lang::Cmn));
        assert_eq!(token_cleaner.next(), Some(("快".to_string(), 126546256)));
        assert_eq!(token_cleaner.next(), Some(("狐".to_string(), 2879689662)));
        assert_eq!(token_cleaner.next(), Some(("跨".to_string(), 2913342670)));
        assert_eq!(token_cleaner.next(), Some(("懒".to_string(), 3199935961)));
        assert_eq!(token_cleaner.next(), Some(("狗".to_string(), 3360772096)));
        assert_eq!(token_cleaner.next(), None);
    }

    #[test]
    fn it_cleans_token_emojis() {
        let mut token_cleaner = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            "🚀 🙋‍♂️🙋‍♂️🙋‍♂️",
        )
        .unwrap();

        assert_eq!(token_cleaner.locale, None);
        assert_eq!(token_cleaner.next(), None);
    }

    #[test]
    fn it_detects_lang_english_regular() {
        assert_eq!(
            TokenLexerBuilder::detect_lang("The quick brown fox jumps over the lazy dog!"),
            Some(Lang::Eng)
        );
    }

    #[test]
    fn it_detects_lang_english_long() {
        assert_eq!(
            TokenLexerBuilder::detect_lang(
                r#"Running an electrical current through water splits it into oxygen and hydrogen,
            the latter of which can be used as a reliable, zero-emission fuel source. In the past,
            the process of purifying water beforehand was too energy intensive for this process to
            be useful — but now scientists have figured out how to skip the process altogether and
            convert seawater into usable hydrogen"#
            ),
            Some(Lang::Eng)
        );
    }

    #[test]
    fn it_doesnt_detect_lang_english_tiny() {
        assert_eq!(TokenLexerBuilder::detect_lang("The quick"), None);
    }
}

#[cfg(all(feature = "benchmark", test))]
mod benches {
    extern crate test;

    use super::*;
    use test::Bencher;

    #[bench]
    fn bench_normalize_token_french_build(b: &mut Bencher) {
        b.iter(|| {
            TokenLexerBuilder::from(
                TokenLexerMode::NormalizeOnly,
                "Le vif renard brun saute par dessus le chien paresseux.",
            )
        });
    }

    #[bench]
    fn bench_normalize_token_french_exhaust(b: &mut Bencher) {
        b.iter(|| {
            let token_cleaner = TokenLexerBuilder::from(
                TokenLexerMode::NormalizeOnly,
                "Le vif renard brun saute par dessus le chien paresseux.",
            )
            .unwrap();

            token_cleaner.map(|value| value.1).collect::<Vec<u32>>()
        });
    }

    #[bench]
    fn bench_clean_token_english_regular_build(b: &mut Bencher) {
        b.iter(|| {
            TokenLexerBuilder::from(
                TokenLexerMode::NormalizeAndCleanup,
                "The quick brown fox jumps over the lazy dog!",
            )
        });
    }

    #[bench]
    fn bench_clean_token_english_regular_exhaust(b: &mut Bencher) {
        b.iter(|| {
            let token_cleaner = TokenLexerBuilder::from(
                TokenLexerMode::NormalizeAndCleanup,
                "The quick brown fox jumps over the lazy dog!",
            )
            .unwrap();

            token_cleaner.map(|value| value.1).collect::<Vec<u32>>()
        });
    }

    #[bench]
    fn bench_clean_token_english_long_exhaust(b: &mut Bencher) {
        b.iter(|| {
            let token_cleaner = TokenLexerBuilder::from(
                TokenLexerMode::NormalizeAndCleanup,
                r#"Running an electrical current through water splits it into oxygen and hydrogen,
                the latter of which can be used as a reliable, zero-emission fuel source. In the
                past, the process of purifying water beforehand was too energy intensive for this
                process to be useful — but now scientists have figured out how to skip the process
                altogether and convert seawater into usable hydrogen"#,
            )
            .unwrap();

            token_cleaner.map(|value| value.1).collect::<Vec<u32>>()
        });
    }

    #[bench]
    fn bench_clean_token_chinese_build(b: &mut Bencher) {
        b.iter(|| TokenLexerBuilder::from(TokenLexerMode::NormalizeAndCleanup, "快狐跨懒狗"));
    }

    #[bench]
    fn bench_clean_token_chinese_exhaust(b: &mut Bencher) {
        b.iter(|| {
            let token_cleaner =
                TokenLexerBuilder::from(TokenLexerMode::NormalizeAndCleanup, "快狐跨懒狗")
                    .unwrap();

            token_cleaner.map(|value| value.1).collect::<Vec<u32>>()
        });
    }

    #[bench]
    fn bench_detect_lang_english_short(b: &mut Bencher) {
        b.iter(|| TokenLexerBuilder::detect_lang("The quick brown fox."));
    }

    #[bench]
    fn bench_detect_lang_english_regular(b: &mut Bencher) {
        b.iter(|| TokenLexerBuilder::detect_lang("The quick brown fox jumps over the lazy dog!"));
    }

    #[bench]
    fn bench_detect_lang_english_long(b: &mut Bencher) {
        b.iter(|| {
            TokenLexerBuilder::detect_lang(
                r#"Running an electrical current through water splits it into oxygen and hydrogen,
            the latter of which can be used as a reliable, zero-emission fuel source. In the past,
            the process of purifying water beforehand was too energy intensive for this process to
            be useful — but now scientists have figured out how to skip the process altogether and
            convert seawater into usable hydrogen"#,
            )
        });
    }

    #[bench]
    fn bench_dont_detect_lang_english_tiny(b: &mut Bencher) {
        b.iter(|| TokenLexerBuilder::detect_lang("The quick"));
    }
}
