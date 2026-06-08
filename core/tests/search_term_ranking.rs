// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

//! Feature: Search scoping

mod common;

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
    let executor = make_test_executor();

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
