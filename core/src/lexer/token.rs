// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

//! Most tokenizing/lexing logic used by Sonic.
//!
//! This module would benefit from some refactoring, as functions are quite long
//! at the moment. At least it works, that’s what counts the most. We’ll do some
//! refactoring once Sonic v2 is released.
// TODO: Refactor module to split long functions into more comprehensible ones.

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use whatlang::Lang;

    use super::itertools::UniqueBy;
    use super::lexing::{SpecialTokenKind, TokenKind};
    use super::preprocessor::{Preprocessor, Token};

    #[test]
    fn test_preprocessor_can_yield_original_positions() {
        let preprocessor = Preprocessor::default();

        let tokens = preprocessor.preprocess("I had a déjà-vu.", None);
        let mut tokens_iter = tokens.tokens();

        assert_eq!(tokens_iter.next().unwrap().range(), 0..=1);
        assert_eq!(tokens_iter.next().unwrap().range(), 2..=5);
        assert_eq!(tokens_iter.next().unwrap().range(), 6..=7);
        assert_eq!(tokens_iter.next().unwrap().range(), 8..=14); // Diacritics
        assert_eq!(tokens_iter.next().unwrap().range(), 15..=17);
    }

    #[test]
    fn test_preprocessor_can_fold_diacritics() {
        let mut preprocessor = Preprocessor::default();

        #[rustfmt::skip]
        assert_eq!(
            preprocessor.preprocess("I had a déjà-vu.", None).normalized_text(),
            "i had a déjà vu"
        );

        preprocessor.normalization_config.diacritic_folding_enabled = true;

        #[rustfmt::skip]
        assert_eq!(
            preprocessor.preprocess("I had a déjà-vu.", None).normalized_text(),
            "i had a deja vu"
        );
    }

    /// Ensures pattern detection can be disabled, and is by default compatible
    /// with v1 indexes.
    #[test]
    fn test_preprocessor_pattern_detection_optional() {
        let mut preprocessor = Preprocessor::default();

        #[rustfmt::skip]
        assert_eq!(
            preprocessor
                .preprocess("Please contact support@example.org", None)
                .tokens().skip(2)
                .map(|token| (token.normalized, token.kind, token.start, token.end, token.index_in_tokenized_text))
                .collect::<Vec<_>>(),
            [
                ("support", TokenKind::Special(SpecialTokenKind::CompatSubtoken), 15, 22, 2),
                ("example.org", TokenKind::Special(SpecialTokenKind::CompatSubtoken), 23, 34, 3),
            ]
        );

        preprocessor
            .tokenization_config
            .compat_split_special_patterns = false;

        #[rustfmt::skip]
        assert_eq!(
            preprocessor
                .preprocess("Please contact support@example.org", None)
                .tokens().skip(2).next()
                .map(|token| (token.as_normalized().to_owned(), token.kind, token.start, token.end, token.index_in_tokenized_text))
                .unwrap(),
            ("support@example.org".to_owned(), TokenKind::Special(SpecialTokenKind::EmailAddress), 15, 34, 2)
        );

        preprocessor.tokenization_config.detect_special_patterns = false;

        #[rustfmt::skip]
        assert_eq!(
            preprocessor
                .preprocess("Please contact support@example.org", None)
                .tokens().skip(2).map(|token| (token.normalized, token.kind)).collect::<Vec<_>>(),
            [
                ("support", TokenKind::Normal),
                ("example.org", TokenKind::Normal),
            ]
        );
    }

    #[test]
    fn test_preprocessor_can_filter_stopwords() {
        let mut preprocessor = Preprocessor::default();

        preprocessor.stopwords_config.deny =
            HashSet::from_iter(["is", "a"].into_iter().map(str::to_owned));

        #[rustfmt::skip]
        assert_eq!(
            preprocessor
                .preprocess("This is a test!", None)
                .tokens().map(Token::into_original).collect::<Vec<_>>(),
            ["This", "test"]
        );

        preprocessor.detect_stopwords = false;

        #[rustfmt::skip]
        assert_eq!(
            preprocessor
                .preprocess("This is a test!", None)
                .tokens().map(Token::into_original).collect::<Vec<_>>(),
            ["This", "is", "a", "test"]
        );
    }

    #[test]
    fn test_preprocessor_can_stem() {
        let mut preprocessor = Preprocessor::default();

        // Disable stopword filtering as it would influence results.
        preprocessor.filter_stopwords = false;

        #[rustfmt::skip]
        assert_eq!(
            preprocessor.preprocess("Hackers doing hacking", Some(Lang::Eng)).normalized_text(),
            "hackers doing hacking"
        );

        preprocessor.normalization_config.stemming_enabled = true;

        #[rustfmt::skip]
        assert_eq!(
            preprocessor.preprocess("Hackers doing hacking", Some(Lang::Eng)).normalized_text(),
            "hacker do hack"
        );

        // NOTE: This works because of automatic language detection.
        #[rustfmt::skip]
        assert_eq!(
            preprocessor.preprocess("Hackers doing hacking", None).normalized_text(),
            "hacker do hack"
        );
    }

    #[test]
    fn test_preprocessor_can_unique_tokens() {
        let mut preprocessor = Preprocessor::default();

        // Disable stopword filtering as it would influence results.
        preprocessor.filter_stopwords = false;

        #[rustfmt::skip]
        assert_eq!(
            UniqueBy::new(
                preprocessor.preprocess("This and this then that and that", None).tokens(),
                Token::hash
            )
            .map(Token::into_normalized)
            .collect::<Vec<_>>(),
            ["this", "and", "then", "that"]
        );
    }

    /// Ensures the preprocessor can detect special patterns, and normalizes
    /// some of them.
    #[test]
    fn test_preprocessor_can_detect_patterns() {
        fn test(sentence: &str, expected: &[(&str, TokenKind)]) {
            let mut preprocessor = Preprocessor::default();
            preprocessor
                .tokenization_config
                .compat_split_special_patterns = false;
            preprocessor.detect_stopwords = false;

            let output = preprocessor.preprocess(sentence, Some(Lang::Eng));
            let tokens = (output.tokens())
                // .inspect(|t| eprintln!("{t:?}"))
                .take(256) // Breaks potential infinite loop.
                .map(|token| (token.normalized, token.kind))
                .collect::<Vec<_>>();

            assert_eq!(tokens, expected, "{sentence:?}");
        }

        // Email address.
        #[rustfmt::skip]
        test(
            "Contact jane.doe@example.org, alice@example.org or bob+foo@example.org for support.",
            &[
                ("contact", TokenKind::Normal),
                ("jane.doe@example.org", TokenKind::Special(SpecialTokenKind::EmailAddress)),
                ("alice@example.org", TokenKind::Special(SpecialTokenKind::EmailAddress)),
                ("or", TokenKind::Normal),
                ("bob+foo@example.org", TokenKind::Special(SpecialTokenKind::EmailAddress)),
                ("for", TokenKind::Normal),
                ("support", TokenKind::Normal),
            ],
        );

        // Phone number like.
        #[rustfmt::skip]
        test(
            "You can also call me at 555-123-4567 or +33 6 12 34 56 78 (06.12.34.56.78 / 06 12 34 56 78).",
            &[
                ("you", TokenKind::Normal),
                ("can", TokenKind::Normal),
                ("also", TokenKind::Normal),
                ("call", TokenKind::Normal),
                ("me", TokenKind::Normal),
                ("at", TokenKind::Normal),
                ("5551234567", TokenKind::Special(SpecialTokenKind::PhoneNumber)),
                ("or", TokenKind::Normal),
                ("+33612345678", TokenKind::Special(SpecialTokenKind::PhoneNumber)),
                ("0612345678", TokenKind::Special(SpecialTokenKind::PhoneNumber)),
                ("0612345678", TokenKind::Special(SpecialTokenKind::PhoneNumber)),
            ],
        );

        // UUID like.
        #[rustfmt::skip]
        test(
            "My account is 6db14cb4-b82e-4e49-8016-ef76c4290a2f.",
            &[
                ("my", TokenKind::Normal),
                ("account", TokenKind::Normal),
                ("is", TokenKind::Normal),
                ("6db14cb4-b82e-4e49-8016-ef76c4290a2f", TokenKind::Special(SpecialTokenKind::Id)),
            ],
        );

        // Hash like.
        #[rustfmt::skip]
        test(
            "Check out b244423d417369795292e9f4530d0c0e6fa07625 and 927ff7701795282232dda41e023c7c6ba29d5a15 (927ff77).",
            &[
                ("check", TokenKind::Normal),
                ("out", TokenKind::Normal),
                ("b244423d417369795292e9f4530d0c0e6fa07625", TokenKind::Special(SpecialTokenKind::Id)),
                ("and", TokenKind::Normal),
                ("927ff7701795282232dda41e023c7c6ba29d5a15", TokenKind::Special(SpecialTokenKind::Id)),
                ("927ff77", TokenKind::Special(SpecialTokenKind::Id)),
            ],
        );

        // URL.
        #[rustfmt::skip]
        test(
            "Have a look at https://example.org/foo?id=123.",
            &[
                ("have", TokenKind::Normal),
                ("a", TokenKind::Normal),
                ("look", TokenKind::Normal),
                ("at", TokenKind::Normal),
                ("https://example.org/foo?id=123", TokenKind::Special(SpecialTokenKind::Url)),
            ],
        );

        // Domain name.
        #[rustfmt::skip]
        test(
            "My domain name is example.org.",
            &[
                ("my", TokenKind::Normal),
                ("domain", TokenKind::Normal),
                ("name", TokenKind::Normal),
                ("is", TokenKind::Normal),
                ("example.org", TokenKind::Special(SpecialTokenKind::Domain)),
            ],
        );
        #[rustfmt::skip]
        test(
            "I don’t put punctuation correctly .See?",
            &[
                ("i", TokenKind::Normal),
                ("don’t", TokenKind::Normal),
                ("put", TokenKind::Normal),
                ("punctuation", TokenKind::Normal),
                ("correctly", TokenKind::Normal),
                ("see", TokenKind::Normal),
            ],
        );

        // IP addresses.
        #[rustfmt::skip]
        test(
            "Try to ping 192.168.1.0, 0.0.0.0, 2606:4700::6812:1c68, or ::1.",
            &[
                ("try", TokenKind::Normal),
                ("to", TokenKind::Normal),
                ("ping", TokenKind::Normal),
                ("192.168.1.0", TokenKind::Special(SpecialTokenKind::Ipv4)),
                ("0.0.0.0", TokenKind::Special(SpecialTokenKind::Ipv4)),
                ("2606:4700::6812:1c68", TokenKind::Special(SpecialTokenKind::Id)),
                ("or", TokenKind::Normal),
                ("::1", TokenKind::Special(SpecialTokenKind::Id)),
            ],
        );

        // Username.
        #[rustfmt::skip]
        test(
            "Contact @alice.",
            &[
                ("contact", TokenKind::Normal),
                ("@alice", TokenKind::Special(SpecialTokenKind::Username)),
            ],
        );

        // Code like.
        #[rustfmt::skip]
        test(
            "It’s tested in test_tokenizer.",
            &[
                ("it’s", TokenKind::Normal),
                ("tested", TokenKind::Normal),
                ("in", TokenKind::Normal),
                ("test_tokenizer", TokenKind::Special(SpecialTokenKind::Id)),
            ],
        );
    }
}

