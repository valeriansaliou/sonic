// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::sync::LazyLock;

use hashbrown::HashSet;
use whatlang::{Lang, Script};

use crate::stopwords::*;

pub struct LexerStopWord;

static STOPWORDS_EPO: LazyLock<HashSet<&str>> = LazyLock::new(|| make(epo::STOPWORDS_EPO));
static STOPWORDS_ENG: LazyLock<HashSet<&str>> = LazyLock::new(|| make(eng::STOPWORDS_ENG));
static STOPWORDS_RUS: LazyLock<HashSet<&str>> = LazyLock::new(|| make(rus::STOPWORDS_RUS));
static STOPWORDS_CMN: LazyLock<HashSet<&str>> = LazyLock::new(|| make(cmn::STOPWORDS_CMN));
static STOPWORDS_SPA: LazyLock<HashSet<&str>> = LazyLock::new(|| make(spa::STOPWORDS_SPA));
static STOPWORDS_POR: LazyLock<HashSet<&str>> = LazyLock::new(|| make(por::STOPWORDS_POR));
static STOPWORDS_ITA: LazyLock<HashSet<&str>> = LazyLock::new(|| make(ita::STOPWORDS_ITA));
static STOPWORDS_BEN: LazyLock<HashSet<&str>> = LazyLock::new(|| make(ben::STOPWORDS_BEN));
static STOPWORDS_FRA: LazyLock<HashSet<&str>> = LazyLock::new(|| make(fra::STOPWORDS_FRA));
static STOPWORDS_DEU: LazyLock<HashSet<&str>> = LazyLock::new(|| make(deu::STOPWORDS_DEU));

static STOPWORDS_UKR: LazyLock<HashSet<&str>> = LazyLock::new(|| make(ukr::STOPWORDS_UKR));
static STOPWORDS_KAT: LazyLock<HashSet<&str>> = LazyLock::new(|| make(kat::STOPWORDS_KAT));
static STOPWORDS_ARA: LazyLock<HashSet<&str>> = LazyLock::new(|| make(ara::STOPWORDS_ARA));
static STOPWORDS_HIN: LazyLock<HashSet<&str>> = LazyLock::new(|| make(hin::STOPWORDS_HIN));
static STOPWORDS_JPN: LazyLock<HashSet<&str>> = LazyLock::new(|| make(jpn::STOPWORDS_JPN));
static STOPWORDS_HEB: LazyLock<HashSet<&str>> = LazyLock::new(|| make(heb::STOPWORDS_HEB));
static STOPWORDS_YID: LazyLock<HashSet<&str>> = LazyLock::new(|| make(yid::STOPWORDS_YID));
static STOPWORDS_POL: LazyLock<HashSet<&str>> = LazyLock::new(|| make(pol::STOPWORDS_POL));
static STOPWORDS_AMH: LazyLock<HashSet<&str>> = LazyLock::new(|| make(amh::STOPWORDS_AMH));
static STOPWORDS_JAV: LazyLock<HashSet<&str>> = LazyLock::new(|| make(jav::STOPWORDS_JAV));

static STOPWORDS_KOR: LazyLock<HashSet<&str>> = LazyLock::new(|| make(kor::STOPWORDS_KOR));
static STOPWORDS_NOB: LazyLock<HashSet<&str>> = LazyLock::new(|| make(nob::STOPWORDS_NOB));
static STOPWORDS_DAN: LazyLock<HashSet<&str>> = LazyLock::new(|| make(dan::STOPWORDS_DAN));
static STOPWORDS_SWE: LazyLock<HashSet<&str>> = LazyLock::new(|| make(swe::STOPWORDS_SWE));
static STOPWORDS_FIN: LazyLock<HashSet<&str>> = LazyLock::new(|| make(fin::STOPWORDS_FIN));
static STOPWORDS_TUR: LazyLock<HashSet<&str>> = LazyLock::new(|| make(tur::STOPWORDS_TUR));
static STOPWORDS_NLD: LazyLock<HashSet<&str>> = LazyLock::new(|| make(nld::STOPWORDS_NLD));
static STOPWORDS_HUN: LazyLock<HashSet<&str>> = LazyLock::new(|| make(hun::STOPWORDS_HUN));
static STOPWORDS_CES: LazyLock<HashSet<&str>> = LazyLock::new(|| make(ces::STOPWORDS_CES));
static STOPWORDS_ELL: LazyLock<HashSet<&str>> = LazyLock::new(|| make(ell::STOPWORDS_ELL));

