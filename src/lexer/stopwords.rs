// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use hashbrown::HashSet;
use whatlang::{Lang, Script};

use crate::stopwords::*;

pub struct LexerStopWord;

// Recursion group #1 (10 items)
lazy_static! {
    static ref STOPWORDS_EPO: HashSet<&'static str> = make(epo::STOPWORDS_EPO);
    static ref STOPWORDS_ENG: HashSet<&'static str> = make(eng::STOPWORDS_ENG);
    static ref STOPWORDS_RUS: HashSet<&'static str> = make(rus::STOPWORDS_RUS);
    static ref STOPWORDS_CMN: HashSet<&'static str> = make(cmn::STOPWORDS_CMN);
    static ref STOPWORDS_SPA: HashSet<&'static str> = make(spa::STOPWORDS_SPA);
    static ref STOPWORDS_POR: HashSet<&'static str> = make(por::STOPWORDS_POR);
    static ref STOPWORDS_ITA: HashSet<&'static str> = make(ita::STOPWORDS_ITA);
    static ref STOPWORDS_BEN: HashSet<&'static str> = make(ben::STOPWORDS_BEN);
    static ref STOPWORDS_FRA: HashSet<&'static str> = make(fra::STOPWORDS_FRA);
    static ref STOPWORDS_DEU: HashSet<&'static str> = make(deu::STOPWORDS_DEU);
}

// Recursion group #2 (10 items)
lazy_static! {
    static ref STOPWORDS_UKR: HashSet<&'static str> = make(ukr::STOPWORDS_UKR);
    static ref STOPWORDS_KAT: HashSet<&'static str> = make(kat::STOPWORDS_KAT);
    static ref STOPWORDS_ARA: HashSet<&'static str> = make(ara::STOPWORDS_ARA);
    static ref STOPWORDS_HIN: HashSet<&'static str> = make(hin::STOPWORDS_HIN);
    static ref STOPWORDS_JPN: HashSet<&'static str> = make(jpn::STOPWORDS_JPN);
    static ref STOPWORDS_HEB: HashSet<&'static str> = make(heb::STOPWORDS_HEB);
    static ref STOPWORDS_YID: HashSet<&'static str> = make(yid::STOPWORDS_YID);
    static ref STOPWORDS_POL: HashSet<&'static str> = make(pol::STOPWORDS_POL);
    static ref STOPWORDS_AMH: HashSet<&'static str> = make(amh::STOPWORDS_AMH);
    static ref STOPWORDS_JAV: HashSet<&'static str> = make(jav::STOPWORDS_JAV);
}

// Recursion group #3 (10 items)
lazy_static! {
    static ref STOPWORDS_KOR: HashSet<&'static str> = make(kor::STOPWORDS_KOR);
    static ref STOPWORDS_NOB: HashSet<&'static str> = make(nob::STOPWORDS_NOB);
    static ref STOPWORDS_DAN: HashSet<&'static str> = make(dan::STOPWORDS_DAN);
    static ref STOPWORDS_SWE: HashSet<&'static str> = make(swe::STOPWORDS_SWE);
    static ref STOPWORDS_FIN: HashSet<&'static str> = make(fin::STOPWORDS_FIN);
    static ref STOPWORDS_TUR: HashSet<&'static str> = make(tur::STOPWORDS_TUR);
    static ref STOPWORDS_NLD: HashSet<&'static str> = make(nld::STOPWORDS_NLD);
    static ref STOPWORDS_HUN: HashSet<&'static str> = make(hun::STOPWORDS_HUN);
    static ref STOPWORDS_CES: HashSet<&'static str> = make(ces::STOPWORDS_CES);
    static ref STOPWORDS_ELL: HashSet<&'static str> = make(ell::STOPWORDS_ELL);
}

