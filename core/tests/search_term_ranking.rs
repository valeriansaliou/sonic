// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

//! Feature: Search scoping

mod common;

use std::sync::Arc;

use crate::common::*;

/// Initially, terms in Sonic queries were separated by implicit `AND`s,
/// meaning “rubber chickens” would only match documents mentioning “rubber”
/// AND “chickens”, but not those mentioning “synthetic rubber” (example from
/// [sonic#302]).
///
/// It makes sense for plenty of use cases, and was a pretty efficient
/// optimization, but it also causes a lot of false negatives (see [sonic#262]).
/// Indeed, when coupled with automatic language detection, words that have
/// been skipped during ingestion can be looked up at query time and the fact
/// that they’re missing would completely abort the query.
///
/// Along with other improvements to search results
/// (see [“v1.7.x - Better search results (non-breaking)”]),
/// we decided to implement a simple weighing algorithm which would sort
/// results on relevance rather than aborting early.
///
/// This test is proof that it works.
///
/// [sonic#262]: https://github.com/valeriansaliou/sonic/issues/262
/// [sonic#302]: https://github.com/valeriansaliou/sonic/issues/302
/// [“v1.7.x - Better search results (non-breaking)”]: https://github.com/valeriansaliou/sonic/milestone/20
#[test]
fn test_no_implicit_and() {
    init_logging();
    let mut executor = make_test_executor();

    {
        let app_conf = Arc::get_mut(&mut executor.app_conf).unwrap();
        // Disable stemming to make results more predictable.
        app_conf.normalization.stemming_enabled = false;
    }

    // NOTE: This is NOT legal advice. It is solely for example purposes.
    exec!(
        executor -> PUSH "articles" "default" "article:1"
        "The GDPR applies to any organization—regardless of where it is located—\
        that processes the personal data of people in the European Union or \
        European Economic Area in connection with offering goods or services \
        to them or monitoring their behavior."
    );
    exec!(
        executor -> PUSH "articles" "default" "article:2"
        "GDPR compliance means implementing the technical, organizational, \
        and legal measures required by the GDPR to protect personal data \
        and uphold individuals’ privacy rights."
    );
    exec!(
        executor -> PUSH "articles" "default" "article:3"
        "The European Union establishes regulations and directives that create \
        common legal standards across its member states in areas such as \
        privacy, competition, consumer protection, and digital markets."
    );
    exec!(
        executor -> PUSH "articles" "default" "article:4"
        "Brussels is a major center for technology policy and digital \
        regulation, shaping rules that influence companies and software \
        services worldwide."
    );

    exec!(executor -> TRIGGER consolidate);

    let response = exec!(executor -> QUERY "articles" "default" "GDPR European Union");
    #[rustfmt::skip]
    assert_eq!(response, [
        // First because it mentions both “GDPR” and “European Union”.
        "article:1",
        // Mentions only “European Union” but inserted most recently.
        "article:3",
        // Mentions only “GDPR”.
        "article:2",
    ]);
}

#[test]
fn test_no_implicit_and_with_stopwords() {
    init_logging();
    let mut executor = make_test_executor();

    {
        let app_conf = Arc::get_mut(&mut executor.app_conf).unwrap();
        // Disable stemming to make results more predictable.
        app_conf.normalization.stemming_enabled = false;
    }

    // NOTE: This is NOT legal advice. It is solely for example purposes.
    exec!(executor -> PUSH "movies" "default" "movie:1" "Back to the Future");
    exec!(executor -> PUSH "movies" "default" "movie:2" "Back to the Future Part II");
    exec!(executor -> PUSH "movies" "default" "movie:3" "Back to the Future Part III");
    exec!(executor -> PUSH "movies" "default" "movie:4" "Back to the Future Part IV"); // It doesn’t exist yet…

    exec!(executor -> TRIGGER consolidate);

    {
        let response = exec!(executor -> QUERY "movies" "default" "Back to the Future");
        #[rustfmt::skip]
        assert_eq!(response, [
            // Exact match.
            "movie:1",
            // Then reverse ingestion order.
            "movie:4", "movie:3", "movie:2",
        ]);
    }

    {
        let response = exec!(executor -> QUERY "movies" "default" "Back to the Future Part II");
        // Reverse ingestion order, because `ii` and `part` are stopwords in English.
        assert_eq!(response, ["movie:4", "movie:3", "movie:2", "movie:1"]);
    }

    {
        let response = exec!(executor -> QUERY "movies" "default" "Back to the Future Part III");
        #[rustfmt::skip]
        assert_eq!(response, [
            // Exact match.
            "movie:3",
            // Then reverse ingestion order.
            "movie:4", "movie:2", "movie:1",
        ]);
    }
}

#[test]
fn test_query_limit_with_typos() {
    init_logging();
    let mut executor = make_test_executor();

    {
        let app_conf = Arc::get_mut(&mut executor.app_conf).unwrap();
        // Disable stemming to make results more predictable.
        app_conf.normalization.stemming_enabled = false;
    }

    // NOTE: This is NOT legal advice. It is solely for example purposes.
    exec!(
        executor -> PUSH "articles" "default" "article:1"
        "Under the General Data Protection Regulation (GDPR), “personal data” is defined as any information relating to an identified or identifiable natural person. An identifiable person is one who can be directly or indirectly identified, in particular by reference to identifiers such as a name, identification number, location data, or an online identifier."
    );
    exec!(
        executor -> PUSH "articles" "default" "article:2"
        "The concept is intentionally broad. It covers obvious identifiers like names, email addresses, and national identification numbers, but also extends to data that can indirectly identify someone when combined with other information. This includes IP addresses, device identifiers, and certain behavioural or usage data when they can be linked back to an individual."
    );
    exec!(
        executor -> PUSH "articles" "default" "article:3"
        "Identifiability is assessed in context, meaning that data considered anonymous in one setting may become personal data in another if reasonable means exist to re-identify the person. The GDPR also considers whether identification requires disproportionate effort, taking into account available technology and cost."
    );
    exec!(
        executor -> PUSH "articles" "default" "article:4"
        "Personal data protection under GDPR applies only to living natural persons, not legal entities such as companies. The regulation also distinguishes between ordinary personal data and “special categories” of personal data, which include sensitive information like health data, biometric data, or data revealing racial or ethnic origin, and which are subject to stricter processing conditions."
    );

    exec!(executor -> TRIGGER consolidate);

    {
        let response =
            exec!(executor -> QUERY "articles" "default" "personal data identifiab" LIMIT(3));
        assert_eq!(response, ["article:1", "article:3", "article:4"]);
    }

    {
        let response =
            exec!(executor -> QUERY "articles" "default" "personal data identifiab" LIMIT(1));
        assert_eq!(response, ["article:1"]);
    }
}