static STOPWORDS_BUL: LazyLock<HashSet<&str>> = LazyLock::new(|| make(bul::STOPWORDS_BUL));
static STOPWORDS_BEL: LazyLock<HashSet<&str>> = LazyLock::new(|| make(bel::STOPWORDS_BEL));
static STOPWORDS_MAR: LazyLock<HashSet<&str>> = LazyLock::new(|| make(mar::STOPWORDS_MAR));
static STOPWORDS_KAN: LazyLock<HashSet<&str>> = LazyLock::new(|| make(kan::STOPWORDS_KAN));
static STOPWORDS_RON: LazyLock<HashSet<&str>> = LazyLock::new(|| make(ron::STOPWORDS_RON));
static STOPWORDS_SLV: LazyLock<HashSet<&str>> = LazyLock::new(|| make(slv::STOPWORDS_SLV));
static STOPWORDS_HRV: LazyLock<HashSet<&str>> = LazyLock::new(|| make(hrv::STOPWORDS_HRV));
static STOPWORDS_SRP: LazyLock<HashSet<&str>> = LazyLock::new(|| make(srp::STOPWORDS_SRP));
static STOPWORDS_MKD: LazyLock<HashSet<&str>> = LazyLock::new(|| make(mkd::STOPWORDS_MKD));
static STOPWORDS_LIT: LazyLock<HashSet<&str>> = LazyLock::new(|| make(lit::STOPWORDS_LIT));

static STOPWORDS_LAV: LazyLock<HashSet<&str>> = LazyLock::new(|| make(lav::STOPWORDS_LAV));
static STOPWORDS_EST: LazyLock<HashSet<&str>> = LazyLock::new(|| make(est::STOPWORDS_EST));
static STOPWORDS_TAM: LazyLock<HashSet<&str>> = LazyLock::new(|| make(tam::STOPWORDS_TAM));
static STOPWORDS_VIE: LazyLock<HashSet<&str>> = LazyLock::new(|| make(vie::STOPWORDS_VIE));
static STOPWORDS_URD: LazyLock<HashSet<&str>> = LazyLock::new(|| make(urd::STOPWORDS_URD));
static STOPWORDS_THA: LazyLock<HashSet<&str>> = LazyLock::new(|| make(tha::STOPWORDS_THA));
static STOPWORDS_GUJ: LazyLock<HashSet<&str>> = LazyLock::new(|| make(guj::STOPWORDS_GUJ));
static STOPWORDS_UZB: LazyLock<HashSet<&str>> = LazyLock::new(|| make(uzb::STOPWORDS_UZB));
static STOPWORDS_PAN: LazyLock<HashSet<&str>> = LazyLock::new(|| make(pan::STOPWORDS_PAN));
static STOPWORDS_AZE: LazyLock<HashSet<&str>> = LazyLock::new(|| make(aze::STOPWORDS_AZE));

static STOPWORDS_IND: LazyLock<HashSet<&str>> = LazyLock::new(|| make(ind::STOPWORDS_IND));
static STOPWORDS_TEL: LazyLock<HashSet<&str>> = LazyLock::new(|| make(tel::STOPWORDS_TEL));
static STOPWORDS_PES: LazyLock<HashSet<&str>> = LazyLock::new(|| make(pes::STOPWORDS_PES));
static STOPWORDS_MAL: LazyLock<HashSet<&str>> = LazyLock::new(|| make(mal::STOPWORDS_MAL));
static STOPWORDS_ORI: LazyLock<HashSet<&str>> = LazyLock::new(|| make(ori::STOPWORDS_ORI));
static STOPWORDS_MYA: LazyLock<HashSet<&str>> = LazyLock::new(|| make(mya::STOPWORDS_MYA));
static STOPWORDS_NEP: LazyLock<HashSet<&str>> = LazyLock::new(|| make(nep::STOPWORDS_NEP));
static STOPWORDS_SIN: LazyLock<HashSet<&str>> = LazyLock::new(|| make(sin::STOPWORDS_SIN));
static STOPWORDS_KHM: LazyLock<HashSet<&str>> = LazyLock::new(|| make(khm::STOPWORDS_KHM));
static STOPWORDS_TUK: LazyLock<HashSet<&str>> = LazyLock::new(|| make(tuk::STOPWORDS_TUK));

