// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use hashbrown::HashSet;
use std::time::Instant;
use unicode_segmentation::{UnicodeSegmentation, UnicodeWords};
use whatlang::{detect as lang_detect, Lang};

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
static TEXT_LANG_DETECT_OVER_CHARS: usize = 20;

impl TokenLexerBuilder {
    pub fn from(mode: TokenLexerMode, text: &str) -> Result<TokenLexer, ()> {
        // Detect text language (if current lexer mode asks for a cleanup, and text is long-enough \
        //   to allow the text locale detection system to function properly)
        let locale = if mode == TokenLexerMode::NormalizeAndCleanup
            && text.len() >= TEXT_LANG_DETECT_OVER_CHARS
        {
            let ngram_start = Instant::now();

            // Truncate text if necessary, as to avoid the ngram or stopwords detector to be \
            //   ran on more words than those that are enough to reliably detect a locale.
            let safe_text = if text.len() > TEXT_LANG_TRUNCATE_OVER_CHARS {
                &text[0..TEXT_LANG_TRUNCATE_OVER_CHARS]
            } else {
                text
            };

            match lang_detect(safe_text) {
                Some(detector) => {
                    let mut locale = detector.lang();

                    let ngram_took = ngram_start.elapsed();

                    info!(
                        "locale detected from lexer text: {} ({} from {} at {}/1 in {}s + {}ms)",
                        text,
                        locale,
                        detector.script(),
                        detector.confidence(),
                        ngram_took.as_secs(),
                        ngram_took.subsec_millis()
                    );

                    // Confidence is low, try to detect locale from stop-words.
                    if detector.is_reliable() == false {
                        debug!(
                            "trying to detect locale from stopwords, as locale is marked as unreliable"
                        );

                        let stopwords_start = Instant::now();

                        // Better alternate locale found?
                        if let Some(alternate_locale) =
                            LexerStopWord::guess_lang(safe_text, detector.script())
                        {
                            let stopwords_took = stopwords_start.elapsed();

                            info!(
                                "detected more accurate locale from stopwords: {} (took: {}s + {}ms)",
                                alternate_locale,
                                stopwords_took.as_secs(),
                                stopwords_took.subsec_millis()
                            );

                            locale = alternate_locale;
                        }
                    }

                    Some(locale)
                }
                None => {
                    info!("no locale could be detected from lexer text: {}", text);

                    None
                }
            }
        } else {
            debug!("not detecting locale from lexer text: {}", text);

            // May be 'NormalizeOnly' mode; no need to perform a locale detection
            None
        };

        // Build final token builder iterator
        Ok(TokenLexer::new(mode, text, locale))
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
    fn it_cleans_token_mandarin() {
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
}
