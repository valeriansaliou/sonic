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
    static ref STOPWORDS_ARB: HashSet<&'static str> = make(arb::STOPWORDS_ARB);
    static ref STOPWORDS_HIN: HashSet<&'static str> = make(hin::STOPWORDS_HIN);
    static ref STOPWORDS_JPN: HashSet<&'static str> = make(jpn::STOPWORDS_JPN);
    static ref STOPWORDS_HEB: HashSet<&'static str> = make(heb::STOPWORDS_HEB);
    static ref STOPWORDS_YDD: HashSet<&'static str> = make(ydd::STOPWORDS_YDD);
    static ref STOPWORDS_POL: HashSet<&'static str> = make(pol::STOPWORDS_POL);
    static ref STOPWORDS_AMH: HashSet<&'static str> = make(amh::STOPWORDS_AMH);
    static ref STOPWORDS_TIR: HashSet<&'static str> = make(tir::STOPWORDS_TIR);
}

// Recursion group #3 (10 items)
lazy_static! {
    static ref STOPWORDS_JAV: HashSet<&'static str> = make(jav::STOPWORDS_JAV);
    static ref STOPWORDS_KOR: HashSet<&'static str> = make(kor::STOPWORDS_KOR);
    static ref STOPWORDS_NOB: HashSet<&'static str> = make(nob::STOPWORDS_NOB);
    static ref STOPWORDS_NNO: HashSet<&'static str> = make(nno::STOPWORDS_NNO);
    static ref STOPWORDS_DAN: HashSet<&'static str> = make(dan::STOPWORDS_DAN);
    static ref STOPWORDS_SWE: HashSet<&'static str> = make(swe::STOPWORDS_SWE);
    static ref STOPWORDS_FIN: HashSet<&'static str> = make(fin::STOPWORDS_FIN);
    static ref STOPWORDS_TUR: HashSet<&'static str> = make(tur::STOPWORDS_TUR);
    static ref STOPWORDS_NLD: HashSet<&'static str> = make(nld::STOPWORDS_NLD);
    static ref STOPWORDS_HUN: HashSet<&'static str> = make(hun::STOPWORDS_HUN);
}

// Recursion group #4 (10 items)
lazy_static! {
    static ref STOPWORDS_CES: HashSet<&'static str> = make(ces::STOPWORDS_CES);
    static ref STOPWORDS_ELL: HashSet<&'static str> = make(ell::STOPWORDS_ELL);
    static ref STOPWORDS_BUL: HashSet<&'static str> = make(bul::STOPWORDS_BUL);
    static ref STOPWORDS_BEL: HashSet<&'static str> = make(bel::STOPWORDS_BEL);
    static ref STOPWORDS_MAR: HashSet<&'static str> = make(mar::STOPWORDS_MAR);
    static ref STOPWORDS_KAN: HashSet<&'static str> = make(kan::STOPWORDS_KAN);
    static ref STOPWORDS_RON: HashSet<&'static str> = make(ron::STOPWORDS_RON);
    static ref STOPWORDS_SLV: HashSet<&'static str> = make(slv::STOPWORDS_SLV);
    static ref STOPWORDS_HRV: HashSet<&'static str> = make(hrv::STOPWORDS_HRV);
    static ref STOPWORDS_SRP: HashSet<&'static str> = make(srp::STOPWORDS_SRP);
}

// Recursion group #5 (10 items)
lazy_static! {
    static ref STOPWORDS_MKD: HashSet<&'static str> = make(mkd::STOPWORDS_MKD);
    static ref STOPWORDS_LIT: HashSet<&'static str> = make(lit::STOPWORDS_LIT);
    static ref STOPWORDS_LAV: HashSet<&'static str> = make(lav::STOPWORDS_LAV);
    static ref STOPWORDS_EST: HashSet<&'static str> = make(est::STOPWORDS_EST);
    static ref STOPWORDS_TAM: HashSet<&'static str> = make(tam::STOPWORDS_TAM);
    static ref STOPWORDS_VIE: HashSet<&'static str> = make(vie::STOPWORDS_VIE);
    static ref STOPWORDS_URD: HashSet<&'static str> = make(urd::STOPWORDS_URD);
    static ref STOPWORDS_THA: HashSet<&'static str> = make(tha::STOPWORDS_THA);
    static ref STOPWORDS_GUJ: HashSet<&'static str> = make(guj::STOPWORDS_GUJ);
    static ref STOPWORDS_UZB: HashSet<&'static str> = make(uzb::STOPWORDS_UZB);
}

