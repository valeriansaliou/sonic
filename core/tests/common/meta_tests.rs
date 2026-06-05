// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

macro_rules! test_ingest_then_query {
    (
        $(normalization_config: { $($nc_field:ident: $nc_value:expr$(,)?)+ },)?
        $(search_config: { $($sc_field:ident: $sc_value:expr$(,)?)+ },)?
        push: $sentence:tt $([$check:ident])* $(LANG($ingest_lang:expr))?,
        query: $examples:tt $(LANG($query_lang:expr))? $(,)?
    ) => {
        init_logging();
        #[allow(unused_mut)]
        let mut executor = make_test_executor();

        {
            #[allow(unused)]
            let app_conf = std::sync::Arc::get_mut(&mut executor.app_conf).unwrap();
            $($(app_conf.normalization.$nc_field = $nc_value;)+)?
        }
        $($(executor.fst_pool.fst_action_config.$sc_field = $sc_value;)+)?

        #[allow(unused_mut, unused_assignments)]
        let mut ingest_lang = "none"; // For logging purposes.
        $(ingest_lang = $ingest_lang;)?

        #[allow(unused_mut, unused_assignments)]
        let mut query_lang = "none"; // For logging purposes.
        $(query_lang = $query_lang;)?

        exec!(executor -> PUSH "messages" "user:1" "chat:1" $sentence $(LANG($ingest_lang))?);
        exec!(executor -> TRIGGER consolidate);

        $(test_ingest_then_query!(internal_ $check: executor, $sentence);)*

        for (needle, should_match) in $examples.into_iter() {
            assert!(!needle.contains("{"), "Needle shouldn’t contain '{{', make sure you formatted the string correctly.");

            let response = exec!(executor -> QUERY "messages" "user:1" needle $(LANG($query_lang))?);
            if should_match {
                assert_eq!(response, ["chat:1"], "Did not find {needle:?} LANG({query_lang}) in {:?} LANG({ingest_lang})", $sentence);
            } else {
                assert_eq!(response, vec![] as Vec<&str>, "Found {needle:?} LANG({query_lang}) in {:?} LANG({ingest_lang})", $sentence);
            }
        }

        // Drop the executor so it’s not debug-printed multiple times on
        // failure if this macro was invoked multiple times in the same test.
        // We could scope this code in a code block (`{ … }`) but it’d be an
        // implementation detail one might remove in the future unknowingly.
        drop(executor);
    };

    (internal_ ensure_no_stopword: $executor:tt, $sentence:ident) => {
        // Sanity check: ensure no stopword was provided (could make
        // examples pass for the wrong reason).
        assert_eq!(
            exec!($executor -> COUNT "messages" "user:1" "chat:1"),
            Ok($sentence.split_ascii_whitespace().count() as u32)
        );
    }
}
pub(crate) use test_ingest_then_query;
