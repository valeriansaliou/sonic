// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::borrow::Cow;
use std::iter::Peekable;
use std::sync::LazyLock;

use charabia::{Language as CharabiaLanguage, Segment};
use hashbrown::HashSet;
use regex::Regex;
use unicode_segmentation::{GraphemeIndices, UnicodeSegmentation};
use whatlang::Lang;

use crate::config::{ConfigNormalization, ConfigTokenization};
use crate::query::QueryGenericLang;

pub struct TokenLexerBuilder;

struct WordToken<'s> {
    raw: &'s str,
    language: Option<Lang>,
}

struct PendingToken<'s> {
    token: Token<'s>,
    language: Option<Lang>,
}

type WordsIter<'s> = Box<dyn Iterator<Item = WordToken<'s>> + 's>;
type TokensIter<'s> = Box<dyn Iterator<Item = PendingToken<'s>> + 's>;

struct EmojiSeparated<'s> {
    text: &'s str,
    graphemes: Option<GraphemeIndices<'s>>,
    start: usize,
    finished: bool,
}

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

pub struct Tokenizer<'s> {
    config: ConfigTokenization,
    text: &'s str,
    lang: Option<Lang>,
    regex_matches: Peekable<regex::CaptureMatches<'static, 's>>,
    regex_cursor: usize,
    tokens: Option<(TokensIter<'s>, usize)>,
    last_language: Option<Lang>,
}

impl<'s> Tokenizer<'s> {
    fn new(text: &'s str, lang: Option<Lang>, config: &ConfigTokenization) -> Self {
        let regex_matches = if config.detect_special_patterns {
            SPECIAL_PATTERNS.captures_iter(text).peekable()
        } else {
            // NOTE: It’s not truly an no-op but it is if we try matching a non-empty line.
            static NOOP_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^$").unwrap());
            NOOP_REGEX.captures_iter(" ").peekable()
        };

        Self {
            config: *config,
            lang,
            regex_matches,
            text,
            regex_cursor: 0,
            tokens: None,
            last_language: None,
        }
    }
}

impl<'s> EmojiSeparated<'s> {
    fn new(text: &'s str) -> Self {
        Self {
            text,
            graphemes: (!text.is_ascii()).then(|| text.grapheme_indices(true)),
            start: 0,
            finished: false,
        }
    }
}

impl<'s> Iterator for EmojiSeparated<'s> {
    type Item = &'s str;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(graphemes) = self.graphemes.as_mut() {
            for (index, grapheme) in graphemes.by_ref() {
                if emojis::get(grapheme).is_none() {
                    continue;
                }

                let before = (self.start < index).then(|| &self.text[self.start..index]);
                self.start = index + grapheme.len();
                if before.is_some() {
                    return before;
                }
            }
        }

        if !self.finished && self.start < self.text.len() {
            self.finished = true;
            return Some(&self.text[self.start..]);
        }
        self.finished = true;
        None
    }
}

fn tokenize_segment<'s>(text: &'s str, lang: Option<Lang>) -> WordsIter<'s> {
    let hinted_language = lang.and_then(|lang| CharabiaLanguage::from_code(lang.code()));

    let to_word = move |token: charabia::Token<'s>| {
        let range = token.byte_start..token.byte_end;
        let raw = &text[range];

        raw.chars().any(char::is_alphanumeric).then(|| WordToken {
            raw,
            language: token
                .language
                .and_then(|language| Lang::from_code(language.code()))
                .or(lang),
        })
    };

    if let Some(hinted_language) = hinted_language {
        // The allow list is local, so collect before returning the iterator.
        let allow_list = [hinted_language];
        let words = text
            .segment_with_option(None, Some(&allow_list))
            .filter_map(to_word)
            .collect::<Vec<_>>();

        Box::new(words.into_iter())
    } else {
        // Stream raw segments because Sonic applies its own normalization.
        Box::new(text.segment().filter_map(to_word))
    }
}

fn tokenize<'s>(text: &'s str, lang: Option<Lang>) -> WordsIter<'s> {
    Box::new(EmojiSeparated::new(text).flat_map(move |segment| tokenize_segment(segment, lang)))
}