static STOPWORDS_AKA: LazyLock<HashSet<&str>> = LazyLock::new(|| make(aka::STOPWORDS_AKA));
static STOPWORDS_ZUL: LazyLock<HashSet<&str>> = LazyLock::new(|| make(zul::STOPWORDS_ZUL));
static STOPWORDS_SNA: LazyLock<HashSet<&str>> = LazyLock::new(|| make(sna::STOPWORDS_SNA));
static STOPWORDS_AFR: LazyLock<HashSet<&str>> = LazyLock::new(|| make(afr::STOPWORDS_AFR));
static STOPWORDS_LAT: LazyLock<HashSet<&str>> = LazyLock::new(|| make(lat::STOPWORDS_LAT));
static STOPWORDS_SLK: LazyLock<HashSet<&str>> = LazyLock::new(|| make(slk::STOPWORDS_SLK));
static STOPWORDS_CAT: LazyLock<HashSet<&str>> = LazyLock::new(|| make(cat::STOPWORDS_CAT));
static STOPWORDS_TGL: LazyLock<HashSet<&str>> = LazyLock::new(|| make(tgl::STOPWORDS_TGL));
static STOPWORDS_HYE: LazyLock<HashSet<&str>> = LazyLock::new(|| make(hye::STOPWORDS_HYE));
static STOPWORDS_CYM: LazyLock<HashSet<&str>> = LazyLock::new(|| make(cym::STOPWORDS_CYM));

fn make<'a>(words: &[&'a str]) -> HashSet<&'a str> {
    words.iter().copied().collect()
}

impl LexerStopWord {
    pub fn is(word: &str, locale: Option<Lang>) -> bool {
        if let Some(locale) = locale {
            // Word is a stopword (given locale)
            if Self::lang_stopwords(locale).contains(word) {
                return true;
            }
        }

        // Not a stopword, or may not be (default)
        false
    }

    pub fn guess_lang(text: &str, script: Script) -> Option<Lang> {
        tracing::debug!(
            "guessing locale from stopwords for script: {} and text: {}",
            script,
            text
        );

        let script_langs = Self::script_langs(script);

        // Count found stop-words in text for each language
        let (mut likely_count, mut likely_lang) = (0, None);

        // Split the text and consume the iterator
        // Notice: this may seem dirty as we allocate memory, but there may be a lot of \
        //   'script_langs' to iterate over (plus, we need to exhaust the whole list as we \
        //   cannot break early by design). We have noticed a 65% performance increase on \
        //   texts of ~100 characters when collecting the iterator there, with a very low memory \
        //   cost as the strings are references and thus there should be no heap allocation. We \
        //   expect this gain to increase even further for longer texts.
        let text_split = text.split_whitespace().collect::<Vec<&str>>();

        for script_lang in script_langs {
            let lang_stopwords = Self::lang_stopwords(*script_lang);

            if !lang_stopwords.is_empty() {
                let mut lang_count = 0;

                // This is a simple split, that does not take into account uppercase letters and \
                //   punctuation, as to prevent memory allocations and other heavy operations. \
                //   Trade-offs are made as this is a best-effort last-resort check.
                for word in &text_split {
                    if lang_stopwords.contains(word) {
                        lang_count += 1;
                    }
                }

                // Found stopwords for this locale in text?
                if lang_count > 0 {
                    tracing::debug!(
                        "got {} common stopwords in guess for locale: {}",
                        lang_count,
                        script_lang
                    );

                    if lang_count > likely_count {
                        likely_count = lang_count;
                        likely_lang = Some(*script_lang);
                    }
                }
            }
        }

        // Return most likely locale (if any)
        likely_lang
    }