pub mod preprocessor {
    use std::cell::OnceCell;
    use std::ops::RangeInclusive;
    use std::rc::Rc;

    use whatlang::Lang;

    use super::lang_detection::detect_lang;
    use super::lexing::{Lexer, TokenKind};
    use super::normalization::{Normalizer, Stemmer};
    use crate::config::{ConfigNormalization, ConfigStopwords, ConfigTokenization};
    use crate::lexer::stemming;
    use crate::lexer::stopwords::is_stopword;
    use crate::store::StoreTermHash;

    pub struct Preprocessor {
        pub tokenization_config: ConfigTokenization,
        pub normalization_config: ConfigNormalization,
        pub stopwords_config: ConfigStopwords,
        pub detect_stopwords: bool,
        pub filter_stopwords: bool,
    }

    impl Preprocessor {
        pub fn new(
            tokenization_config: ConfigTokenization,
            normalization_config: ConfigNormalization,
            stopwords_config: ConfigStopwords,
            detect_stopwords: bool,
            filter_stopwords: bool,
        ) -> Self {
            Self {
                tokenization_config,
                normalization_config,
                stopwords_config,
                detect_stopwords,
                filter_stopwords,
            }
        }

        pub fn preprocess<'t>(&self, text: &'t str, lang: Option<Lang>) -> PreprocessorOutput<'t> {
            let lang = match lang {
                // If user provided a language, use it.
                Some(hinted_lang) => {
                    // Use hinted language (current lexer mode asks for a cleanup).
                    tracing::debug!(?text, "using hinted lang: {hinted_lang:?}");

                    lang
                }

                // If user asked to cleanup, detect the language.
                None if self.filter_stopwords => {
                    let lang = detect_lang(text);
                    tracing::debug!(?text, "detected lang: {lang:?}");
                    lang
                }

                // If user asked not to cleanup but stemming is enabled,
                // detect the language.
                #[cfg(feature = "stemming")]
                None if self.normalization_config.stemming_enabled => {
                    let lang = detect_lang(text);
                    tracing::debug!(?text, "detected lang: {lang:?}");
                    lang
                }

                // Otherwise, don’t detect the language.
                None => {
                    tracing::debug!("not detecting lang");

                    None
                }
            };

