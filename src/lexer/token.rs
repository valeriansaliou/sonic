// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::collections::HashSet;
use unicode_segmentation::{UnicodeSegmentation, UnicodeWords};
use whatlang::{detect as lang_detect, Lang};

use super::stopwords::LexerStopWord;
use crate::store::identifiers::{StoreTermHash, StoreTermHashed};

pub struct TokenLexerBuilder;

pub struct TokenLexer<'a> {
    locale: Option<Lang>,
    words: UnicodeWords<'a>,
    yields: HashSet<StoreTermHashed>,
}

impl TokenLexerBuilder {
    pub fn from(text: &str) -> Result<TokenLexer, ()> {
        // Detect text language
        let locale = match lang_detect(text) {
            Some(detector) => {
                info!(
                    "locale detected from lexer text: {} (locale: {}, script: {}, confidence: {})",
                    text,
                    detector.lang(),
                    detector.script(),
                    detector.confidence()
                );

                Some(detector.lang())
            }
            None => {
                info!("no locale could be detected from lexer text: {}", text);

                None
            }
        };

        // Build final token builder iterator
        Ok(TokenLexer::new(text, locale))
    }
}

impl<'a> TokenLexer<'a> {
    fn new(text: &'a str, locale: Option<Lang>) -> TokenLexer<'a> {
        TokenLexer {
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

            // Check if normalized word is a stop-word?
            if LexerStopWord::is(&word, self.locale) == false {
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
                    debug!("lexer did not yield word: {} because: word already yielded", word);
                }
            } else {
                debug!("lexer did not yield word: {} because: word is a stop-word", word);
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
        let mut token_cleaner =
            TokenLexerBuilder::from("The quick brown fox jumps over the lazy dog!").unwrap();

        assert_eq!(token_cleaner.locale, Some(Lang::Eng));
        assert_eq!(token_cleaner.next(), Some("quick".to_string()));
        assert_eq!(token_cleaner.next(), Some("brown".to_string()));
        assert_eq!(token_cleaner.next(), Some("fox".to_string()));
        assert_eq!(token_cleaner.next(), Some("jumps".to_string()));
        assert_eq!(token_cleaner.next(), Some("lazy".to_string()));
        assert_eq!(token_cleaner.next(), Some("dog".to_string()));
        assert_eq!(token_cleaner.next(), None);
    }

    #[test]
    fn it_cleans_token_french() {
        let mut token_cleaner =
            TokenLexerBuilder::from("Le vif renard brun saute par dessus le chien paresseux.")
                .unwrap();

        assert_eq!(token_cleaner.locale, Some(Lang::Fra));
        assert_eq!(token_cleaner.next(), Some("renard".to_string()));
        assert_eq!(token_cleaner.next(), Some("brun".to_string()));
        assert_eq!(token_cleaner.next(), Some("saute".to_string()));
        assert_eq!(token_cleaner.next(), Some("chien".to_string()));
        assert_eq!(token_cleaner.next(), Some("paresseux".to_string()));
        assert_eq!(token_cleaner.next(), None);
    }

    #[test]
    fn it_cleans_token_mandarin() {
        let mut token_cleaner = TokenLexerBuilder::from("快狐跨懒狗").unwrap();

        assert_eq!(token_cleaner.locale, Some(Lang::Cmn));
        assert_eq!(token_cleaner.next(), Some("快".to_string()));
        assert_eq!(token_cleaner.next(), Some("狐".to_string()));
        assert_eq!(token_cleaner.next(), Some("跨".to_string()));
        assert_eq!(token_cleaner.next(), Some("懒".to_string()));
        assert_eq!(token_cleaner.next(), Some("狗".to_string()));
        assert_eq!(token_cleaner.next(), None);
    }

    #[test]
    fn it_cleans_token_emojis() {
        let mut token_cleaner =
            TokenLexerBuilder::from("🚀 🙋‍♂️🙋‍♂️🙋‍♂️").unwrap();

        assert_eq!(token_cleaner.locale, None);
        assert_eq!(token_cleaner.next(), None);
    }
}