    fn lang_stopwords(lang: Lang) -> &'static HashSet<&'static str> {
        match lang {
            Lang::Epo => &*STOPWORDS_EPO,
            Lang::Eng => &*STOPWORDS_ENG,
            Lang::Rus => &*STOPWORDS_RUS,
            Lang::Cmn => &*STOPWORDS_CMN,
            Lang::Spa => &*STOPWORDS_SPA,
            Lang::Por => &*STOPWORDS_POR,
            Lang::Ita => &*STOPWORDS_ITA,
            Lang::Ben => &*STOPWORDS_BEN,
            Lang::Fra => &*STOPWORDS_FRA,
            Lang::Deu => &*STOPWORDS_DEU,
            Lang::Ukr => &*STOPWORDS_UKR,
            Lang::Kat => &*STOPWORDS_KAT,
            Lang::Ara => &*STOPWORDS_ARA,
            Lang::Hin => &*STOPWORDS_HIN,
            Lang::Jpn => &*STOPWORDS_JPN,
            Lang::Heb => &*STOPWORDS_HEB,
            Lang::Yid => &*STOPWORDS_YID,
            Lang::Pol => &*STOPWORDS_POL,
            Lang::Amh => &*STOPWORDS_AMH,
            Lang::Jav => &*STOPWORDS_JAV,
            Lang::Kor => &*STOPWORDS_KOR,
            Lang::Nob => &*STOPWORDS_NOB,
            Lang::Dan => &*STOPWORDS_DAN,
            Lang::Swe => &*STOPWORDS_SWE,
            Lang::Fin => &*STOPWORDS_FIN,
            Lang::Tur => &*STOPWORDS_TUR,
            Lang::Nld => &*STOPWORDS_NLD,
            Lang::Hun => &*STOPWORDS_HUN,
            Lang::Ces => &*STOPWORDS_CES,
            Lang::Ell => &*STOPWORDS_ELL,
            Lang::Bul => &*STOPWORDS_BUL,
            Lang::Bel => &*STOPWORDS_BEL,
            Lang::Mar => &*STOPWORDS_MAR,
            Lang::Kan => &*STOPWORDS_KAN,
            Lang::Ron => &*STOPWORDS_RON,
            Lang::Slv => &*STOPWORDS_SLV,
            Lang::Hrv => &*STOPWORDS_HRV,
            Lang::Srp => &*STOPWORDS_SRP,
            Lang::Mkd => &*STOPWORDS_MKD,
            Lang::Lit => &*STOPWORDS_LIT,
            Lang::Lav => &*STOPWORDS_LAV,
            Lang::Est => &*STOPWORDS_EST,
            Lang::Tam => &*STOPWORDS_TAM,
            Lang::Vie => &*STOPWORDS_VIE,
            Lang::Urd => &*STOPWORDS_URD,
            Lang::Tha => &*STOPWORDS_THA,
            Lang::Guj => &*STOPWORDS_GUJ,
            Lang::Uzb => &*STOPWORDS_UZB,
            Lang::Pan => &*STOPWORDS_PAN,
            Lang::Aze => &*STOPWORDS_AZE,
            Lang::Ind => &*STOPWORDS_IND,
            Lang::Tel => &*STOPWORDS_TEL,
            Lang::Pes => &*STOPWORDS_PES,
            Lang::Mal => &*STOPWORDS_MAL,
            Lang::Ori => &*STOPWORDS_ORI,
            Lang::Mya => &*STOPWORDS_MYA,
            Lang::Nep => &*STOPWORDS_NEP,
            Lang::Sin => &*STOPWORDS_SIN,
            Lang::Khm => &*STOPWORDS_KHM,
            Lang::Tuk => &*STOPWORDS_TUK,
            Lang::Aka => &*STOPWORDS_AKA,
            Lang::Zul => &*STOPWORDS_ZUL,
            Lang::Sna => &*STOPWORDS_SNA,
            Lang::Afr => &*STOPWORDS_AFR,
            Lang::Lat => &*STOPWORDS_LAT,
            Lang::Slk => &*STOPWORDS_SLK,
            Lang::Cat => &*STOPWORDS_CAT,
            Lang::Tgl => &*STOPWORDS_TGL,
            Lang::Hye => &*STOPWORDS_HYE,
            Lang::Cym => &*STOPWORDS_CYM,
        }
    }