            // PERF: By using Unicode Normalization Form KC, we are pretty
            //   sure the normalized text won’t exceed `text.len()`. Because
            //   we won’t store spaces nor stopwords, we might end up with a
            //   lot of unused space. However, it’s better to waste a few bytes
            //   than to re-allocate.
            // NOTE: We do `+ 1` because we store spaces in normalized text
            //   (for more useful printing) but if the input is space-separated
            //   ASCII then the last pushed space character would cause a
            //   useless re-allocation (since we pop it right after).
            let mut text_normalized = String::with_capacity(text.len() + 1);
            // PERF: It’s better to initialize `spans` with a low capacity
            //   than the default `0` for `Vec::new`. We will fill it anyway.
            let mut spans = Vec::<TokenSpan>::with_capacity(text.len() / 8);

            let lexer = Lexer::new(self.tokenization_config);

            let normalizer = Normalizer::new(self.normalization_config);

            // Choose the stemming algorithm once
            let stemming_algorithm: OnceCell<Option<Stemmer>> = OnceCell::new();

            'tokenization: for (index, mut token) in lexer.lex(text, lang).enumerate() {
                let (start_normalized, end_normalized) =
                    normalizer.normalize(&token, &mut text_normalized);

                let normalized = &text_normalized[start_normalized..end_normalized];

                // Check if word is a stopword.
                if self.detect_stopwords && is_stopword(normalized, lang, &self.stopwords_config) {
                    if self.filter_stopwords {
                        // Remove normalized word from normalized text as it won’t
                        // be used.
                        text_normalized.truncate(start_normalized);

                        continue 'tokenization;
                    } else {
                        token.kind = TokenKind::Stopword;
                    }
                }

                let mut span = TokenSpan {
                    start_original: token.start,
                    end_original: token.start + token.raw.len(),
                    start_normalized,
                    end_normalized,
                    kind: token.kind,
                    // NOTE: Index takes stopwords into account, even if skipped.
                    index,
                    hash: Rc::default(),
                };

                // Stemming
                if self.normalization_config.stemming_enabled
                    && let Some(stemmer) = stemming_algorithm.get_or_init(|| match lang {
                        Some(ref lang) => stemming::snowball_algorithm(lang).map(Stemmer::new),
                        None => None,
                    })
                {
                    stemmer.stem(&mut span, &mut text_normalized);
                }

                spans.push(span);