impl<'s> Iterator for Tokenizer<'s> {
    type Item = Token<'s>;

    fn next(&mut self) -> Option<Self::Item> {
        // If we were walking words, continue.
        if let Some((tokens, end)) = self.tokens.as_mut() {
            match tokens.next() {
                Some(pending) => {
                    self.last_language = pending.language;
                    return Some(pending.token);
                }
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
                    let mut tokens: TokensIter<'s> =
                        Box::new(tokenize(gap, self.lang).map(|word| PendingToken {
                            token: Token::Word(word.raw),
                            language: word.language,
                        }));

                    if let Some(pending) = tokens.next() {
                        self.last_language = pending.language;
                        self.tokens = Some((tokens, end));
                        return Some(pending.token);
                    }
                }

                // Once all normal words have been visited, yield the special
                // chunk.
                let next = if self.config.compat_split_special_patterns {
                    let regex_match = captures.get_match().as_str();
                    let words = tokenize(regex_match, self.lang);

                    let mut tokens: TokensIter<'s> = Box::new(words.map(|word| PendingToken {
                        token: Token::Special {
                            raw: word.raw,
                            normalized: Cow::Borrowed(word.raw),
                        },
                        language: None,
                    }));

                    let next = tokens.next().unwrap_or(PendingToken {
                        token: Token::Special {
                            raw: regex_match,
                            normalized: Cow::Borrowed(regex_match),
                        },
                        language: None,
                    });

                    self.tokens = Some((tokens, end));

                    Some(next.token)
                } else {
                    Some(Token::special(captures))
                };

                // Advance the iterator now that we’ve visited all previous
                // tokens.
                self.regex_matches.next();
                self.regex_cursor = end;
                self.last_language = None;

                next
            }
            None => {
                // When there are no more special chunks, finish by tokenizing
                // normally.
                let gap = &self.text[self.regex_cursor..];
                let mut tokens: TokensIter<'s> =
                    Box::new(tokenize(gap, self.lang).map(|word| PendingToken {
                        token: Token::Word(word.raw),
                        language: word.language,
                    }));

                if let Some(pending) = tokens.next() {
                    self.last_language = pending.language;
                    self.tokens = Some((tokens, self.text.len()));
                    return Some(pending.token);
                }

                None
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Token<'s> {
    /// Any word, for which fuzzy matching can be applied.
    Word(&'s str),

    /// A special token, like an email address, which should not be fuzzy
    /// matched.
    Special {
        raw: &'s str,
        normalized: Cow<'s, str>,
    },
}

impl<'s> Token<'s> {
    fn special(captures: &regex::Captures<'s>) -> Self {
        let (raw, normalized) = if let Some(m) = captures.name("email") {
            (m.as_str(), Cow::Borrowed(m.as_str()))
        } else if let Some(m) = captures.name("username") {
            (m.as_str(), Cow::Borrowed(m.as_str()))
        } else if let Some(m) = captures.name("url") {
            (m.as_str(), Cow::Borrowed(m.as_str()))
        } else if let Some(m) = captures.name("ipv4") {
            (m.as_str(), Cow::Borrowed(m.as_str()))
        } else if let Some(m) = captures.name("phone") {
            let raw = m.as_str();
            let normalized: String = raw
                .chars()
                .filter(|c| c.is_ascii_digit() || *c == '+')
                .collect();
            (raw, Cow::Owned(normalized))
        } else if let Some(m) = captures.name("domain") {
            (m.as_str(), Cow::Borrowed(m.as_str()))
        } else if let Some(m) = captures.name("id") {
            (m.as_str(), Cow::Borrowed(m.as_str()))
        } else {
            unreachable!("One name always matches")
        };

        Self::Special { raw, normalized }
    }
}

#[derive(PartialEq, Eq)]
pub enum NormalizedToken {
    /// Any word, for which fuzzy matching can be applied.
    Word(String),

    /// A special token, like an email address, which should not be fuzzy
    /// matched.
    Special(String),
}

impl std::fmt::Debug for NormalizedToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Word(str) => std::fmt::Debug::fmt(str, f),
            Self::Special(str) => f.debug_tuple("Special").field(str).finish(),
        }
    }
}