    fn script_langs(script: Script) -> &'static [Lang] {
        match script {
            Script::Latin => &[
                Lang::Spa,
                Lang::Eng,
                Lang::Por,
                Lang::Ind,
                Lang::Fra,
                Lang::Deu,
                Lang::Jav,
                Lang::Vie,
                Lang::Ita,
                Lang::Tur,
                Lang::Pol,
                Lang::Ron,
                Lang::Hrv,
                Lang::Nld,
                Lang::Uzb,
                Lang::Hun,
                Lang::Aze,
                Lang::Ces,
                Lang::Zul,
                Lang::Swe,
                Lang::Aka,
                Lang::Sna,
                Lang::Afr,
                Lang::Fin,
                Lang::Tuk,
                Lang::Dan,
                Lang::Nob,
                Lang::Lit,
                Lang::Slv,
                Lang::Epo,
                Lang::Lav,
                Lang::Est,
                Lang::Lat,
                Lang::Slk,
                Lang::Cat,
                Lang::Tgl,
                Lang::Cym,
            ],
            Script::Cyrillic => &[
                Lang::Rus,
                Lang::Ukr,
                Lang::Srp,
                Lang::Aze,
                Lang::Bel,
                Lang::Bul,
                Lang::Tuk,
                Lang::Mkd,
            ],
            Script::Arabic => &[Lang::Ara, Lang::Urd, Lang::Pes],
            Script::Armenian => &[Lang::Hye],
            Script::Devanagari => &[Lang::Hin, Lang::Mar, Lang::Nep],
            Script::Ethiopic => &[Lang::Amh],
            Script::Hebrew => &[Lang::Heb, Lang::Yid],
            Script::Mandarin => &[Lang::Cmn],
            Script::Bengali => &[Lang::Ben],
            Script::Hangul => &[Lang::Kor],
            Script::Georgian => &[Lang::Kat],
            Script::Greek => &[Lang::Ell],
            Script::Kannada => &[Lang::Kan],
            Script::Tamil => &[Lang::Tam],
            Script::Thai => &[Lang::Tha],
            Script::Gujarati => &[Lang::Guj],
            Script::Gurmukhi => &[Lang::Pan],
            Script::Telugu => &[Lang::Tel],
            Script::Malayalam => &[Lang::Mal],
            Script::Oriya => &[Lang::Ori],
            Script::Myanmar => &[Lang::Mya],
            Script::Sinhala => &[Lang::Sin],
            Script::Khmer => &[Lang::Khm],
            Script::Katakana | Script::Hiragana => &[Lang::Jpn],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_detects_stopwords() {
        assert!(!LexerStopWord::is("the", None));
        assert!(LexerStopWord::is("the", Some(Lang::Eng)));
        assert!(!LexerStopWord::is("fox", Some(Lang::Eng)));
        assert!(!LexerStopWord::is("bonjour", Some(Lang::Fra)));
        assert!(LexerStopWord::is("ici", Some(Lang::Fra)));
        assert!(LexerStopWord::is("adéu", Some(Lang::Cat)));
    }

    #[test]
    fn it_guesses_language() {
        assert_eq!(
            LexerStopWord::guess_lang(
                "I believe there is an extremely simple way to whip climate change.",
                Script::Latin
            ),
            Some(Lang::Eng)
        );
        assert_eq!(
            LexerStopWord::guess_lang(
                "permettre aux pharmaciens de délivrer sous certaines conditions des médicaments",
                Script::Latin
            ),
            Some(Lang::Fra)
        );
        assert_eq!(
            LexerStopWord::guess_lang(
                "Tarlós István főpolgármester utasítása alapján a Főváros a Budapest Portálon",
                Script::Latin
            ),
            Some(Lang::Hun)
        );
        assert_eq!(
            LexerStopWord::guess_lang(
                "Tots els éssers humans neixen lliures i iguals en dignitat i en drets.",
                Script::Latin
            ),
            Some(Lang::Cat)
        );
        assert_eq!(
            LexerStopWord::guess_lang("aux", Script::Latin),
            Some(Lang::Fra)
        );
        assert_eq!(
            LexerStopWord::guess_lang("feefeffd zd", Script::Latin),
            None
        );
    }
}

#[cfg(all(feature = "benchmark", test))]
mod benches {
    extern crate test;

    use super::*;
    use test::Bencher;

    #[bench]
    fn bench_detect_stopwords_not_found(b: &mut Bencher) {
        b.iter(|| LexerStopWord::is("fox", Some(Lang::Eng)));
    }

    #[bench]
    fn bench_detect_stopwords_found(b: &mut Bencher) {
        b.iter(|| LexerStopWord::is("the", Some(Lang::Eng)));
    }

    #[bench]
    fn bench_guess_language_latin(b: &mut Bencher) {
        b.iter(|| {
            LexerStopWord::guess_lang(
                "I believe there is an extremely simple way to whip climate change.",
                Script::Latin,
            )
        });
    }

    #[bench]
    fn bench_guess_language_mandarin(b: &mut Bencher) {
        b.iter(|| LexerStopWord::guess_lang("快狐跨懒狗", Script::Mandarin));
    }
}
