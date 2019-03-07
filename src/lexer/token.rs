// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use unicode_segmentation::{UnicodeSegmentation, UnicodeWords};
use whatlang::{detect as lang_detect, Lang};

use super::stopwords::LexerStopWord;

pub struct TokenLexerBuilder;

pub struct TokenLexer<'a> {
    locale: Option<Lang>,
    words: UnicodeWords<'a>,
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
        }
    }
}

impl<'a> Iterator for TokenLexer<'a> {
    type Item = String;

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
                debug!("lexer yielded word: {}", word);

                return Some(word);
            }

            debug!("lexer did not yield word: {}", word);
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_cleans_token() {
        let mut token_cleaner = TokenLexer::new("The quick brown fox!", Some(Lang::Eng));

        assert_eq!(token_cleaner.next(), Some("the".to_string()));
        assert_eq!(token_cleaner.next(), Some("quick".to_string()));
        assert_eq!(token_cleaner.next(), Some("brown".to_string()));
        assert_eq!(token_cleaner.next(), Some("fox".to_string()));
        assert_eq!(token_cleaner.next(), None);
    }
}
