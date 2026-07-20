// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

// All stopwords are sourced from: https://github.com/stopwords-iso
// Last update: 7th March 2019

pub mod afr;
pub mod aka;
pub mod amh;
pub mod ara;
pub mod aze;
pub mod bel;
pub mod ben;
pub mod bul;
pub mod cat;
pub mod ces;
pub mod cmn;
pub mod cym;
pub mod dan;
pub mod deu;
pub mod ell;
pub mod eng;
pub mod epo;
pub mod est;
pub mod fin;
pub mod fra;
pub mod guj;
pub mod heb;
pub mod hin;
pub mod hrv;
pub mod hun;
pub mod hye;
pub mod ind;
pub mod ita;
pub mod jav;
pub mod jpn;
pub mod kan;
pub mod kat;
pub mod khm;
pub mod kor;
pub mod lat;
pub mod lav;
pub mod lit;
pub mod mal;
pub mod mar;
pub mod mkd;
pub mod mya;
pub mod nep;
pub mod nld;
pub mod nob;
pub mod ori;
pub mod pan;
pub mod pes;
pub mod pol;
pub mod por;
pub mod ron;
pub mod rus;
pub mod sin;
pub mod slk;
pub mod slv;
pub mod sna;
pub mod spa;
pub mod srp;
pub mod swe;
pub mod tam;
pub mod tel;
pub mod tgl;
pub mod tha;
pub mod tuk;
pub mod tur;
pub mod ukr;
pub mod urd;
pub mod uzb;
pub mod vie;
pub mod yid;
pub mod zul;

#[allow(dead_code)]
pub fn lang_stopwords(lang: whatlang::Lang) -> &'static [&'static str] {
    use whatlang::Lang;

    match lang {
        Lang::Afr => afr::STOPWORDS_AFR,
        Lang::Aka => aka::STOPWORDS_AKA,
        Lang::Amh => amh::STOPWORDS_AMH,
        Lang::Ara => ara::STOPWORDS_ARA,
        Lang::Aze => aze::STOPWORDS_AZE,
        Lang::Bel => bel::STOPWORDS_BEL,
        Lang::Ben => ben::STOPWORDS_BEN,
        Lang::Bul => bul::STOPWORDS_BUL,
        Lang::Cat => cat::STOPWORDS_CAT,
        Lang::Ces => ces::STOPWORDS_CES,
        Lang::Cmn => cmn::STOPWORDS_CMN,
        Lang::Cym => cym::STOPWORDS_CYM,
        Lang::Dan => dan::STOPWORDS_DAN,
        Lang::Deu => deu::STOPWORDS_DEU,
        Lang::Ell => ell::STOPWORDS_ELL,
        Lang::Eng => eng::STOPWORDS_ENG,
        Lang::Epo => epo::STOPWORDS_EPO,
        Lang::Est => est::STOPWORDS_EST,
        Lang::Fin => fin::STOPWORDS_FIN,
        Lang::Fra => fra::STOPWORDS_FRA,
        Lang::Guj => guj::STOPWORDS_GUJ,
        Lang::Heb => heb::STOPWORDS_HEB,
        Lang::Hin => hin::STOPWORDS_HIN,
        Lang::Hrv => hrv::STOPWORDS_HRV,
        Lang::Hun => hun::STOPWORDS_HUN,
        Lang::Hye => hye::STOPWORDS_HYE,
        Lang::Ind => ind::STOPWORDS_IND,
        Lang::Ita => ita::STOPWORDS_ITA,
        Lang::Jav => jav::STOPWORDS_JAV,
        Lang::Jpn => jpn::STOPWORDS_JPN,
        Lang::Kan => kan::STOPWORDS_KAN,
        Lang::Kat => kat::STOPWORDS_KAT,
        Lang::Khm => khm::STOPWORDS_KHM,
        Lang::Kor => kor::STOPWORDS_KOR,
        Lang::Lat => lat::STOPWORDS_LAT,
        Lang::Lav => lav::STOPWORDS_LAV,
        Lang::Lit => lit::STOPWORDS_LIT,
        Lang::Mal => mal::STOPWORDS_MAL,
        Lang::Mar => mar::STOPWORDS_MAR,
        Lang::Mkd => mkd::STOPWORDS_MKD,
        Lang::Mya => mya::STOPWORDS_MYA,
        Lang::Nep => nep::STOPWORDS_NEP,
        Lang::Nld => nld::STOPWORDS_NLD,
        Lang::Nob => nob::STOPWORDS_NOB,
        Lang::Ori => ori::STOPWORDS_ORI,
        Lang::Pan => pan::STOPWORDS_PAN,
        Lang::Pes => pes::STOPWORDS_PES,
        Lang::Pol => pol::STOPWORDS_POL,
        Lang::Por => por::STOPWORDS_POR,
        Lang::Ron => ron::STOPWORDS_RON,
        Lang::Rus => rus::STOPWORDS_RUS,
        Lang::Sin => sin::STOPWORDS_SIN,
        Lang::Slk => slk::STOPWORDS_SLK,
        Lang::Slv => slv::STOPWORDS_SLV,
        Lang::Sna => sna::STOPWORDS_SNA,
        Lang::Spa => spa::STOPWORDS_SPA,
        Lang::Srp => srp::STOPWORDS_SRP,
        Lang::Swe => swe::STOPWORDS_SWE,
        Lang::Tam => tam::STOPWORDS_TAM,
        Lang::Tel => tel::STOPWORDS_TEL,
        Lang::Tgl => tgl::STOPWORDS_TGL,
        Lang::Tha => tha::STOPWORDS_THA,
        Lang::Tuk => tuk::STOPWORDS_TUK,
        Lang::Tur => tur::STOPWORDS_TUR,
        Lang::Ukr => ukr::STOPWORDS_UKR,
        Lang::Urd => urd::STOPWORDS_URD,
        Lang::Uzb => uzb::STOPWORDS_UZB,
        Lang::Vie => vie::STOPWORDS_VIE,
        Lang::Yid => yid::STOPWORDS_YID,
        Lang::Zul => zul::STOPWORDS_ZUL,
    }
}

#[cfg(test)]
#[test]
fn test_stopwords_normalization() {
    use unicode_normalization::UnicodeNormalization as _;

    for lang in whatlang::Lang::all() {
        for &stopword in lang_stopwords(*lang) {
            let lowercase = stopword.to_lowercase();
            if lowercase.as_str() != stopword {
                eprintln!("{lang:?}/lower: {stopword:?}\n        -> {lowercase:?}");
            }
            assert_eq!(
                stopword,
                stopword.to_lowercase().as_str(),
                "Wrong casing (lang: {})",
                lang.eng_name()
            );

            let nfkc = stopword.nfkc().to_string();
            if nfkc.as_str() != stopword {
                eprintln!("{lang:?}/NFKC: {stopword:?}\n       -> {nfkc:?}");
            }
            assert_eq!(
                stopword,
                stopword.nfkc().to_string().as_str(),
                "Wrong Unicode normalization (lang: {})",
                lang.eng_name()
            );
        }
    }
}