// Recursion group #4 (10 items)
lazy_static! {
    static ref STOPWORDS_BUL: HashSet<&'static str> = make(bul::STOPWORDS_BUL);
    static ref STOPWORDS_BEL: HashSet<&'static str> = make(bel::STOPWORDS_BEL);
    static ref STOPWORDS_MAR: HashSet<&'static str> = make(mar::STOPWORDS_MAR);
    static ref STOPWORDS_KAN: HashSet<&'static str> = make(kan::STOPWORDS_KAN);
    static ref STOPWORDS_RON: HashSet<&'static str> = make(ron::STOPWORDS_RON);
    static ref STOPWORDS_SLV: HashSet<&'static str> = make(slv::STOPWORDS_SLV);
    static ref STOPWORDS_HRV: HashSet<&'static str> = make(hrv::STOPWORDS_HRV);
    static ref STOPWORDS_SRP: HashSet<&'static str> = make(srp::STOPWORDS_SRP);
    static ref STOPWORDS_MKD: HashSet<&'static str> = make(mkd::STOPWORDS_MKD);
    static ref STOPWORDS_LIT: HashSet<&'static str> = make(lit::STOPWORDS_LIT);
}

// Recursion group #5 (10 items)
lazy_static! {
    static ref STOPWORDS_LAV: HashSet<&'static str> = make(lav::STOPWORDS_LAV);
    static ref STOPWORDS_EST: HashSet<&'static str> = make(est::STOPWORDS_EST);
    static ref STOPWORDS_TAM: HashSet<&'static str> = make(tam::STOPWORDS_TAM);
    static ref STOPWORDS_VIE: HashSet<&'static str> = make(vie::STOPWORDS_VIE);
    static ref STOPWORDS_URD: HashSet<&'static str> = make(urd::STOPWORDS_URD);
    static ref STOPWORDS_THA: HashSet<&'static str> = make(tha::STOPWORDS_THA);
    static ref STOPWORDS_GUJ: HashSet<&'static str> = make(guj::STOPWORDS_GUJ);
    static ref STOPWORDS_UZB: HashSet<&'static str> = make(uzb::STOPWORDS_UZB);
    static ref STOPWORDS_PAN: HashSet<&'static str> = make(pan::STOPWORDS_PAN);
    static ref STOPWORDS_AZE: HashSet<&'static str> = make(aze::STOPWORDS_AZE);
}

// Recursion group #6 (10 items)
lazy_static! {
    static ref STOPWORDS_IND: HashSet<&'static str> = make(ind::STOPWORDS_IND);
    static ref STOPWORDS_TEL: HashSet<&'static str> = make(tel::STOPWORDS_TEL);
    static ref STOPWORDS_PES: HashSet<&'static str> = make(pes::STOPWORDS_PES);
    static ref STOPWORDS_MAL: HashSet<&'static str> = make(mal::STOPWORDS_MAL);
    static ref STOPWORDS_ORI: HashSet<&'static str> = make(ori::STOPWORDS_ORI);
    static ref STOPWORDS_MYA: HashSet<&'static str> = make(mya::STOPWORDS_MYA);
    static ref STOPWORDS_NEP: HashSet<&'static str> = make(nep::STOPWORDS_NEP);
    static ref STOPWORDS_SIN: HashSet<&'static str> = make(sin::STOPWORDS_SIN);
    static ref STOPWORDS_KHM: HashSet<&'static str> = make(khm::STOPWORDS_KHM);
    static ref STOPWORDS_TUK: HashSet<&'static str> = make(tuk::STOPWORDS_TUK);
}

// Recursion group #7 (7 items)
lazy_static! {
    static ref STOPWORDS_AKA: HashSet<&'static str> = make(aka::STOPWORDS_AKA);
    static ref STOPWORDS_ZUL: HashSet<&'static str> = make(zul::STOPWORDS_ZUL);
    static ref STOPWORDS_SNA: HashSet<&'static str> = make(sna::STOPWORDS_SNA);
    static ref STOPWORDS_AFR: HashSet<&'static str> = make(afr::STOPWORDS_AFR);
    static ref STOPWORDS_LAT: HashSet<&'static str> = make(lat::STOPWORDS_LAT);
    static ref STOPWORDS_SLK: HashSet<&'static str> = make(slk::STOPWORDS_SLK);
    static ref STOPWORDS_CAT: HashSet<&'static str> = make(cat::STOPWORDS_CAT);
}

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
        debug!(
            "guessing locale from stopwords for script: {} and text: {}",
            script, text
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
                    debug!(
                        "got {} common stopwords in guess for locale: {}",
                        lang_count, script_lang
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