                // Push a whitespace so normalized text is easier to read.
                // It’s not required at all, but it makes it easier for humans
                // to read normalized text if printed.
                text_normalized.push(' ');
            }

            // Remove trailing whitespace.
            _ = text_normalized.pop();

            PreprocessorOutput {
                text_original: text,
                text_normalized,
                spans,
            }
        }
    }

    impl Default for Preprocessor {
        fn default() -> Self {
            Self {
                tokenization_config: ConfigTokenization {
                    detect_special_patterns: true,
                    compat_split_special_patterns: true,
                },
                normalization_config: ConfigNormalization {
                    unicode_normalization: None,
                    diacritic_folding_enabled: false,
                    stemming_enabled: false,
                },
                stopwords_config: ConfigStopwords::default(),
                detect_stopwords: true,
                filter_stopwords: true,
            }
        }
    }

    /// Cache-optimized storage for tokenized text.
    ///
    /// See <https://youtu.be/ryfbBB3pHfI?si=wJAqA8wNYmWbaEh1> regarding why
    /// this structure is more performant than a `Vec` of tokens.
    pub struct PreprocessorOutput<'s> {
        text_original: &'s str,
        text_normalized: String,
        spans: Vec<TokenSpan>,
    }

    impl<'s> PreprocessorOutput<'s> {
        pub fn tokens(&'s self) -> TokensIter<'s> {
            TokensIter {
                spans: self.spans.iter(),
                text_original: self.text_original,
                text_normalized: self.text_normalized.as_str(),
            }
        }

        pub fn original_text(&'s self) -> &'s str {
            self.text_original
        }

        pub fn normalized_text(&'s self) -> &'s str {
            self.text_normalized.as_str()
        }
    }

    pub struct TokenSpan {
        start_original: usize,
        end_original: usize,
        pub start_normalized: usize,
        pub end_normalized: usize,
        kind: TokenKind,
        index: usize,
        hash: Rc<OnceCell<StoreTermHash>>,
    }

    /// Iterator over [`Tokens`].
    pub struct TokensIter<'s> {
        text_original: &'s str,
        text_normalized: &'s str,
        spans: std::slice::Iter<'s, TokenSpan>,
    }

    impl<'s> Iterator for TokensIter<'s> {
        type Item = Token<'s>;

        fn next(&mut self) -> Option<Self::Item> {
            self.spans.next().map(|span| Token {
                original: &self.text_original[span.start_original..span.end_original],
                normalized: &self.text_normalized[span.start_normalized..span.end_normalized],
                kind: span.kind,
                start: span.start_original,
                end: span.end_original,
                index_in_tokenized_text: span.index,
                hash: Rc::clone(&span.hash),
            })
        }

        fn size_hint(&self) -> (usize, Option<usize>) {
            self.spans.size_hint()
        }
    }

    impl<'s> ExactSizeIterator for TokensIter<'s> {
        fn len(&self) -> usize {
            self.spans.len()
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Token<'s> {
        original: &'s str,

        pub(super) normalized: &'s str,

        /// Marker to differenciate special tokens (like email addresses), which
        /// should be processed differently, (like disabling fuzzy matching
        /// during search queries).
        pub(super) kind: TokenKind,

        /// Start index (byte) in original text.
        pub(super) start: usize,

        /// Start index (byte) in original text.
        pub(super) end: usize,

        /// Index of the token in the tokenized text (stopwords included).
        ///
        /// To get the index without considering stopwords, use
        /// [`core::iter::Iterator::enumerate`].
        pub(super) index_in_tokenized_text: usize,

        hash: Rc<OnceCell<StoreTermHash>>,
    }

    impl<'s> Token<'s> {
        pub fn into_original(self) -> &'s str {
            self.original
        }

        pub fn into_normalized(self) -> &'s str {
            self.normalized
        }

        pub fn into_kind(self) -> TokenKind {
            self.kind
        }

        pub const fn as_original(&self) -> &str {
            self.original
        }

        pub const fn as_normalized(&self) -> &str {
            self.normalized
        }

        pub const fn kind(&self) -> &TokenKind {
            &self.kind
        }

        #[inline]
        pub const fn is_special(&self) -> bool {
            matches!(self.kind, TokenKind::Special(_))
        }

        #[inline]
        pub const fn is_stopword(&self) -> bool {
            matches!(self.kind, TokenKind::Stopword)
        }

        #[inline]
        pub const fn is_not_stopword(&self) -> bool {
            !self.is_stopword()
        }

        pub const fn range(&self) -> RangeInclusive<usize> {
            self.start..=self.end
        }

        /// Hash of the **normalized** version of the token.
        pub fn hash(&self) -> StoreTermHash {
            *(self.hash).get_or_init(|| StoreTermHash::from(self.normalized))
        }

        /// Hash of the **normalized** version of the token.
        pub fn into_hash(self) -> StoreTermHash {
            *(self.hash).get_or_init(|| StoreTermHash::from(self.normalized))
        }
    }
}

pub mod lexing {
    use std::{iter::Peekable, sync::LazyLock};

    use regex::Regex;
    use whatlang::Lang;

    use crate::config::ConfigTokenization;

    #[cfg(feature = "tokenizer-chinese")]
    static TOKENIZER_JIEBA: LazyLock<jieba_rs::Jieba> = LazyLock::new(jieba_rs::Jieba::new);