// Recursion group #6 (10 items)
lazy_static! {
    static ref STOPWORDS_PAN: HashSet<&'static str> = make(pan::STOPWORDS_PAN);
    static ref STOPWORDS_AZJ: HashSet<&'static str> = make(azj::STOPWORDS_AZJ);
    static ref STOPWORDS_IND: HashSet<&'static str> = make(ind::STOPWORDS_IND);
    static ref STOPWORDS_TEL: HashSet<&'static str> = make(tel::STOPWORDS_TEL);
    static ref STOPWORDS_PES: HashSet<&'static str> = make(pes::STOPWORDS_PES);
    static ref STOPWORDS_MAL: HashSet<&'static str> = make(mal::STOPWORDS_MAL);
    static ref STOPWORDS_HAU: HashSet<&'static str> = make(hau::STOPWORDS_HAU);
    static ref STOPWORDS_ORI: HashSet<&'static str> = make(ori::STOPWORDS_ORI);
    static ref STOPWORDS_MYA: HashSet<&'static str> = make(mya::STOPWORDS_MYA);
    static ref STOPWORDS_BHO: HashSet<&'static str> = make(bho::STOPWORDS_BHO);
}

// Recursion group #7 (10 items)
lazy_static! {
    static ref STOPWORDS_TGL: HashSet<&'static str> = make(tgl::STOPWORDS_TGL);
    static ref STOPWORDS_YOR: HashSet<&'static str> = make(yor::STOPWORDS_YOR);
    static ref STOPWORDS_MAI: HashSet<&'static str> = make(mai::STOPWORDS_MAI);
    static ref STOPWORDS_ORM: HashSet<&'static str> = make(orm::STOPWORDS_ORM);
    static ref STOPWORDS_IBO: HashSet<&'static str> = make(ibo::STOPWORDS_IBO);
    static ref STOPWORDS_CEB: HashSet<&'static str> = make(ceb::STOPWORDS_CEB);
    static ref STOPWORDS_KUR: HashSet<&'static str> = make(kur::STOPWORDS_KUR);
    static ref STOPWORDS_MLG: HashSet<&'static str> = make(mlg::STOPWORDS_MLG);
    static ref STOPWORDS_SKR: HashSet<&'static str> = make(skr::STOPWORDS_SKR);
    static ref STOPWORDS_NEP: HashSet<&'static str> = make(nep::STOPWORDS_NEP);
}

// Recursion group #8 (10 items)
lazy_static! {
    static ref STOPWORDS_SIN: HashSet<&'static str> = make(sin::STOPWORDS_SIN);
    static ref STOPWORDS_KHM: HashSet<&'static str> = make(khm::STOPWORDS_KHM);
    static ref STOPWORDS_TUK: HashSet<&'static str> = make(tuk::STOPWORDS_TUK);
    static ref STOPWORDS_SOM: HashSet<&'static str> = make(som::STOPWORDS_SOM);
    static ref STOPWORDS_NYA: HashSet<&'static str> = make(nya::STOPWORDS_NYA);
    static ref STOPWORDS_AKA: HashSet<&'static str> = make(aka::STOPWORDS_AKA);
    static ref STOPWORDS_ZUL: HashSet<&'static str> = make(zul::STOPWORDS_ZUL);
    static ref STOPWORDS_KIN: HashSet<&'static str> = make(kin::STOPWORDS_KIN);
    static ref STOPWORDS_HAT: HashSet<&'static str> = make(hat::STOPWORDS_HAT);
    static ref STOPWORDS_ILO: HashSet<&'static str> = make(ilo::STOPWORDS_ILO);
}

// Recursion group #9 (5 items)
lazy_static! {
    static ref STOPWORDS_RUN: HashSet<&'static str> = make(run::STOPWORDS_RUN);
    static ref STOPWORDS_SNA: HashSet<&'static str> = make(sna::STOPWORDS_SNA);
    static ref STOPWORDS_UIG: HashSet<&'static str> = make(uig::STOPWORDS_UIG);
    static ref STOPWORDS_AFR: HashSet<&'static str> = make(afr::STOPWORDS_AFR);
    static ref STOPWORDS_LAT: HashSet<&'static str> = make(lat::STOPWORDS_LAT);
}

