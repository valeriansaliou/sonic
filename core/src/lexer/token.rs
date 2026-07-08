// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::borrow::Cow;
use std::iter::Peekable;
use std::sync::LazyLock;
use std::time::Instant;

use hashbrown::HashSet;
use regex::Regex;
use unicode_segmentation::UnicodeSegmentation;
use whatlang::Lang;

use crate::config::{ConfigNormalization, ConfigTokenization};
use crate::query::QueryGenericLang;
use crate::store::identifiers::{StoreTermHash, StoreTermHashed};

use super::stopwords::LexerStopWord;

pub struct TokenLexerBuilder;

type WordsIter<'s> = Box<dyn Iterator<Item = &'s str> + 's>;

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
    text: &'s str,
    lang: Option<Lang>,
    regex_matches: Peekable<regex::CaptureMatches<'static, 's>>,
    regex_cursor: usize,
    words: Option<(WordsIter<'s>, usize)>,
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
            lang,
            regex_matches,
            text,
            regex_cursor: 0,
            words: None,
        }
    }
}

fn tokenize<'s>(text: &'s str, lang: Option<Lang>) -> Box<dyn Iterator<Item = &'s str> + 's> {
    match lang {
        #[cfg(feature = "tokenizer-chinese")]
        Some(Lang::Cmn) => Box::from(TOKENIZER_JIEBA.cut(text, false).into_iter()),
        #[cfg(feature = "tokenizer-japanese")]
        Some(Lang::Jpn) => match TOKENIZER_LINDERA.tokenize(text) {
            Ok(tokens) => Box::from(tokens.into_iter()),
            Err(err) => {
                tracing::warn!("unable to tokenize japanese, falling back: {}", err);

                Box::from(text.unicode_words())
            }
        },
        _ => Box::from(text.unicode_words()),
    }
}

impl<'s> Iterator for Tokenizer<'s> {
    type Item = Token<'s>;