impl std::ops::Deref for NormalizedToken {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Word(str) => str,
            Self::Special(str) => str,
        }
    }
}

impl NormalizedToken {
    pub fn is_special(&self) -> bool {
        matches!(self, Self::Special(_))
    }

    pub fn into_inner(self) -> String {
        match self {
            Self::Word(str) => str,
            Self::Special(str) => str,
        }
    }
}

impl From<NormalizedToken> for String {
    #[inline]
    fn from(value: NormalizedToken) -> Self {
        value.into_inner()
    }
}

pub struct TokenLexer<'a> {
    tokenizer: Tokenizer<'a>,
    yields: HashSet<String>,
    config: ConfigNormalization,
}

#[derive(PartialEq)]
pub enum TokenLexerMode {
    NormalizeAndCleanup,
    NormalizeOnly,
}

impl TokenLexerBuilder {
    pub fn from(
        _mode: TokenLexerMode,
        lang: Option<Lang>,
        text: &str,
        normalization_config: ConfigNormalization,
        tokenization_config: ConfigTokenization,
    ) -> Result<TokenLexer<'_>, ()> {
        Ok(TokenLexer::new(
            text,
            lang,
            normalization_config,
            tokenization_config,
        ))
    }
}

impl<'a> TokenLexer<'a> {
    fn new(
        text: &'a str,
        lang: Option<Lang>,
        normalization_config: ConfigNormalization,
        tokenization_config: ConfigTokenization,
    ) -> TokenLexer<'a> {
        TokenLexer {
            tokenizer: Tokenizer::new(text, lang, &tokenization_config),
            yields: HashSet::new(),
            config: normalization_config,
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

impl<'a> Iterator for TokenLexer<'a> {
    type Item = (NormalizedToken, usize);

    // Guarantees provided by the lexer on the output: \
    //   - Text is split per-word in a script-aware way \
    //   - Words are normalized (i.e. case is folded (≈ lower-cased), \
    //     diacritics are optionally folded, word is opionally stemmed) \
    //   - Gibberish words are removed (ie. words that may just be junk)
    fn next(&mut self) -> Option<Self::Item> {
        'tokenize: while let Some(token) = self.tokenizer.next() {
            let language = self.tokenizer.last_language;

            let (word, original_len) = match token {
                Token::Word(original_word) => {
                    let original_len = original_word.len();

                    #[cfg(debug_assertions)]
                    let mut current_word: String = original_word.to_owned();

                    // NOTE: We use an iterator to avoid unnecessary `String`
                    //   allocations.
                    let mut chars: Box<dyn Iterator<Item = char>> = Box::new(original_word.chars());

                    // Case folding
                    {
                        use caseless::Caseless as _;

                        chars = Box::new(chars.default_case_fold());

                        #[cfg(debug_assertions)]
                        {
                            let new_word = chars.collect();
                            tracing::trace!("Case folding: {current_word:?} -> {new_word:?}");
                            current_word = new_word;
                            chars = Box::new(current_word.chars());
                        }
                    }

                    // Diacritic folding
                    if self.config.diacritic_folding_enabled {
                        use unicode_normalization::UnicodeNormalization as _;
                        use unicode_normalization::char::is_combining_mark;

                        chars = Box::new(chars.nfd().filter(|c| !is_combining_mark(*c)));

                        #[cfg(debug_assertions)]
                        {
                            let new_word = chars.collect();
                            tracing::trace!("Diacritic folding: {current_word:?} -> {new_word:?}");
                            current_word = new_word;
                            chars = Box::new(current_word.chars());
                        }
                    }

                    // NOTE: We need to collect here as stemming algorithms need to
                    //   lookup whole words.
                    #[allow(unused_mut)]
                    let mut new_word: String = chars.collect();

                    // Stemming
                    #[cfg(feature = "stemming")]
                    if self.config.stemming_enabled {
                        if let Some(algo) = language
                            .as_ref()
                            .and_then(super::stemming::snowball_algorithm)
                        {
                            new_word = String::from(snowball::stem(algo, &new_word));

                            tracing::debug!(
                                "lexer stemmed word {original_word:?} into {new_word:?} using Snowball algorithm {algo:?}"
                            );

                            #[cfg(debug_assertions)]
                            {
                                tracing::trace!("Stemming: {current_word:?} -> {new_word:?}");
                                current_word = new_word.clone();
                            }
                        }
                    }

                    (NormalizedToken::Word(new_word), original_len)
                }
                Token::Special { normalized, .. } => {
                    let len = normalized.len();
                    (NormalizedToken::Special(normalized.into_owned()), len)
                }
            };

            if word.len() > self.tokenizer.config.max_token_length {
                tracing::debug!(
                    "lexer did not yield word {word:?}: length exceeds {} bytes",
                    self.tokenizer.config.max_token_length
                );
                continue 'tokenize;
            }

            // Check if word was not already yielded? (we return unique words)
            if !self.yields.insert(word.to_string()) {
                tracing::debug!("lexer did not yield word {word:?}: word already yielded");
                continue 'tokenize;
            }

            tracing::debug!("lexer yielded word: {word:?}");

            return Some((word, original_len));
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NORMALIZATION_CONFIG: ConfigNormalization = ConfigNormalization {
        diacritic_folding_enabled: false,
        stemming_enabled: false,
    };
    const TOKENIZATION_CONFIG: ConfigTokenization = ConfigTokenization {
        detect_special_patterns: true,
        compat_split_special_patterns: false,
        max_token_length: 128,
    };

    #[test]
    fn test_tokenizer() {
        fn test(sentence: &str, expected: Vec<Token>) {
            let tokens = Tokenizer::new(sentence, Some(Lang::Eng), &TOKENIZATION_CONFIG)
                // .inspect(|t| eprintln!("{t:?}"))
                .take(256) // Breaks potential infinite loop.
                .collect::<Vec<_>>();

            assert_eq!(tokens, expected, "{sentence:?}");
        }

        // Email address.
        test(
            "Contact jane.doe@example.org, alice@example.org or bob+foo@example.org for support.",
            vec![
                Token::Word("Contact"),
                Token::Special {
                    raw: "jane.doe@example.org",
                    normalized: Cow::Borrowed("jane.doe@example.org"),
                },
                Token::Special {
                    raw: "alice@example.org",
                    normalized: Cow::Borrowed("alice@example.org"),
                },
                Token::Word("or"),
                Token::Special {
                    raw: "bob+foo@example.org",
                    normalized: Cow::Borrowed("bob+foo@example.org"),
                },
                Token::Word("for"),
                Token::Word("support"),
            ],
        );

        // Phone number like.
        test(
            "You can also call me at 555-123-4567 or +33 6 12 34 56 78 (06.12.34.56.78 / 06 12 34 56 78).",
            vec![
                Token::Word("You"),
                Token::Word("can"),
                Token::Word("also"),
                Token::Word("call"),
                Token::Word("me"),
                Token::Word("at"),
                Token::Special {
                    raw: "555-123-4567",
                    normalized: Cow::Borrowed("5551234567"),
                },
                Token::Word("or"),
                Token::Special {
                    raw: "+33 6 12 34 56 78",
                    normalized: Cow::Borrowed("+33612345678"),
                },
                Token::Special {
                    raw: "06.12.34.56.78",
                    normalized: Cow::Borrowed("0612345678"),
                },
                Token::Special {
                    raw: "06 12 34 56 78",
                    normalized: Cow::Borrowed("0612345678"),
                },
            ],
        );

        // UUID like.
        test(
            "My account is 6db14cb4-b82e-4e49-8016-ef76c4290a2f.",
            vec![
                Token::Word("My"),
                Token::Word("account"),
                Token::Word("is"),
                Token::Special {
                    raw: "6db14cb4-b82e-4e49-8016-ef76c4290a2f",
                    normalized: Cow::Borrowed("6db14cb4-b82e-4e49-8016-ef76c4290a2f"),
                },
            ],
        );

        // Hash like.
        test(
            "Check out b244423d417369795292e9f4530d0c0e6fa07625 and 927ff7701795282232dda41e023c7c6ba29d5a15 (927ff77).",
            vec![
                Token::Word("Check"),
                Token::Word("out"),
                Token::Special {
                    raw: "b244423d417369795292e9f4530d0c0e6fa07625",
                    normalized: Cow::Borrowed("b244423d417369795292e9f4530d0c0e6fa07625"),
                },
                Token::Word("and"),
                Token::Special {
                    raw: "927ff7701795282232dda41e023c7c6ba29d5a15",
                    normalized: Cow::Borrowed("927ff7701795282232dda41e023c7c6ba29d5a15"),
                },
                Token::Special {
                    raw: "927ff77",
                    normalized: Cow::Borrowed("927ff77"),
                },
            ],
        );

        // URL.
        test(
            "Have a look at https://example.org/foo?id=123.",
            vec![
                Token::Word("Have"),
                Token::Word("a"),
                Token::Word("look"),
                Token::Word("at"),
                Token::Special {
                    raw: "https://example.org/foo?id=123",
                    normalized: Cow::Borrowed("https://example.org/foo?id=123"),
                },
            ],
        );

        // Domain name.
        test(
            "My domain name is example.org.",
            vec![
                Token::Word("My"),
                Token::Word("domain"),
                Token::Word("name"),
                Token::Word("is"),
                Token::Special {
                    raw: "example.org",
                    normalized: Cow::Borrowed("example.org"),
                },
            ],
        );
        test(
            "I don’t put punctuation correctly .See?",
            vec![
                Token::Word("I"),
                Token::Word("don"),
                Token::Word("t"),
                Token::Word("put"),
                Token::Word("punctuation"),
                Token::Word("correctly"),
                Token::Word("See"),
            ],
        );

        // IP addresses.
        test(
            "Try to ping 192.168.1.0, 0.0.0.0, 2606:4700::6812:1c68, or ::1.",
            vec![
                Token::Word("Try"),
                Token::Word("to"),
                Token::Word("ping"),
                Token::Special {
                    raw: "192.168.1.0",
                    normalized: Cow::Borrowed("192.168.1.0"),
                },
                Token::Special {
                    raw: "0.0.0.0",
                    normalized: Cow::Borrowed("0.0.0.0"),
                },
                Token::Special {
                    raw: "2606:4700::6812:1c68",
                    normalized: Cow::Borrowed("2606:4700::6812:1c68"),
                },
                Token::Word("or"),
                Token::Special {
                    raw: "::1",
                    normalized: Cow::Borrowed("::1"),
                },
            ],
        );

        // Username.
        test(
            "Contact @alice.",
            vec![
                Token::Word("Contact"),
                Token::Special {
                    raw: "@alice",
                    normalized: Cow::Borrowed("@alice"),
                },
            ],
        );

        // Code like.
        test(
            "It’s tested in test_tokenizer.",
            vec![
                Token::Word("It"),
                Token::Word("s"),
                Token::Word("tested"),
                Token::Word("in"),
                Token::Special {
                    raw: "test_tokenizer",
                    normalized: Cow::Borrowed("test_tokenizer"),
                },
            ],
        );
    }

    #[test]
    fn it_uses_emojis_as_word_boundaries() {
        let tokens = Tokenizer::new(
            "hello🚀world 🤣🤣the 👨‍👩‍👧‍👦test foo@example.org🚀",
            Some(Lang::Eng),
            &TOKENIZATION_CONFIG,
        )
        .collect::<Vec<_>>();

        assert_eq!(
            tokens,
            [
                Token::Word("hello"),
                Token::Word("world"),
                Token::Word("the"),
                Token::Word("test"),
                Token::Special {
                    raw: "foo@example.org",
                    normalized: Cow::Borrowed("foo@example.org"),
                },
            ]
        );
    }

    #[test]
    fn it_drops_oversized_tokens() {
        let mut config = TOKENIZATION_CONFIG;
        config.max_token_length = 5;
        let tokens = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            None,
            "short lengthy ééé a@b.co",
            NORMALIZATION_CONFIG,
            config,
        )
        .unwrap()
        .map(|(token, _)| token.into_inner())
        .collect::<Vec<_>>();

        assert_eq!(tokens, ["short"]);
    }

    #[test]
    fn it_tokenizes_english_without_dropping_stopwords() {
        let token_cleaner = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            None,
            "The quick brown fox jumps over the lazy dog!",
            NORMALIZATION_CONFIG,
            TOKENIZATION_CONFIG,
        )
        .unwrap();

        let tokens = token_cleaner
            .map(|(token, _)| token.into_inner())
            .collect::<Vec<_>>();

        assert_eq!(
            tokens,
            [
                "the", "quick", "brown", "fox", "jumps", "over", "lazy", "dog"
            ]
        );
    }

    #[test]
    fn it_tokenizes_french_without_dropping_stopwords() {
        let token_cleaner = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            None,
            "Le vif renard brun saute par dessus le chien paresseux.",
            NORMALIZATION_CONFIG,
            TOKENIZATION_CONFIG,
        )
        .unwrap();

        let tokens = token_cleaner
            .map(|(token, _)| token.into_inner())
            .collect::<Vec<_>>();

        assert_eq!(
            tokens,
            [
                "le",
                "vif",
                "renard",
                "brun",
                "saute",
                "par",
                "dessus",
                "chien",
                "paresseux",
            ]
        );
    }