    #[cfg(feature = "tokenizer-japanese")]
    static TOKENIZER_LINDERA: LazyLock<lindera_tokenizer::tokenizer::Tokenizer> =
        LazyLock::new(|| {
            lindera_tokenizer::tokenizer::Tokenizer::from_config(
                lindera_tokenizer::tokenizer::TokenizerConfig {
                    dictionary: lindera_dictionary::DictionaryConfig {
                        kind: Some(lindera_dictionary::DictionaryKind::UniDic),
                        path: None,
                    },
                    user_dictionary: None,
                    mode: lindera_core::mode::Mode::Normal,
                },
            )
            .expect("unable to initialize japanese tokenizer")
        });

    /// Splits text into tokens, depending on language.
    pub struct Tokenizer {
        lang: Option<Lang>,
    }

    impl Tokenizer {
        fn tokenize<'s>(&self, text: &'s str) -> Box<dyn Iterator<Item = (usize, &'s str)> + 's> {
            use unicode_segmentation::UnicodeSegmentation as _;

            match self.lang {
                #[cfg(feature = "tokenizer-chinese")]
                Some(Lang::Cmn) => Box::from(
                    TOKENIZER_JIEBA
                        .cut(text, false)
                        .into_iter()
                        .map(|token| (token.start, token.word)),
                ),
                #[cfg(feature = "tokenizer-japanese")]
                Some(Lang::Jpn) => match TOKENIZER_LINDERA.tokenize(text) {
                    Ok(tokens) => Box::from(
                        tokens
                            .into_iter()
                            .map(|token| (token.token_start, token.text)),
                    ),
                    Err(err) => {
                        tracing::warn!("unable to tokenize japanese, falling back: {}", err);

                        Box::from(text.unicode_word_indices())
                    }
                },
                _ => Box::from(text.unicode_word_indices()),
            }
        }
    }

    #[test]
    fn test_tokenizer_lang_none() {
        let tokenizer = Tokenizer { lang: None };

        assert_eq!(
            tokenizer
                .tokenize("This is an example.")
                .map(|(_index, token)| token)
                .collect::<Vec<_>>(),
            vec!["This", "is", "an", "example"],
        );
    }

    #[test]
    fn test_tokenizer_latin() {
        let tokenizer = Tokenizer {
            lang: Some(Lang::Eng),
        };

        assert_eq!(
            tokenizer
                .tokenize("This is an example.")
                .map(|(_index, token)| token)
                .collect::<Vec<_>>(),
            ["This", "is", "an", "example"],
        );
    }

    #[cfg(feature = "tokenizer-chinese")]
    #[test]
    fn test_tokenizer_cmn() {
        let tokenizer = Tokenizer {
            lang: Some(Lang::Cmn),
        };

        assert_eq!(
            tokenizer
                .tokenize("我来到北京清华大学")
                .map(|(_index, token)| token)
                .collect::<Vec<_>>(),
            ["我", "来到", "北京", "清华大学"],
        );
    }

    #[cfg(feature = "tokenizer-japanese")]
    #[test]
    fn test_tokenizer_jpn() {
        let tokenizer = Tokenizer {
            lang: Some(Lang::Jpn),
        };

        assert_eq!(
            tokenizer
                .tokenize("関西国際空港限定トートバッグ")
                .map(|(_index, token)| token)
                .collect::<Vec<_>>(),
            ["関西国際空港", "限定", "トートバッグ"],
        );
    }

    /// Detects special patterns and splits text accordingly.
    ///
    /// Uses [`Tokenizer`] internally.
    pub struct Lexer {
        config: ConfigTokenization,
    }

    impl Lexer {
        pub fn new(config: ConfigTokenization) -> Self {
            Self { config }
        }

        pub fn lex<'s>(&self, text: &'s str, lang: Option<Lang>) -> LexerTokens<'s> {
            // FIXME(major): Don’t allow trailing dot in `email`.
            // FIXME(major): Allow `()` in `phone` (remember to update normalizer).
            // TODO(major): Test that numbers in various scripts are all detected as special.
            static SPECIAL_PATTERNS: LazyLock<Regex> = LazyLock::new(|| {
                Regex::new(concat!(
                    r"(?P<email>[\w.+-]+@[\w-]+\.[\w.-]+)",
                    r"|(?P<username>@[^\s]*\w)",
                    r"|(?P<url>\w{2,}://[^\s]*[^\s.])",
                    r"|(?P<ipv4>\d{1,3}(?:\.\d{1,3}){3})(?:[^\.\d]|$)",
                    r"|(?P<phone>\+?\d+(?:[\s\.-]?\d+){4,})",
                    r"|(?P<domain>[a-zA-Z0-9.-]+\.[a-zA-Z]{2,})",
                    r"|(?P<id>[\w\d:_-]*[\d_][\w\d:-]*)"
                ))
                .unwrap()
            });

            let regex_matches = if self.config.detect_special_patterns {
                SPECIAL_PATTERNS.captures_iter(text).peekable()
            } else {
                // NOTE: It’s not truly an no-op but it is if we try matching a non-empty line.
                static NOOP_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^$").unwrap());
                NOOP_REGEX.captures_iter(" ").peekable()
            };

