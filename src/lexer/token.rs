// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use iso639_2::Iso639_2;
use unicode_segmentation::{UnicodeSegmentation, UnicodeWords};

pub struct TokenLexerBuilder;

pub struct TokenLexer<'a> {
    locale: Option<Iso639_2>,
    words: UnicodeWords<'a>,
}

impl TokenLexerBuilder {
    pub fn from(text: &str) -> Result<TokenLexer, ()> {
        // Detect text language
        // TODO: from 'text' w/ 'ngram'
        let locale = None;

        // Build final token builder iterator
        Ok(TokenLexer::new(text, locale))
    }
}

impl<'a> TokenLexer<'a> {
    fn new(text: &'a str, locale: Option<Iso639_2>) -> TokenLexer<'a> {
        TokenLexer {
            locale: locale,
            words: text.unicode_words(),
        }
    }
}

impl<'a> Iterator for TokenLexer<'a> {
    type Item = String;

    fn next(&mut self) -> Option<Self::Item> {
        // TODO: nuke non-words + gibberish
        // TODO: nuke stop-words
        if let Some(word) = self.words.next() {
            // Lower-case word
            // Notice: unfortunately, as Rust is unicode-aware, we need to convert the str slice \
            //   to a heap-indexed String; as lower-cased characters may change in bit size.
            let word = word.to_lowercase();

            return Some(word);
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_cleans_token() {
        let mut token_cleaner = TokenLexer::new("The quick brown fox!");

        assert_eq!(token_cleaner.next(), Some("the".to_string()));
        assert_eq!(token_cleaner.next(), Some("quick".to_string()));
        assert_eq!(token_cleaner.next(), Some("brown".to_string()));
        assert_eq!(token_cleaner.next(), Some("fox".to_string()));
        assert_eq!(token_cleaner.next(), None);
    }
}
