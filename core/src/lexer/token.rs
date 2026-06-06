// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use hashbrown::HashSet;
use std::time::Instant;
use unicode_segmentation::{UnicodeSegmentation, UnicodeWords};
use whatlang::{
    Lang, detect as lang_detect_all, detect_lang as lang_detect, detect_script as script_detect,
};

#[cfg(feature = "tokenizer-chinese")]
use std::vec::IntoIter;

use super::stopwords::LexerStopWord;
use crate::config::ConfigNormalization;
use crate::query::QueryGenericLang;
use crate::store::identifiers::{StoreTermHash, StoreTermHashed};

pub struct TokenLexerBuilder;

pub struct TokenLexer<'a> {
    mode: TokenLexerMode,
    locale: Option<Lang>,
    #[cfg(feature = "stemming")]
    snowball_algorithm: Option<snowball::Algorithm>,
    words: TokenLexerWords<'a>,
    yields: HashSet<StoreTermHashed>,
    config: ConfigNormalization,
}

#[derive(PartialEq)]
pub enum TokenLexerMode {
    NormalizeAndCleanup,
    NormalizeOnly,
}

enum TokenLexerWords<'a> {
    UAX29(UnicodeWords<'a>),

    #[cfg(feature = "tokenizer-chinese")]
    JieBa(IntoIter<&'a str>),

    #[cfg(feature = "tokenizer-japanese")]
    Lindera(IntoIter<lindera_tokenizer::token::Token<'a>>),
}

const TEXT_LANG_TRUNCATE_OVER_CHARS: usize = 200;
const TEXT_LANG_DETECT_PROCEED_OVER_CHARS: usize = 20;
const TEXT_LANG_DETECT_NGRAM_UNDER_CHARS: usize = 60;

#[cfg(feature = "tokenizer-chinese")]
lazy_static! {
    static ref TOKENIZER_JIEBA: jieba_rs::Jieba = jieba_rs::Jieba::new();
}

#[cfg(feature = "tokenizer-japanese")]
lazy_static! {
    static ref TOKENIZER_LINDERA: lindera_tokenizer::tokenizer::Tokenizer =
        lindera_tokenizer::tokenizer::Tokenizer::from_config(
            lindera_tokenizer::tokenizer::TokenizerConfig {
                dictionary: lindera_dictionary::DictionaryConfig {
                    kind: Some(lindera_dictionary::DictionaryKind::UniDic),
                    path: None
                },
                user_dictionary: None,
                mode: lindera_core::mode::Mode::Normal,
            }
        )
        .expect("unable to initialize japanese tokenizer");
}

impl TokenLexerBuilder {
    pub fn from(
        mode: TokenLexerMode,
        lang: Option<Lang>,
        text: &str,
        config: ConfigNormalization,
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
                TokenLexerMode::NormalizeOnly if config.stemming_enabled => {
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
        Ok(TokenLexer::new(mode, text, locale, config))
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
            // Notice: then 'len()' check above was not UTF-8 aware, but is better than \
            //   nothing as it avoids entering the below iterator for small strings.
            // Notice: we fallback on text if the result is 'None'; as if it is 'None' there \
            //   was less characters than the truncate limit in the UTF-8 parsed text. With \
            //   this unwrap-way, we avoid doing a 'text.chars().count()' every time, which is \
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

        match lang_detect_all(safe_text) {
            Some(detector) => {
                let ngram_took = ngram_start.elapsed();

                let mut locale = detector.lang();

                tracing::info!(
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
                if !detector.is_reliable() {
                    tracing::debug!("[slow lexer] trying to detect locale from stopwords instead");

                    // Better alternate locale found?
                    if let Some(alternate_locale) =
                        LexerStopWord::guess_lang(safe_text, detector.script())
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

        match script_detect(safe_text) {
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
                    lang_detect(safe_text)
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
        config: ConfigNormalization,
    ) -> TokenLexer<'a> {
        // Tokenize words (depending on the locale)
        let words = match locale {
            #[cfg(feature = "tokenizer-chinese")]
            Some(Lang::Cmn) => TokenLexerWords::JieBa(TOKENIZER_JIEBA.cut(text, false).into_iter()),
            #[cfg(feature = "tokenizer-japanese")]
            Some(Lang::Jpn) => match TOKENIZER_LINDERA.tokenize(text) {
                Ok(tokens) => TokenLexerWords::Lindera(tokens.into_iter()),
                Err(err) => {
                    tracing::warn!("unable to tokenize japanese, falling back: {}", err);

                    TokenLexerWords::UAX29(text.unicode_words())
                }
            },
            _ => TokenLexerWords::UAX29(text.unicode_words()),
        };

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
            words,
            yields: HashSet::new(),
            config,
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
    type Item = (String, StoreTermHashed, usize);

    // Guarantees provided by the lexer on the output: \
    //   - Text is split per-word in a script-aware way \
    //   - Words are normalized (i.e. case is folded (≈ lower-cased), \
    //     diacritics are optionally folded, word is opionally stemmed) \
    //   - Gibberish words are removed (ie. words that may just be junk) \
    //   - Stop-words are removed
    fn next(&mut self) -> Option<Self::Item> {
        for original_word in &mut self.words {
            let original_len = original_word.len();

            let word = {
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
                            "lexer stemmed word {original_word:?} into {new_word:?} \
                            using Snowball algorithm {algo:?}"
                        );

                        #[cfg(debug_assertions)]
                        {
                            tracing::trace!("Stemming: {current_word:?} -> {new_word:?}");
                            current_word = new_word.clone();
                        }
                    }
                }

                new_word
            };

            // Check if normalized word is a stop-word? (if should normalize and cleanup)
            if self.mode == TokenLexerMode::NormalizeOnly || !LexerStopWord::is(&word, self.locale)
            {
                // Hash the term (this is used by all iterator consumers, as well as internally \
                //   in the iterator to keep track of already-yielded words in a space-optimized \
                //   manner, ie. by using 32-bit unsigned integer hashes)
                let term_hash = StoreTermHash::from(&word);

                // Check if word was not already yielded? (we return unique words)
                if !self.yields.contains(&term_hash) {
                    tracing::debug!("lexer yielded word: {}", word);

                    self.yields.insert(term_hash);

                    return Some((word, term_hash, original_len));
                } else {
                    tracing::debug!(
                        "lexer did not yield word: {} because: word already yielded",
                        word
                    );
                }
            } else {
                tracing::debug!(
                    "lexer did not yield word: {} because: word is a stop-word",
                    word
                );
            }
        }

        None
    }
}

impl<'a> Iterator for TokenLexerWords<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            TokenLexerWords::UAX29(token) => token.next(),

            #[cfg(feature = "tokenizer-chinese")]
            TokenLexerWords::JieBa(token) => token.next(),

            #[cfg(feature = "tokenizer-japanese")]
            TokenLexerWords::Lindera(token) => match token.next() {
                Some(inner) => Some(inner.text),
                None => None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NORMALIZATION_CONFIG: ConfigNormalization = ConfigNormalization {
        diacritic_folding_enabled: false,
        stemming_enabled: false,
    };

    #[test]
    fn it_cleans_token_english() {
        let mut token_cleaner = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            None,
            "The quick brown fox jumps over the lazy dog!",
            NORMALIZATION_CONFIG,
        )
        .unwrap();

        assert_eq!(token_cleaner.locale, Some(Lang::Eng));
        assert_eq!(
            token_cleaner.next(),
            Some(("quick".to_string(), 4179131656, 5))
        );
        assert_eq!(
            token_cleaner.next(),
            Some(("brown".to_string(), 1268820067, 5))
        );
        assert_eq!(
            token_cleaner.next(),
            Some(("fox".to_string(), 667256324, 3))
        );
        assert_eq!(
            token_cleaner.next(),
            Some(("jumps".to_string(), 633865164, 5))
        );
        assert_eq!(
            token_cleaner.next(),
            Some(("lazy".to_string(), 4130433347, 4))
        );
        assert_eq!(
            token_cleaner.next(),
            Some(("dog".to_string(), 2044924251, 3))
        );
        assert_eq!(token_cleaner.next(), None);
    }

    #[test]
    fn it_cleans_token_french() {
        let mut token_cleaner = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            None,
            "Le vif renard brun saute par dessus le chien paresseux.",
            NORMALIZATION_CONFIG,
        )
        .unwrap();

        assert_eq!(token_cleaner.locale, Some(Lang::Fra));
        assert_eq!(
            token_cleaner.next(),
            Some(("renard".to_string(), 1635186311, 6))
        );
        assert_eq!(
            token_cleaner.next(),
            Some(("brun".to_string(), 2763604928, 4))
        );
        assert_eq!(
            token_cleaner.next(),
            Some(("saute".to_string(), 1918158211, 5))
        );
        assert_eq!(
            token_cleaner.next(),
            Some(("chien".to_string(), 2177818351, 5))
        );
        assert_eq!(
            token_cleaner.next(),
            Some(("paresseux".to_string(), 1678693110, 9))
        );
        assert_eq!(token_cleaner.next(), None);
    }

    #[cfg(feature = "tokenizer-chinese")]
    #[test]
    fn it_cleans_token_chinese_jieba() {
        let mut token_cleaner = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            None,
            "我们中出了一个叛徒",
            NORMALIZATION_CONFIG,
        )
        .unwrap();

        assert_eq!(token_cleaner.locale, Some(Lang::Cmn));
        assert_eq!(token_cleaner.next(), Some(("出".into(), 241978070, 3)));
        assert_eq!(token_cleaner.next(), Some(("一个".into(), 2596274530, 6)));
        assert_eq!(token_cleaner.next(), Some(("叛徒".into(), 3244183759, 6)));
        assert_eq!(token_cleaner.next(), None);
    }

    #[cfg(not(feature = "tokenizer-chinese"))]
    #[test]
    fn it_cleans_token_chinese_naive() {
        let mut token_cleaner = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            None,
            "快狐跨懒狗快狐跨懒狗",
            NORMALIZATION_CONFIG,
        )
        .unwrap();

        assert_eq!(token_cleaner.locale, Some(Lang::Cmn));
        assert_eq!(token_cleaner.next(), Some(("快".to_string(), 126546256, 3)));
        assert_eq!(
            token_cleaner.next(),
            Some(("狐".to_string(), 2879689662, 3))
        );
        assert_eq!(
            token_cleaner.next(),
            Some(("跨".to_string(), 2913342670, 3))
        );
        assert_eq!(
            token_cleaner.next(),
            Some(("懒".to_string(), 3199935961, 3))
        );
        assert_eq!(
            token_cleaner.next(),
            Some(("狗".to_string(), 3360772096, 3))
        );
        assert_eq!(token_cleaner.next(), None);
    }

    #[cfg(feature = "tokenizer-japanese")]
    #[test]
    fn it_cleans_token_japanese_lindera_product() {
        let mut token_cleaner = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            None,
            "関西国際空港限定トートバッグ",
            NORMALIZATION_CONFIG,
        )
        .unwrap();

        assert_eq!(token_cleaner.locale, Some(Lang::Jpn));
        assert_eq!(
            token_cleaner.next(),
            Some(("関西".to_string(), 1283572620, 6))
        );
        assert_eq!(
            token_cleaner.next(),
            Some(("国際".to_string(), 2132457693, 6))
        );
        assert_eq!(
            token_cleaner.next(),
            Some(("空港".to_string(), 865668138, 6))
        );
        assert_eq!(
            token_cleaner.next(),
            Some(("限定".to_string(), 3708465176, 6))
        );
        assert_eq!(
            token_cleaner.next(),
            Some(("トート".to_string(), 881444746, 9))
        );
        assert_eq!(
            token_cleaner.next(),
            Some(("バッグ".to_string(), 3515727814, 9))
        );
        assert_eq!(token_cleaner.next(), None);
    }

    #[cfg(feature = "tokenizer-japanese")]
    #[test]
    fn it_cleans_token_japanese_lindera_food() {
        let token_cleaner = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            None,
            "𠮷野家",
            NORMALIZATION_CONFIG,
        )
        .unwrap();

        assert_eq!(token_cleaner.locale, None);

        let token_cleaner = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            None,
            "ヱビスビール",
            NORMALIZATION_CONFIG,
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
        )
        .unwrap();

        assert_eq!(token_cleaner.locale, Some(Lang::Jpn));
        assert_eq!(
            token_cleaner.next(),
            Some(("𠮷".to_string(), 2866455824, 4))
        );
        assert_eq!(
            token_cleaner.next(),
            Some(("野家".to_string(), 1324395598, 6))
        );
        assert_eq!(
            token_cleaner.next(),
            Some(("ヱビス".to_string(), 1696836208, 9))
        );
        assert_eq!(
            token_cleaner.next(),
            Some(("ビール".to_string(), 3421909800, 9))
        );
        assert_eq!(
            token_cleaner.next(),
            Some(("飲ん".to_string(), 3196735184, 6))
        );
        assert_eq!(token_cleaner.next(), None);
    }

    #[test]
    fn it_cleans_token_emojis() {
        let mut token_cleaner = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            None,
            "🚀 🙋‍♂️🙋‍♂️🙋‍♂️",
            NORMALIZATION_CONFIG,
        )
        .unwrap();

        assert_eq!(token_cleaner.locale, None);
        assert_eq!(token_cleaner.next(), None);
    }

    #[test]
    fn it_cleans_token_lang_hinted() {
        let mut token_cleaner_right = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            Some(Lang::Eng),
            "This will be cleaned properly, as English was hinted rightfully so.",
            NORMALIZATION_CONFIG,
        )
        .unwrap();
        let mut token_cleaner_wrong = TokenLexerBuilder::from(
            TokenLexerMode::NormalizeAndCleanup,
            Some(Lang::Fra),
            "This will not be cleaned properly, as French was hinted but this is English.",
            NORMALIZATION_CONFIG,
        )
        .unwrap();

        assert_eq!(token_cleaner_right.locale, Some(Lang::Eng));
        assert_eq!(token_cleaner_wrong.locale, Some(Lang::Fra));

        assert_eq!(
            token_cleaner_right.next(),
            Some(("cleaned".to_string(), 3550382624, 7))
        );
        assert_eq!(
            token_cleaner_wrong.next(),
            Some(("this".to_string(), 493303710, 4))
        );
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
