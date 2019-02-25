// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use iso639_2::Iso639_2;
use unicode_segmentation::{UnicodeSegmentation, UnicodeWords};

pub struct LexedTokens(TokenLexer, Option<Iso639_2>);
pub struct LexedTokensBuilder;

struct TokenCleaner<'a> {
    words: UnicodeWords<'a>,
}

pub struct TokenLexer {
    // TODO: this is shit
    words: String,
}

pub enum LexedTokensError {
    Void,
}

impl<'a> TokenCleaner<'a> {
    fn new(text: &'a str) -> TokenCleaner<'a> {
        TokenCleaner {
            words: text.unicode_words(),
        }
    }
}

impl<'a> TokenLexer {
    fn new(words: String) -> TokenLexer {
        TokenLexer { words: words }
    }
}

impl LexedTokensBuilder {
    pub fn from(text: &str) -> Result<LexedTokens, LexedTokensError> {
        // TODO: investigate [https://crates.io/crates/natural]

        // TODO: this is shit.
        // let mut entry_token: Option<Token> = None;
        // let mut last_token: Option<Token> = None;

        // 1. Clean text
        let token_cleaner = TokenCleaner::new(text);

        // 2. Detect text language
        // TODO

        // 3. Nuke stop-words
        // TODO

        // while let Some(text_part) = parts.next() {
        //     let token = Token{
        //         word: text_part,
        //         next: None,
        //     };

        //     // Store entry point?
        //     if entry_token.is_none() == true {
        //         entry_token = Some(token);
        //     }

        //     if let Some(last_token_inner) = last_token {
        //         last_token_inner.next = Some(Box::new(token));
        //     }

        //     last_token = Some(token);
        // }

        // TODO
        // 4. Detect locale (ngram)
        // 5. Nuke stopwords using detected locale (rebuild iterator)
        // 6. Rebuld LexedTokens<> (freeze chained tokens object)
        // 7. Validate LexedTokens<> chain is not empty

        // TODO: pass cleaner iterator
        // TODO: return detected language
        Ok(LexedTokens(TokenLexer::new(String::new()), None))
    }
}

impl<'a> Iterator for TokenCleaner<'a> {
    type Item = String;

    fn next(&mut self) -> Option<Self::Item> {
        // TODO: nuke non-words + gibberish
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
        let mut token_cleaner = TokenCleaner::new("The quick brown fox!");

        assert_eq!(token_cleaner.next(), Some("the".to_string()));
        assert_eq!(token_cleaner.next(), Some("quick".to_string()));
        assert_eq!(token_cleaner.next(), Some("brown".to_string()));
        assert_eq!(token_cleaner.next(), Some("fox".to_string()));
        assert_eq!(token_cleaner.next(), None);
    }
}