    #[cfg(feature = "tokenizer-chinese")]
    #[test]
    fn it_tokenizes_chinese_without_dropping_stopwords() {
        let token_cleaner = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            None,
            "我们中出了一个叛徒",
            NORMALIZATION_CONFIG,
            TOKENIZATION_CONFIG,
        )
        .unwrap();

        let mut tokens = token_cleaner.map(|(token, _)| token.into_inner());

        assert_eq!(tokens.next(), Some("我们".to_owned()));
        assert_eq!(tokens.next(), Some("中".to_owned()));
        assert_eq!(tokens.next(), Some("出".to_owned()));
        assert_eq!(tokens.next(), Some("了".to_owned()));
        assert_eq!(tokens.next(), Some("一个".to_owned()));
        assert_eq!(tokens.next(), Some("叛徒".to_owned()));
        assert_eq!(tokens.next(), None);
    }

    #[cfg(not(feature = "tokenizer-chinese"))]
    #[test]
    fn it_uses_fallback_chinese_segmentation() {
        let token_cleaner = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            None,
            "快狐跨懒狗快狐跨懒狗",
            NORMALIZATION_CONFIG,
            TOKENIZATION_CONFIG,
        )
        .unwrap();

        let mut tokens = token_cleaner.map(|(token, _)| token.into_inner());

        assert_eq!(tokens.next(), Some("快狐跨懒狗快狐跨懒狗".to_owned()));
        assert_eq!(tokens.next(), None);
    }

    #[cfg(feature = "tokenizer-japanese")]
    #[test]
    fn it_tokenizes_japanese_product() {
        let token_cleaner = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            None,
            "関西国際空港限定トートバッグ",
            NORMALIZATION_CONFIG,
            TOKENIZATION_CONFIG,
        )
        .unwrap();

        let mut tokens = token_cleaner.map(|(token, _)| token.into_inner());

        assert_eq!(tokens.next(), Some("関西".to_owned()));
        assert_eq!(tokens.next(), Some("国際".to_owned()));
        assert_eq!(tokens.next(), Some("空港".to_owned()));
        assert_eq!(tokens.next(), Some("限定".to_owned()));
        assert_eq!(tokens.next(), Some("トート".to_owned()));
        assert_eq!(tokens.next(), Some("バッグ".to_owned()));
        assert_eq!(tokens.next(), None);
    }

    #[cfg(feature = "tokenizer-japanese")]
    #[test]
    fn it_tokenizes_japanese_food() {
        let mut token_cleaner = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            None,
            "𠮷野家",
            NORMALIZATION_CONFIG,
            TOKENIZATION_CONFIG,
        )
        .unwrap();

        assert!(token_cleaner.next().is_some());

        let mut token_cleaner = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            None,
            "ヱビスビール",
            NORMALIZATION_CONFIG,
            TOKENIZATION_CONFIG,
        )
        .unwrap();

        assert!(token_cleaner.next().is_some());
    }

    #[cfg(feature = "tokenizer-japanese")]
    #[test]
    fn it_tokenizes_japanese_without_dropping_stopwords() {
        let token_cleaner = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            None,
            "𠮷野家でヱビスビールを飲んだ",
            NORMALIZATION_CONFIG,
            TOKENIZATION_CONFIG,
        )
        .unwrap();

        let mut tokens = token_cleaner.map(|(token, _)| token.into_inner());

        assert_eq!(tokens.next(), Some("𠮷".to_owned()));
        assert_eq!(tokens.next(), Some("野家".to_owned()));
        assert_eq!(tokens.next(), Some("で".to_owned()));
        assert_eq!(tokens.next(), Some("ヱビス".to_owned()));
        assert_eq!(tokens.next(), Some("ビール".to_owned()));
        assert_eq!(tokens.next(), Some("を".to_owned()));
        assert_eq!(tokens.next(), Some("飲ん".to_owned()));
        assert_eq!(tokens.next(), Some("だ".to_owned()));
        assert_eq!(tokens.next(), None);
    }

    #[test]
    fn it_cleans_token_emojis() {
        let mut token_cleaner = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            None,
            "🚀 🙋‍♂️🙋‍♂️🙋‍♂️",
            NORMALIZATION_CONFIG,
            TOKENIZATION_CONFIG,
        )
        .unwrap();

        assert_eq!(token_cleaner.next(), None);
    }

    #[test]
    fn it_passes_language_hint_to_charabia() {
        let words = tokenize("A short ambiguous text", Some(Lang::Eng)).collect::<Vec<_>>();

        assert!(!words.is_empty());
        assert!(words.iter().all(|word| word.language == Some(Lang::Eng)));
    }

    #[test]
    fn segmentation_matches_previous_charabia_tokenization() {
        use charabia::{TokenKind, TokenizerBuilder};

        fn previous_words(text: &str, lang: Option<Lang>) -> Vec<(&str, Option<Lang>)> {
            let tokenizer = TokenizerBuilder::default().into_tokenizer();
            let hinted_language =
                lang.and_then(|lang| CharabiaLanguage::from_code(lang.code()));
            let allow_list = hinted_language.as_ref().map(std::slice::from_ref);

            tokenizer
                .tokenize_with_allow_list(text, allow_list)
                .filter_map(|token| {
                    let range = token.byte_start..token.byte_end;

                    matches!(token.kind, TokenKind::Word | TokenKind::StopWord)
                        .then(|| &text[range])
                        .filter(|raw| raw.chars().any(char::is_alphanumeric))
                        .map(|raw| {
                            let language = token
                                .language
                                .and_then(|language| Lang::from_code(language.code()))
                                .or(lang);

                            (raw, language)
                        })
                })
                .collect()
        }

        let cases = [
            ("The quick brown fox jumps over the lazy dog!", None),
            ("L'été à Paris coûte 25€.", Some(Lang::Fra)),
            ("我们中出了一个叛徒", None),
            ("فارسی و العربية 123", None),
            ("関西国際空港限定トートバッグ", None),
            ("Words 🚀 control\u{0}characters and@example.org", None),
        ];

        for (text, lang) in cases {
            let segmented = tokenize(text, lang)
                .map(|word| (word.raw, word.language))
                .collect::<Vec<_>>();

            assert_eq!(segmented, previous_words(text, lang), "{text:?}");
        }
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
                NORMALIZATION_CONFIG,
                TOKENIZATION_CONFIG,
            )
        });
    }

    #[bench]
    fn bench_normalize_token_french_exhaust(b: &mut Bencher) {
        b.iter(|| {
            let token_cleaner = TokenLexerBuilder::from(
                TokenLexerMode::NormalizeOnly,
                "Le vif renard brun saute par dessus le chien paresseux.",
                NORMALIZATION_CONFIG,
                TOKENIZATION_CONFIG,
            )
            .unwrap();

            token_cleaner.map(|value| value.1).collect::<Vec<usize>>()
        });
    }

    #[bench]
    fn bench_clean_token_english_regular_build(b: &mut Bencher) {
        b.iter(|| {
            TokenLexerBuilder::from(
                TokenLexerMode::NormalizeAndCleanup,
                None,
                "The quick brown fox jumps over the lazy dog!",
                NORMALIZATION_CONFIG,
                TOKENIZATION_CONFIG,
            )
        });
    }

    #[bench]
    fn bench_clean_token_english_regular_exhaust(b: &mut Bencher) {
        b.iter(|| {
            let token_cleaner = TokenLexerBuilder::from(
                TokenLexerMode::NormalizeAndCleanup,
                None,
                "The quick brown fox jumps over the lazy dog!",
                NORMALIZATION_CONFIG,
                TOKENIZATION_CONFIG,
            )
            .unwrap();

            token_cleaner.map(|value| value.1).collect::<Vec<usize>>()
        });
    }

    #[bench]
    fn bench_clean_token_english_long_exhaust(b: &mut Bencher) {
        b.iter(|| {
            let token_cleaner = TokenLexerBuilder::from(
                TokenLexerMode::NormalizeAndCleanup,
                None,
                r#"Running an electrical current through water splits it into oxygen and hydrogen,
                the latter of which can be used as a reliable, zero-emission fuel source. In the
                past, the process of purifying water beforehand was too energy intensive for this
                process to be useful — but now scientists have figured out how to skip the process
                altogether and convert seawater into usable hydrogen"#,
                NORMALIZATION_CONFIG,
                TOKENIZATION_CONFIG,
            )
            .unwrap();

            token_cleaner.map(|value| value.1).collect::<Vec<usize>>()
        });
    }

    #[bench]
    fn bench_clean_token_english_hinted_build(b: &mut Bencher) {
        b.iter(|| {
            TokenLexerBuilder::from(
                TokenLexerMode::NormalizeAndCleanup(Some(Lang::Eng)),
                "The quick brown fox jumps over the lazy dog!",
                NORMALIZATION_CONFIG,
                TOKENIZATION_CONFIG,
            )
        });
    }

    #[bench]
    fn bench_clean_token_english_hinted_exhaust(b: &mut Bencher) {
        b.iter(|| {
            let token_cleaner = TokenLexerBuilder::from(
                TokenLexerMode::NormalizeAndCleanup(Some(Lang::Eng)),
                "The quick brown fox jumps over the lazy dog!",
                NORMALIZATION_CONFIG,
                TOKENIZATION_CONFIG,
            )
            .unwrap();

            token_cleaner.map(|value| value.1).collect::<Vec<usize>>()
        });
    }

    #[bench]
    fn bench_clean_token_chinese_build(b: &mut Bencher) {
        b.iter(|| {
            TokenLexerBuilder::from(
                TokenLexerMode::NormalizeAndCleanup,
                None,
                "我们中出了一个叛徒",
                NORMALIZATION_CONFIG,
                TOKENIZATION_CONFIG,
            )
        });
    }

    #[bench]
    fn bench_clean_token_chinese_exhaust(b: &mut Bencher) {
        b.iter(|| {
            let token_cleaner = TokenLexerBuilder::from(
                TokenLexerMode::NormalizeAndCleanup,
                None,
                "我们中出了一个叛徒",
                NORMALIZATION_CONFIG,
                TOKENIZATION_CONFIG,
            )
            .unwrap();

            token_cleaner.map(|value| value.1).collect::<Vec<usize>>()
        });
    }

    #[bench]
    fn bench_clean_token_japanese_build(b: &mut Bencher) {
        b.iter(|| {
            TokenLexerBuilder::from(
                TokenLexerMode::NormalizeAndCleanup,
                None,
                "関西国際空港限定トートバッグ",
                NORMALIZATION_CONFIG,
                TOKENIZATION_CONFIG,
            )
        });
    }

    #[bench]
    fn bench_clean_token_japanese_exhaust(b: &mut Bencher) {
        b.iter(|| {
            let token_cleaner = TokenLexerBuilder::from(
                TokenLexerMode::NormalizeAndCleanup,
                None,
                "関西国際空港限定トートバッグ",
                NORMALIZATION_CONFIG,
                TOKENIZATION_CONFIG,
            )
            .unwrap();

            token_cleaner.map(|value| value.1).collect::<Vec<usize>>()
        });
    }
}
