// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

macro_rules! test_ingest_then_query {
    ($sentence:tt $([$option:tt])* $(LANG($ingest_lang:expr))?, $examples:tt $(LANG($query_lang:expr))?) => {
        init_logging();
        let executor = make_test_executor();

        exec!(executor -> PUSH "messages" "user:1" "chat:1" $sentence $(LANG($ingest_lang))?);
        exec!(executor -> TRIGGER consolidate);

        $(test_ingest_then_query!(internal_ $option: executor, $sentence);)*

        for (needle, should_match) in $examples.into_iter() {
            assert!(!needle.contains("{"), "Needle shouldn’t contain '{{', make sure you formatted the string correctly.");

            let response = exec!(executor -> QUERY "messages" "user:1" needle $(LANG($query_lang))?);
            if should_match {
                assert_eq!(response, ["chat:1"], "Did not find {needle:?} in {:?}", $sentence);
            } else {
                assert_eq!(response, vec![] as Vec<&str>, "Found {needle:?} in {:?}", $sentence);
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