    fn next(&mut self) -> Option<Self::Item> {
        // If we were walking words, continue.
        if let Some((words, end)) = self.words.as_mut() {
            match words.next() {
                Some(w) => return Some(Token::Word(w)),
                None => {
                    self.regex_cursor = *end;
                    self.words = None;
                }
            }
        }

        // Check where the next special chunk is located.
        match self.regex_matches.peek() {
            Some(captures) => {
                let m = captures.get_match();
                let start = m.start();
                let end = m.end();

                // Up until that special chunk, tokenize normally.
                if start > self.regex_cursor {
                    let gap = &self.text[self.regex_cursor..start];
                    let mut words = tokenize(gap, self.lang);

                    if let Some(w) = words.next() {
                        self.words = Some((words, end));
                        return Some(Token::Word(w));
                    }
                }

                // Once all normal words have been visited, yield the special
                // chunk.
                self.regex_cursor = end;
                let next = Some(Token::special(captures));

                // Advance the iterator now that we’ve visited all previous
                // tokens.
                self.regex_matches.next();

                next
            }
            None => {
                // When there are no more special chunks, finish by tokenizing
                // normally.
                let gap = &self.text[self.regex_cursor..];
                let mut words = tokenize(gap, self.lang);

                if let Some(w) = words.next() {
                    self.words = Some((words, self.text.len()));
                    return Some(Token::Word(w));
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
    mode: TokenLexerMode,
    locale: Option<Lang>,
    #[cfg(feature = "stemming")]
    snowball_algorithm: Option<snowball::Algorithm>,
    tokenizer: Tokenizer<'a>,
    yields: HashSet<StoreTermHashed>,
    config: ConfigNormalization,
}

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

const TEXT_LANG_TRUNCATE_OVER_CHARS: usize = 200;
const TEXT_LANG_DETECT_PROCEED_OVER_CHARS: usize = 20;
const TEXT_LANG_DETECT_NGRAM_UNDER_CHARS: usize = 60;

#[cfg(feature = "tokenizer-chinese")]
static TOKENIZER_JIEBA: LazyLock<jieba_rs::Jieba> = LazyLock::new(jieba_rs::Jieba::new);

#[cfg(feature = "tokenizer-japanese")]
static TOKENIZER_LINDERA: LazyLock<lindera_tokenizer::tokenizer::Tokenizer> = LazyLock::new(|| {
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

impl TokenLexerBuilder {
    pub fn from(
        mode: TokenLexerMode,
        lang: Option<Lang>,
        text: &str,
        normalization_config: ConfigNormalization,
        tokenization_config: ConfigTokenization,
    ) -> Result<TokenLexer<'_>, ()> {
        let locale = match lang {
            // If user provided a language, use it.
            Some(hinted_lang) => {
                // Use hinted language (current lexer mode asks for a cleanup)
                tracing::debug!(
                    "using hinted locale: {} from lexer text: {}",
                    hinted_lang,
                    text
                );

                lang
            }

            None => match mode {
                // If user asked to cleanup, detect the language.
                TokenLexerMode::NormalizeAndCleanup => {
                    let locale = Self::detect_lang(text);
                    tracing::debug!("detected locale: {:?} from lexer text: {}", locale, text);
                    locale
                }

                // If user asked not to cleanup but stemming is enabled, detect the language.
                #[cfg(feature = "stemming")]
                TokenLexerMode::NormalizeOnly if normalization_config.stemming_enabled => {
                    let locale = Self::detect_lang(text);
                    tracing::debug!("detected locale: {:?} from lexer text: {}", locale, text);
                    locale
                }

                // Otherwise, don’t detect the language.
                TokenLexerMode::NormalizeOnly => {
                    tracing::debug!("not detecting locale from lexer text: {}", text);

                    None
                }
            },
        };

        // Build final token builder iterator
        Ok(TokenLexer::new(
            mode,
            text,
            locale,
            normalization_config,
            tokenization_config,
        ))
    }

    fn detect_lang(text: &str) -> Option<Lang> {
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

            Self::detect_lang_slow(safe_text)
        } else {
            tracing::debug!(
                "lexer text is equal or longer than {} characters, using the fast method",
                TEXT_LANG_DETECT_NGRAM_UNDER_CHARS
            );

            Self::detect_lang_fast(safe_text)
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

impl<'a> TokenLexer<'a> {
    fn new(
        mode: TokenLexerMode,
        text: &'a str,
        locale: Option<Lang>,
        normalization_config: ConfigNormalization,
        tokenization_config: ConfigTokenization,
    ) -> TokenLexer<'a> {
        // Tokenize words (depending on the locale)
        let tokenizer = Tokenizer::new(text, locale, &tokenization_config);

        // Identify Snowball algorithm now to avoid doing it for every token.
        #[cfg(feature = "stemming")]
        let snowball_algorithm = match &locale {
            Some(locale) => super::stemming::snowball_algorithm(locale),
            None => None,
        };

        TokenLexer {
            mode,
            locale,
            #[cfg(feature = "stemming")]
            snowball_algorithm,
            tokenizer,
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
    type Item = (NormalizedToken, StoreTermHashed, usize);

    // Guarantees provided by the lexer on the output: \
    //   - Text is split per-word in a script-aware way \
    //   - Words are normalized (i.e. case is folded (≈ lower-cased), \
    //     diacritics are optionally folded, word is opionally stemmed) \
    //   - Gibberish words are removed (ie. words that may just be junk) \
    //   - Stop-words are removed
    fn next(&mut self) -> Option<Self::Item> {
        'tokenize: for token in self.tokenizer.by_ref() {
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
                        if let Some(algo) = self.snowball_algorithm {
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

            // Check if normalized word is a stop-word? (if should normalize and cleanup)
            if self.mode.should_cleanup() && LexerStopWord::is(&word, self.locale) {
                tracing::debug!("lexer did not yield word {word:?}: word is a stop-word");
                continue 'tokenize;
            }

            // Hash the term (this is used by all iterator consumers, as well as internally \
            //   in the iterator to keep track of already-yielded words in a space-optimized \
            //   manner, ie. by using 32-bit unsigned integer hashes)
            let term_hash = StoreTermHash::from(&word);

            // Check if word was not already yielded? (we return unique words)
            if self.yields.contains(&term_hash) {
                tracing::debug!("lexer did not yield word {word:?}: word already yielded");
                continue 'tokenize;
            }

            tracing::debug!("lexer yielded word: {word:?}");

            self.yields.insert(term_hash);

            return Some((word, term_hash, original_len));
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
                Token::Word("don’t"),
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
                Token::Word("It’s"),
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
    fn it_cleans_token_english() {
        let token_cleaner = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            None,
            "The quick brown fox jumps over the lazy dog!",
            NORMALIZATION_CONFIG,
            TOKENIZATION_CONFIG,
        )
        .unwrap();

        assert_eq!(token_cleaner.locale, Some(Lang::Eng));

        let mut tokens = token_cleaner.map(|(token, _, _)| token.into_inner());

        assert_eq!(tokens.next(), Some("quick".to_owned()));
        assert_eq!(tokens.next(), Some("brown".to_owned()));
        assert_eq!(tokens.next(), Some("fox".to_owned()));
        assert_eq!(tokens.next(), Some("jumps".to_owned()));
        assert_eq!(tokens.next(), Some("lazy".to_owned()));
        assert_eq!(tokens.next(), Some("dog".to_owned()));
        assert_eq!(tokens.next(), None);
    }

    #[test]
    fn it_cleans_token_french() {
        let token_cleaner = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            None,
            "Le vif renard brun saute par dessus le chien paresseux.",
            NORMALIZATION_CONFIG,
            TOKENIZATION_CONFIG,
        )
        .unwrap();

        assert_eq!(token_cleaner.locale, Some(Lang::Fra));

        let mut tokens = token_cleaner.map(|(token, _, _)| token.into_inner());

        assert_eq!(tokens.next(), Some("renard".to_owned()));
        assert_eq!(tokens.next(), Some("brun".to_owned()));
        assert_eq!(tokens.next(), Some("saute".to_owned()));
        assert_eq!(tokens.next(), Some("chien".to_owned()));
        assert_eq!(tokens.next(), Some("paresseux".to_owned()));
        assert_eq!(tokens.next(), None);
    }

    #[cfg(feature = "tokenizer-chinese")]
    #[test]
    fn it_cleans_token_chinese_jieba() {
        let token_cleaner = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            None,
            "我们中出了一个叛徒",
            NORMALIZATION_CONFIG,
            TOKENIZATION_CONFIG,
        )
        .unwrap();

        assert_eq!(token_cleaner.locale, Some(Lang::Cmn));

        let mut tokens = token_cleaner.map(|(token, _, _)| token.into_inner());

        assert_eq!(tokens.next(), Some("出".to_owned()));
        assert_eq!(tokens.next(), Some("一个".to_owned()));
        assert_eq!(tokens.next(), Some("叛徒".to_owned()));
        assert_eq!(tokens.next(), None);
    }

    #[cfg(not(feature = "tokenizer-chinese"))]
    #[test]
    fn it_cleans_token_chinese_naive() {
        let token_cleaner = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            None,
            "快狐跨懒狗快狐跨懒狗",
            NORMALIZATION_CONFIG,
            TOKENIZATION_CONFIG,
        )
        .unwrap();

        assert_eq!(token_cleaner.locale, Some(Lang::Cmn));

        let mut tokens = token_cleaner.map(|(token, _, _)| token.into_inner());

        assert_eq!(tokens.next(), Some("快".to_owned()));
        assert_eq!(tokens.next(), Some("狐".to_owned()));
        assert_eq!(tokens.next(), Some("跨".to_owned()));
        assert_eq!(tokens.next(), Some("懒".to_owned()));
        assert_eq!(tokens.next(), Some("狗".to_owned()));
        assert_eq!(tokens.next(), None);
    }

    #[cfg(feature = "tokenizer-japanese")]
    #[test]
    fn it_cleans_token_japanese_lindera_product() {
        let mut token_cleaner = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            None,
            "関西国際空港限定トートバッグ",
            NORMALIZATION_CONFIG,
            TOKENIZATION_CONFIG,
        )
        .unwrap();

        assert_eq!(token_cleaner.locale, Some(Lang::Jpn));

        let mut tokens = token_cleaner.map(|(token, _, _)| token.into_inner());

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
    fn it_cleans_token_japanese_lindera_food() {
        let token_cleaner = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            None,
            "𠮷野家",
            NORMALIZATION_CONFIG,
            TOKENIZATION_CONFIG,
        )
        .unwrap();

        assert_eq!(token_cleaner.locale, None);

        let token_cleaner = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            None,
            "ヱビスビール",
            NORMALIZATION_CONFIG,
            TOKENIZATION_CONFIG,
        )
        .unwrap();

        assert_eq!(token_cleaner.locale, None);
    }

    #[cfg(feature = "tokenizer-japanese")]
    #[test]
    fn it_cleans_token_japanese_lindera_sentence() {
        let mut token_cleaner = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            None,
            "𠮷野家でヱビスビールを飲んだ",
            NORMALIZATION_CONFIG,
            TOKENIZATION_CONFIG,
        )
        .unwrap();

        assert_eq!(token_cleaner.locale, Some(Lang::Jpn));

        let mut tokens = token_cleaner.map(|(token, _, _)| token.into_inner());

        assert_eq!(tokens.next(), Some("𠮷".to_owned()));
        assert_eq!(tokens.next(), Some("野家".to_owned()));
        assert_eq!(tokens.next(), Some("ヱビス".to_owned()));
        assert_eq!(tokens.next(), Some("ビール".to_owned()));
        assert_eq!(tokens.next(), Some("飲ん".to_owned()));
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

        assert_eq!(token_cleaner.locale, None);

        assert_eq!(token_cleaner.next(), None);
    }

    #[test]
    fn it_cleans_token_lang_hinted() {
        let token_cleaner_right = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            Some(Lang::Eng),
            "This will be cleaned properly, as English was hinted rightfully so.",
            NORMALIZATION_CONFIG,
            TOKENIZATION_CONFIG,
        )
        .unwrap();
        let token_cleaner_wrong = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            Some(Lang::Fra),
            "This will not be cleaned properly, as French was hinted but this is English.",
            NORMALIZATION_CONFIG,
            TOKENIZATION_CONFIG,
        )
        .unwrap();

        assert_eq!(token_cleaner_right.locale, Some(Lang::Eng));
        assert_eq!(token_cleaner_wrong.locale, Some(Lang::Fra));

        let mut tokens_right = token_cleaner_right.map(|(token, _, _)| token.into_inner());
        let mut tokens_wrong = token_cleaner_wrong.map(|(token, _, _)| token.into_inner());

        assert_eq!(tokens_right.next(), Some("cleaned".to_owned()));
        assert_eq!(tokens_wrong.next(), Some("this".to_owned()));
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

            token_cleaner.map(|value| value.1).collect::<Vec<u32>>()
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

            token_cleaner.map(|value| value.1).collect::<Vec<u32>>()
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

            token_cleaner.map(|value| value.1).collect::<Vec<u32>>()
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

            token_cleaner.map(|value| value.1).collect::<Vec<u32>>()
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

            token_cleaner.map(|value| value.1).collect::<Vec<u32>>()
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
