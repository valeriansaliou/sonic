// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use whatlang::Lang;

pub fn snowball_algorithm(locale: &Lang) -> Option<snowball::Algorithm> {
    // NOTE: This is just so we notice when `snowball` adds more algorithms.
    #[cfg(debug_assertions)]
    match snowball::Algorithm::English {
        snowball::Algorithm::Arabic
        | snowball::Algorithm::Armenian
        | snowball::Algorithm::Basque // Unsupported (ISO 639-3: eus)
        | snowball::Algorithm::Catalan
        | snowball::Algorithm::Danish
        | snowball::Algorithm::Dutch
        | snowball::Algorithm::DutchPorter // Dutch is better
        | snowball::Algorithm::English
        | snowball::Algorithm::Esperanto
        | snowball::Algorithm::Estonian
        | snowball::Algorithm::Finnish
        | snowball::Algorithm::French
        | snowball::Algorithm::German
        | snowball::Algorithm::Greek
        | snowball::Algorithm::Hindi
        | snowball::Algorithm::Hungarian
        | snowball::Algorithm::Indonesian
        | snowball::Algorithm::Irish // Unsupported (ISO 639-3: gle)
        | snowball::Algorithm::Italian
        | snowball::Algorithm::Lithuanian
        | snowball::Algorithm::Lovins // No need for such fallback
        | snowball::Algorithm::Nepali
        | snowball::Algorithm::Norwegian
        | snowball::Algorithm::Porter // No need for such fallback
        | snowball::Algorithm::Portuguese
        | snowball::Algorithm::Romanian
        | snowball::Algorithm::Russian
        | snowball::Algorithm::Serbian
        | snowball::Algorithm::Spanish
        | snowball::Algorithm::Swedish
        | snowball::Algorithm::Tamil
        | snowball::Algorithm::Turkish
        | snowball::Algorithm::Yiddish => {}
    }

    match locale {
        Lang::Ara => Some(snowball::Algorithm::Arabic),
        Lang::Cat => Some(snowball::Algorithm::Catalan),
        Lang::Dan => Some(snowball::Algorithm::Danish),
        Lang::Deu => Some(snowball::Algorithm::German),
        Lang::Ell => Some(snowball::Algorithm::Greek),
        Lang::Eng => Some(snowball::Algorithm::English),
        Lang::Epo => Some(snowball::Algorithm::Esperanto),
        Lang::Est => Some(snowball::Algorithm::Estonian),
        Lang::Fin => Some(snowball::Algorithm::Finnish),
        Lang::Fra => Some(snowball::Algorithm::French),
        Lang::Hin => Some(snowball::Algorithm::Hindi),
        Lang::Hun => Some(snowball::Algorithm::Hungarian),
        Lang::Hye => Some(snowball::Algorithm::Armenian),
        Lang::Ind => Some(snowball::Algorithm::Indonesian),
        Lang::Ita => Some(snowball::Algorithm::Italian),
        Lang::Lit => Some(snowball::Algorithm::Lithuanian),
        Lang::Nep => Some(snowball::Algorithm::Nepali),
        Lang::Nld => Some(snowball::Algorithm::Dutch),
        Lang::Nob => Some(snowball::Algorithm::Norwegian),
        Lang::Por => Some(snowball::Algorithm::Portuguese),
        Lang::Ron => Some(snowball::Algorithm::Romanian),
        Lang::Rus => Some(snowball::Algorithm::Russian),
        Lang::Spa => Some(snowball::Algorithm::Spanish),
        Lang::Srp => Some(snowball::Algorithm::Serbian),
        Lang::Swe => Some(snowball::Algorithm::Swedish),
        Lang::Tam => Some(snowball::Algorithm::Tamil),
        Lang::Tur => Some(snowball::Algorithm::Turkish),
        Lang::Yid => Some(snowball::Algorithm::Yiddish),
        Lang::Afr
        | Lang::Aka
        | Lang::Amh
        | Lang::Aze
        | Lang::Bel
        | Lang::Ben
        | Lang::Bul
        | Lang::Ces
        | Lang::Cmn
        | Lang::Cym
        | Lang::Guj
        | Lang::Heb
        | Lang::Hrv
        | Lang::Jav
        | Lang::Jpn
        | Lang::Kan
        | Lang::Kat
        | Lang::Khm
        | Lang::Kor
        | Lang::Lat
        | Lang::Lav
        | Lang::Mal
        | Lang::Mar
        | Lang::Mkd
        | Lang::Mya
        | Lang::Ori
        | Lang::Pan
        | Lang::Pes
        | Lang::Pol
        | Lang::Sin
        | Lang::Slk
        | Lang::Slv
        | Lang::Sna
        | Lang::Tel
        | Lang::Tgl
        | Lang::Tha
        | Lang::Tuk
        | Lang::Ukr
        | Lang::Urd
        | Lang::Uzb
        | Lang::Vie
        | Lang::Zul => None,
    }
}
