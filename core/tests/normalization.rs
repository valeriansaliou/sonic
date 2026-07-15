// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;

use crate::common::*;

/// Ensures Unicode normalization configuration impacts what is indexed.
#[test]
fn test_unicode_normalization_ingest() {
    use sonic::config::UnicodeNormalization;

    let café_nfc = "caf\u{E9}";
    let café_nfd = "cafe\u{301}";

    fn test(unicode_normalization: Option<UnicodeNormalization>, ingest: &str, expected: &str) {
        init_logging();
        #[allow(unused_mut)]
        let mut executor = make_test_executor();

        {
            #[allow(unused)]
            let app_conf = std::sync::Arc::get_mut(&mut executor.app_conf).unwrap();
            app_conf.normalization = sonic::config::ConfigNormalization {
                unicode_normalization,
                diacritic_folding_enabled: false,
                stemming_enabled: false,
            };
        }

        exec!(executor -> PUSH "messages" "user:1" "chat:1" ingest LANG("none"));
        exec!(executor -> TRIGGER consolidate);

        let index = exec!(executor -> LIST "messages" "user:1" LIMIT(99));
        assert_eq!(
            index.as_slice(),
            &[expected.to_owned()],
            "normalization: {unicode_normalization:?}"
        );
    }

    test(None, café_nfc, café_nfc);
    test(None, café_nfd, café_nfd);

    test(Some(UnicodeNormalization::Nfc), café_nfd, café_nfc);
}

/// See <https://github.com/valeriansaliou/sonic/issues/370>.
#[test]
fn test_unicode_normalization_query() {
    use unicode_normalization::UnicodeNormalization as _;

    let café = "café".nfc().to_string();
    let café = café.as_str();

    test_ingest_then_query!(
        normalization_config: { diacritic_folding_enabled: false },
        push: café LANG("none"),
        query: [
            ("café".nfc().to_string().as_str(), true),
            ("café".nfd().to_string().as_str(), true),
        ] LANG("none")
    );
}