            LexerTokens {
                compat_split_special_patterns: self.config.compat_split_special_patterns,
                tokenizer: Tokenizer { lang },
                regex_matches,
                text,
                regex_cursor: 0,
                tokens: None,
            }
        }
    }

    pub struct LexerTokens<'s> {
        compat_split_special_patterns: bool,
        text: &'s str,
        tokenizer: Tokenizer,
        regex_matches: Peekable<regex::CaptureMatches<'static, 's>>,
        regex_cursor: usize,
        tokens: Option<(Box<dyn Iterator<Item = LexerToken<'s>> + 's>, usize)>,
    }

    impl<'s> Iterator for LexerTokens<'s> {
        type Item = LexerToken<'s>;

        fn next(&mut self) -> Option<Self::Item> {
            // If we were walking words, continue.
            if let Some((tokens, end)) = self.tokens.as_mut() {
                match tokens.next() {
                    Some(token) => return Some(token),
                    None => {
                        self.regex_cursor = *end;
                        self.tokens = None;
                    }
                }
            }

            // Check where the next special chunk is located.
            match self.regex_matches.peek() {
                Some(captures) => {
                    let regex_match = captures.get_match();
                    let start = regex_match.start();
                    let end = regex_match.end();

                    // Up until that special chunk, tokenize normally.
                    if start > self.regex_cursor {
                        let gap = &self.text[self.regex_cursor..start];
                        let mut tokens = (self.tokenizer.tokenize(gap))
                            // Map to normal token but also map local index
                            // to global index.
                            .map({
                                let global_start = self.regex_cursor;
                                move |(local_start, raw)| {
                                    LexerToken::normal(global_start + local_start, raw)
                                }
                            });

                        if let Some(token) = tokens.next() {
                            self.tokens = Some((Box::new(tokens), end));
                            return Some(token);
                        }
                    }

                    // Once all normal words have been visited, yield the special chunk
                    // (or sub-split if `compat_split_special_patterns` is enabled).
                    let next = if self.compat_split_special_patterns {
                        let regex_match = captures.get_match();
                        let raw_tokens = self.tokenizer.tokenize(regex_match.as_str());

                        let mut tokens = raw_tokens.map({
                            let global_start = regex_match.start();

                            move |(local_start, raw)| LexerToken {
                                start: global_start + local_start,
                                raw,
                                kind: TokenKind::Special(SpecialTokenKind::CompatSubtoken),
                            }
                        });

                        let next = tokens.next().unwrap_or(LexerToken {
                            start: regex_match.start(),
                            raw: regex_match.as_str(),
                            kind: TokenKind::Special(SpecialTokenKind::CompatSubtoken),
                        });

                        self.tokens = Some((Box::new(tokens), end));

                        Some(next)
                    } else {
                        Some(LexerToken::special(captures))
                    };

                    // Advance the iterator now that we’ve visited all previous
                    // tokens.
                    self.regex_matches.next();
                    self.regex_cursor = end;

                    next
                }
                None => {
                    // When there are no more special chunks, finish by
                    // tokenizing normally.
                    let gap = &self.text[self.regex_cursor..];
                    let mut tokens = (self.tokenizer.tokenize(gap))
                        // Map to normal token but also map local index
                        // to global index.
                        .map({
                            let global_start = self.regex_cursor;
                            move |(local_start, raw)| {
                                LexerToken::normal(global_start + local_start, raw)
                            }
                        });

                    if let Some(token) = tokens.next() {
                        self.tokens = Some((Box::from(tokens), self.text.len()));
                        return Some(token);
                    }

                    None
                }
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum TokenKind {
        Normal,
        Special(SpecialTokenKind),
        Stopword,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum SpecialTokenKind {
        EmailAddress,
        Username,
        Url,
        Ipv4,
        PhoneNumber,
        Domain,
        Id,
        /// Special token created by `compat_split_special_patterns`. Should
        /// still be considered special (e.g. disabling fuzzy matching), but
        /// has no special meaning anymore.
        CompatSubtoken,
    }

    #[derive(Debug, PartialEq, Eq)]
    pub struct LexerToken<'s> {
        pub start: usize,
        pub raw: &'s str,
        pub kind: TokenKind,
    }

    impl<'s> LexerToken<'s> {
        #[inline]
        fn normal(start: usize, raw: &'s str) -> Self {
            Self {
                start,
                raw,
                kind: TokenKind::Normal,
            }
        }

        #[inline]
        fn special(captures: &regex::Captures<'s>) -> Self {
            let (m, kind) = if let Some(m) = captures.name("email") {
                (m, SpecialTokenKind::EmailAddress)
            } else if let Some(m) = captures.name("username") {
                (m, SpecialTokenKind::Username)
            } else if let Some(m) = captures.name("url") {
                (m, SpecialTokenKind::Url)
            } else if let Some(m) = captures.name("ipv4") {
                (m, SpecialTokenKind::Ipv4)
            } else if let Some(m) = captures.name("phone") {
                (m, SpecialTokenKind::PhoneNumber)
            } else if let Some(m) = captures.name("domain") {
                (m, SpecialTokenKind::Domain)
            } else if let Some(m) = captures.name("id") {
                (m, SpecialTokenKind::Id)
            } else {
                unreachable!("One name always matches")
            };

            Self {
                start: m.start(),
                raw: m.as_str(),
                kind: TokenKind::Special(kind),
            }
        }

        #[allow(dead_code)]
        #[inline]
        fn stopword(start: usize, raw: &'s str) -> Self {
            Self {
                start,
                raw,
                kind: TokenKind::Stopword,
            }
        }

        pub fn end(&self) -> usize {
            self.start + self.raw.len()
        }
    }
}

mod normalization {
    use super::lexing::{LexerToken, SpecialTokenKind, TokenKind};
    use super::preprocessor::TokenSpan;
    use crate::config::{ConfigNormalization, UnicodeNormalization};

    pub(super) struct Normalizer {
        normalization_config: ConfigNormalization,
    }

    impl Normalizer {
        pub(super) fn new(normalization_config: ConfigNormalization) -> Self {
            Self {
                normalization_config,
            }
        }

        pub(super) fn normalize(
            &self,
            token: &LexerToken,
            text_normalized: &mut String,
        ) -> (usize, usize) {
            use unicode_normalization::UnicodeNormalization as _;
            use unicode_normalization::char::is_combining_mark;

            let start_normalized = text_normalized.len();

            match token.kind {
                TokenKind::Normal | TokenKind::Stopword => {
                    // Case folding
                    let chars = caseless::Caseless::default_case_fold(token.raw.chars());

                    match (
                        self.normalization_config.unicode_normalization,
                        self.normalization_config.diacritic_folding_enabled,
                    ) {
                        (None, false) => {
                            for char in chars {
                                text_normalized.push(char);
                            }
                        }

                        // Unicode normalization
                        (Some(normalization), false) => {
                            let chars = match normalization {
                                UnicodeNormalization::Nfc => chars.nfc(),
                                UnicodeNormalization::Nfkc => chars.nfkc(),
                            };

                            for char in chars {
                                text_normalized.push(char);
                            }
                        }

                        // Diacritic folding
                        (None, true) => {
                            for char in chars {
                                for char in char.nfd().filter(|c| !is_combining_mark(*c)) {
                                    text_normalized.push(char);
                                }
                            }
                        }

                        // Unicode normalization + diacritic folding
                        (Some(normalization), true) => {
                            // Diacritic folding
                            // NOTE: Perform first as it makes the text end up in a
                            //   normal form that might not be what the user wants.
                            let chars = chars.nfd().filter(|c| !is_combining_mark(*c));

                            let chars = match normalization {
                                UnicodeNormalization::Nfc => chars.nfc(),
                                UnicodeNormalization::Nfkc => chars.nfkc(),
                            };

                            for char in chars {
                                text_normalized.push(char);
                            }
                        }
                    }
                }
                TokenKind::Special(SpecialTokenKind::PhoneNumber) => {
                    for char in token
                        .raw
                        .chars()
                        .filter(|c| c.is_ascii_digit() || *c == '+')
                    {
                        text_normalized.push(char);
                    }
                }
                TokenKind::Special(_) => {
                    text_normalized.push_str(token.raw);
                }
            }

            (start_normalized, text_normalized.len())
        }
    }

    pub(super) struct Stemmer {
        algorithm: snowball::Algorithm,
    }

    impl Stemmer {
        pub(super) fn new(algorithm: snowball::Algorithm) -> Self {
            Self { algorithm }
        }

        pub(super) fn stem(&self, span: &mut TokenSpan, text_normalized: &mut String) {
            match (self.algorithm.stemmer())
                .stem(&text_normalized[span.start_normalized..span.end_normalized])
            {
                std::borrow::Cow::Borrowed(_) => { /* Nothing to do */ }
                std::borrow::Cow::Owned(new_word) => {
                    // Replace normalized word by new normalization.
                    text_normalized.truncate(span.start_normalized);
                    text_normalized.push_str(new_word.as_ref());
                    span.end_normalized = text_normalized.len();
                }
            }
        }
    }
}

pub mod itertools {
    /// A wrapper iterator that deduplicates elements.
    pub struct UniqueBy<I: Iterator, F, T, H> {
        inner: I,
        seen: std::collections::HashSet<T, H>,
        map: F,
    }

    impl<T, U, I, F> UniqueBy<I, F, U, std::hash::RandomState>
    where
        U: Eq + std::hash::Hash + Copy,
        I: Iterator<Item = T>,
        F: Fn(&T) -> U,
    {
        pub fn new(inner: I, map: F) -> Self {
            Self {
                seen: std::collections::HashSet::with_capacity(inner.size_hint().0),
                inner,
                map,
            }
        }
    }

    impl<T, U, I, F, H> UniqueBy<I, F, U, H>
    where
        U: Eq + std::hash::Hash + Copy,
        I: Iterator<Item = T>,
        F: Fn(&T) -> U,
    {
        pub fn new_with_hasher(inner: I, map: F, hasher: H) -> UniqueBy<I, F, U, H> {
            UniqueBy {
                seen: std::collections::HashSet::with_capacity_and_hasher(
                    inner.size_hint().0,
                    hasher,
                ),
                inner,
                map,
            }
        }

        pub fn seen(&self) -> &std::collections::HashSet<U, H> {
            &self.seen
        }
    }

    impl<T, U, I, F, H> Iterator for UniqueBy<I, F, U, H>
    where
        U: Eq + std::hash::Hash + Copy,
        I: Iterator<Item = T>,
        F: Fn(&T) -> U,
        H: std::hash::BuildHasher,
    {
        type Item = T;

        #[allow(clippy::manual_find, reason = "Readability")]
        fn next(&mut self) -> Option<Self::Item> {
            for next in self.inner.by_ref() {
                if self.seen.insert((self.map)(&next)) {
                    return Some(next);
                }
            }

            None
        }

        fn size_hint(&self) -> (usize, Option<usize>) {
            let (min, max) = self.inner.size_hint();
            // PERF: By using `min / 2`, we benefit from more allocation upfront,
            //   without allocating as much as `min`, and we know for sure there
            //   will be at most 1 reallocation if the inner iterator contains
            //   more than `min / 2` unique items. It seems like a sweet spot.
            (min / 2, max)
        }
    }
}

pub mod to_rework {
    use crate::executor::QueryGenericLang;

    #[derive(PartialEq)]
    pub enum TokenLexerMode {
        NormalizeAndCleanup,
        NormalizeOnly,
    }

    impl TokenLexerMode {
        pub fn should_cleanup(&self) -> bool {
            match self {
                Self::NormalizeAndCleanup => true,
                Self::NormalizeOnly => false,
            }
        }
    }

    impl TokenLexerMode {
        pub fn from_query_lang(lang: &Option<QueryGenericLang>) -> TokenLexerMode {
            match lang {
                Some(QueryGenericLang::Enabled(_)) => {
                    // Cleanup with provided language
                    TokenLexerMode::NormalizeAndCleanup
                }
                Some(QueryGenericLang::Disabled) => {
                    // Normalize only (language purposefully set to 'none')
                    TokenLexerMode::NormalizeOnly
                }
                None => {
                    // Auto-detect language and cleanup (this is the default behavior)
                    TokenLexerMode::NormalizeAndCleanup
                }
            }
        }
    }
}

// TODO: Migrate language detection tests from old tokenizer’s `token.rs`?
mod lang_detection {
    use std::time::Instant;

    use whatlang::Lang;

    use crate::lexer::stopwords::LexerStopWord;

    const TEXT_LANG_TRUNCATE_OVER_CHARS: usize = 200;
    const TEXT_LANG_DETECT_PROCEED_OVER_CHARS: usize = 20;
    const TEXT_LANG_DETECT_NGRAM_UNDER_CHARS: usize = 60;

    pub(super) fn detect_lang(text: &str) -> Option<Lang> {
        tracing::debug!("detecting locale from lexer text: {}", text);

        // Detect only if text is long-enough to allow the text locale detection system to \
        //   function properly
        if text.len() < TEXT_LANG_DETECT_PROCEED_OVER_CHARS {
            return None;
        }

        // Truncate text if necessary, as to avoid the ngram or stopwords detector to be \
        //   ran on more words than those that are enough to reliably detect a locale.
        let safe_text = if text.len() > TEXT_LANG_TRUNCATE_OVER_CHARS {
            tracing::debug!(
                "lexer text needs to be truncated, as it is too long ({}/{}): {}",
                text.len(),
                TEXT_LANG_TRUNCATE_OVER_CHARS,
                text
            );

            // Perform an UTF-8 aware truncation
            let end_index = text.floor_char_boundary(TEXT_LANG_TRUNCATE_OVER_CHARS);
            &text[..end_index]
        } else {
            text
        };

        tracing::debug!("will detect locale for lexer safe text: {}", safe_text);

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
            tracing::debug!(
                "lexer text is shorter than {} characters, using the slow method",
                TEXT_LANG_DETECT_NGRAM_UNDER_CHARS
            );

            detect_lang_slow(safe_text)
        } else {
            tracing::debug!(
                "lexer text is equal or longer than {} characters, using the fast method",
                TEXT_LANG_DETECT_NGRAM_UNDER_CHARS
            );

            detect_lang_fast(safe_text)
        }
    }

    fn detect_lang_slow(safe_text: &str) -> Option<Lang> {
        let ngram_start = Instant::now();

        match whatlang::detect(safe_text) {
            Some(info) => {
                let ngram_took = ngram_start.elapsed();

                let mut locale = info.lang();

                tracing::info!(
                    "[slow lexer] locale detected from text: {} ({} from {} at {}/1; {}s + {}ms)",
                    safe_text,
                    locale,
                    info.script(),
                    info.confidence(),
                    ngram_took.as_secs(),
                    ngram_took.subsec_millis()
                );

                // Confidence is low, try to detect locale from stop-words.
                // Notice: this is a fallback but should not be too reliable for short \
                //   texts.
                if !info.is_reliable() {
                    tracing::debug!("[slow lexer] trying to detect locale from stopwords instead");

                    // Better alternate locale found?
                    if let Some(alternate_locale) =
                        LexerStopWord::guess_lang(safe_text, info.script())
                    {
                        tracing::info!(
                            "[slow lexer] detected more accurate locale from stopwords: {}",
                            alternate_locale
                        );

                        locale = alternate_locale;
                    }
                }

                Some(locale)
            }
            None => {
                tracing::info!(
                    "[slow lexer] no locale could be detected from text: {}",
                    safe_text
                );

                None
            }
        }
    }

    fn detect_lang_fast(safe_text: &str) -> Option<Lang> {
        let stopwords_start = Instant::now();

        match whatlang::detect_script(safe_text) {
            Some(script) => {
                // Locale found?
                if let Some(locale) = LexerStopWord::guess_lang(safe_text, script) {
                    let stopwords_took = stopwords_start.elapsed();

                    tracing::info!(
                        "[fast lexer] locale detected from text: {} ({}; {}s + {}ms)",
                        safe_text,
                        locale,
                        stopwords_took.as_secs(),
                        stopwords_took.subsec_millis()
                    );

                    Some(locale)
                } else {
                    tracing::debug!(
                        "[fast lexer] trying to detect locale from fallback ngram instead"
                    );

                    // No locale found, fallback on slow ngram.
                    whatlang::detect_lang(safe_text)
                }
            }
            None => {
                tracing::info!(
                    "[fast lexer] no script could be detected from text: {}",
                    safe_text
                );

                None
            }
        }
    }
}