fn make<'a>(words: &[&'a str]) -> HashSet<&'a str> {
    words.into_iter().map(|word| *word).collect()
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
            Lang::Arb => &*STOPWORDS_ARB,
            Lang::Hin => &*STOPWORDS_HIN,
            Lang::Jpn => &*STOPWORDS_JPN,
            Lang::Heb => &*STOPWORDS_HEB,
            Lang::Ydd => &*STOPWORDS_YDD,
            Lang::Pol => &*STOPWORDS_POL,
            Lang::Amh => &*STOPWORDS_AMH,
            Lang::Tir => &*STOPWORDS_TIR,
            Lang::Jav => &*STOPWORDS_JAV,
            Lang::Kor => &*STOPWORDS_KOR,
            Lang::Nob => &*STOPWORDS_NOB,
            Lang::Nno => &*STOPWORDS_NNO,
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
            Lang::Azj => &*STOPWORDS_AZJ,
            Lang::Ind => &*STOPWORDS_IND,
            Lang::Tel => &*STOPWORDS_TEL,
            Lang::Pes => &*STOPWORDS_PES,
            Lang::Mal => &*STOPWORDS_MAL,
            Lang::Hau => &*STOPWORDS_HAU,
            Lang::Ori => &*STOPWORDS_ORI,
            Lang::Mya => &*STOPWORDS_MYA,
            Lang::Bho => &*STOPWORDS_BHO,
            Lang::Tgl => &*STOPWORDS_TGL,
            Lang::Yor => &*STOPWORDS_YOR,
            Lang::Mai => &*STOPWORDS_MAI,
            Lang::Orm => &*STOPWORDS_ORM,
            Lang::Ibo => &*STOPWORDS_IBO,
            Lang::Ceb => &*STOPWORDS_CEB,
            Lang::Kur => &*STOPWORDS_KUR,
            Lang::Mlg => &*STOPWORDS_MLG,
            Lang::Skr => &*STOPWORDS_SKR,
            Lang::Nep => &*STOPWORDS_NEP,
            Lang::Sin => &*STOPWORDS_SIN,
            Lang::Khm => &*STOPWORDS_KHM,
            Lang::Tuk => &*STOPWORDS_TUK,
            Lang::Som => &*STOPWORDS_SOM,
            Lang::Nya => &*STOPWORDS_NYA,
            Lang::Aka => &*STOPWORDS_AKA,
            Lang::Zul => &*STOPWORDS_ZUL,
            Lang::Kin => &*STOPWORDS_KIN,
            Lang::Hat => &*STOPWORDS_HAT,
            Lang::Ilo => &*STOPWORDS_ILO,
            Lang::Run => &*STOPWORDS_RUN,
            Lang::Sna => &*STOPWORDS_SNA,
            Lang::Uig => &*STOPWORDS_UIG,
            Lang::Afr => &*STOPWORDS_AFR,
            Lang::Lat => &*STOPWORDS_LAT,
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
                Lang::Orm,
                Lang::Ron,
                Lang::Hau,
                Lang::Hrv,
                Lang::Nld,
                Lang::Kur,
                Lang::Yor,
                Lang::Uzb,
                Lang::Ibo,
                Lang::Ceb,
                Lang::Tgl,
                Lang::Hun,
                Lang::Azj,
                Lang::Ces,
                Lang::Mlg,
                Lang::Nya,
                Lang::Kin,
                Lang::Zul,
                Lang::Swe,
                Lang::Som,
                Lang::Ilo,
                Lang::Uig,
                Lang::Hat,
                Lang::Aka,
                Lang::Sna,
                Lang::Afr,
                Lang::Fin,
                Lang::Run,
                Lang::Tuk,
                Lang::Dan,
                Lang::Nob,
                Lang::Nno,
                Lang::Lit,
                Lang::Slv,
                Lang::Epo,
                Lang::Lav,
                Lang::Est,
                Lang::Lat,
            ],
            Script::Cyrillic => &[
                Lang::Rus,
                Lang::Ukr,
                Lang::Srp,
                Lang::Azj,
                Lang::Bel,
                Lang::Bul,
                Lang::Tuk,
                Lang::Mkd,
            ],
            Script::Arabic => &[Lang::Arb, Lang::Urd, Lang::Skr, Lang::Uig, Lang::Pes],
            Script::Devanagari => &[Lang::Hin, Lang::Mar, Lang::Mai, Lang::Bho, Lang::Nep],
            Script::Ethiopic => &[Lang::Amh, Lang::Tir],
            Script::Hebrew => &[Lang::Heb, Lang::Ydd],
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
        assert_eq!(LexerStopWord::is("the", None), false);
        assert_eq!(LexerStopWord::is("the", Some(Lang::Eng)), true);
        assert_eq!(LexerStopWord::is("fox", Some(Lang::Eng)), false);
        assert_eq!(LexerStopWord::is("bonjour", Some(Lang::Fra)), false);
        assert_eq!(LexerStopWord::is("ici", Some(Lang::Fra)), true);
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
