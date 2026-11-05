// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

//! Feature: Snippet matching

mod common;

use sonic::lexer::snippet_matching::MatchRange;

use crate::common::*;

// NOTE: This is NOT legal advice. It is solely for example purposes.
const GDPR_SHORT_PARAGRAPHS: &[&str] = &[
    "Under the General Data Protection Regulation (GDPR), “personal data” is defined as any information relating to an identified or identifiable natural person. An identifiable person is one who can be directly or indirectly identified, in particular by reference to identifiers such as a name, identification number, location data, or an online identifier.",
    "The concept is intentionally broad. It covers obvious identifiers like names, email addresses, and national identification numbers, but also extends to data that can indirectly identify someone when combined with other information. This includes IP addresses, device identifiers, and certain behavioural or usage data when they can be linked back to an individual.",
    "Identifiability is assessed in context, meaning that data considered anonymous in one setting may become personal data in another if reasonable means exist to re-identify the person. The GDPR also considers whether identification requires disproportionate effort, taking into account available technology and cost.",
    "Personal data protection under GDPR applies only to living natural persons, not legal entities such as companies. The regulation also distinguishes between ordinary personal data and “special categories” of personal data, which include sensitive information like health data, biometric data, or data revealing racial or ethnic origin, and which are subject to stricter processing conditions.",
];
// Copyright: [General Data Protection Regulation - Wikipedia](https://en.wikipedia.org/wiki/General_Data_Protection_Regulation#Controller_and_processor), 2026-11-04.
// NOTE: This is NOT legal advice. It is solely for example purposes.
const WIKIPEDIA_GDPR_CONTROLLER_AND_PROCESSOR: &[&str] = &[
    "Data controllers must clearly disclose any data collection, declare the lawful basis and purpose for data processing, and state how long data is being retained and if it is being shared with any third parties or outside of the EEA. Firms have the obligation to protect data of employees and consumers to the degree where only the necessary data is extracted with minimum interference with data privacy from employees, consumers, or third parties. Firms should have internal controls and regulations for various departments such as audit, internal controls, and operations. Data subjects have the right to request a portable copy of the data collected by a controller in a common format, as well as the right to have their data erased under certain circumstances. Public authorities, and businesses whose core activities consist of regular or systematic processing of personal data, are required to employ a data protection officer (DPO), who is responsible for managing compliance with the GDPR. Data controllers must report data breaches to national supervisory authorities within 72 hours if they have an adverse effect on user privacy. In some cases, violators of the GDPR may be fined up to €20 million or up to 4% of the annual worldwide turnover of the preceding financial year in case of an enterprise, whichever is greater.",
    "To be able to demonstrate compliance with the GDPR, the data controller must implement measures that meet the principles of data protection by design and by default. Article 25 requires data protection measures to be designed into the development of business processes for products and services. Such measures include pseudonymising personal data, by the controller, as soon as possible (Recital 78). It is the responsibility and the liability of the data controller to implement effective measures and be able to demonstrate the compliance of processing activities even if the processing is carried out by a data processor on behalf of the controller (Recital 74). When data is collected, data subjects must be clearly informed about the extent of data collection, the legal basis for the processing of personal data, how long data is retained, if data is being transferred to a third-party and/or outside the EU, and any automated decision-making that is made on a solely algorithmic basis. Data subjects must be informed of their privacy rights under the GDPR, including their right to revoke consent to data processing at any time, their right to view their personal data and access an overview of how it is being processed, their right to obtain a portable copy of the stored data, their right to erasure of their data under certain circumstances, their right to contest any automated decision-making that was made on a solely algorithmic basis, and their right to file complaints with a Data Protection Authority. As such, the data subject must also be provided with contact details for the data controller and their designated data protection officer, where applicable.",
    "Data protection impact assessments (Article 35) have to be conducted when specific risks occur to the rights and freedoms of data subjects. Risk assessment and mitigation is required and prior approval of the data protection authorities is required for high risks.",
];

/// Tests that Sonic supports returning snippets in addition to object
/// identifiers, and that the default config returns 1 snippet per object.
#[test]
fn test_snippet_matching_basic() {
    let collection = "test_snippet_matching_basic";

    init_logging();
    let executor = make_test_executor(|_| {});

    for (idx, paragraph) in GDPR_SHORT_PARAGRAPHS.iter().enumerate() {
        let oid = &format!("article:{idx}");
        exec!(executor -> PUSH collection "default" oid paragraph);
    }

    exec!(executor -> TRIGGER consolidate);

    let response =
        exec!(executor -> WITH SNIPPETS QUERY collection "default" "Personal data protection");
    let mut results = (response.into_iter()).map(|item| {
        (
            item.oid,
            (item.snippets.into_iter())
                .map(|snippet| (snippet.range, snippet.text))
                .collect::<Vec<_>>(),
        )
    });
    assert_eq!(results.next(), Some((
        "article:3".to_owned(),
        vec![(
            MatchRange { start: 0, end: 303 },
            "Personal data protection under GDPR applies only to living natural persons, not legal entities such as companies. The regulation also distinguishes between ordinary personal data and “special categories” of personal data, which include sensitive information like health data, biometric data, or data".to_owned(),
        )]
    )));
    assert_eq!(results.next(), Some((
        "article:0".to_owned(),
        vec![(
            MatchRange { start: 0, end: 286 },
            "Under the General Data Protection Regulation (GDPR), “personal data” is defined as any information relating to an identified or identifiable natural person. An identifiable person is one who can be directly or indirectly identified, in particular by reference to identifiers such as".to_owned(),
        )]
    )));
    assert_eq!(results.next(), Some((
        "article:2".to_owned(),
        vec![(
            MatchRange { start: 0, end: 304 },
            "Identifiability is assessed in context, meaning that data considered anonymous in one setting may become personal data in another if reasonable means exist to re-identify the person. The GDPR also considers whether identification requires disproportionate effort, taking into account available technology".to_owned(),
        )]
    )));
    assert_eq!(results.next(), Some((
        "article:1".to_owned(),
        vec![(
            MatchRange { start: 78, end: 363 },
            "email addresses, and national identification numbers, but also extends to data that can indirectly identify someone when combined with other information. This includes IP addresses, device identifiers, and certain behavioural or usage data when they can be linked back to an individual".to_owned(),
        )]
    )));
    assert_eq!(results.next(), None);
}

/// Tests that Sonic supports choosing the number of returned snippets.
#[test]
fn test_snippet_matching_max_snippet_count() {
    let collection = "test_snippet_matching_max_snippet_count";

    init_logging();
    let executor = make_test_executor(|_| {});

    for (idx, paragraph) in WIKIPEDIA_GDPR_CONTROLLER_AND_PROCESSOR.iter().enumerate() {
        let oid = &format!("article:{idx}");
        exec!(executor -> PUSH collection "default" oid paragraph);
    }

    exec!(executor -> TRIGGER consolidate);

    let response = exec!(executor -> WITH SNIPPETS(3) QUERY collection "default" "GDPR" LIMIT(2));
    let mut results = (response.into_iter()).map(|item| {
        (
            item.oid,
            (item.snippets.into_iter())
                .map(|snippet| (snippet.range, snippet.text))
                .collect::<Vec<_>>(),
        )
    });
    #[rustfmt::skip]
    assert_eq!(results.next(), Some((
        "article:1".to_owned(),
        vec![
            (
                MatchRange { start: 0, end: 268 },
                "To be able to demonstrate compliance with the GDPR, the data controller must implement measures that meet the principles of data protection by design and by default. Article 25 requires data protection measures to be designed into the development of business processes".to_owned(),
            ),
            (
                MatchRange { start: 933, end: 1179 },
                "decision-making that is made on a solely algorithmic basis. Data subjects must be informed of their privacy rights under the GDPR, including their right to revoke consent to data processing at any time, their right to view their personal data and".to_owned(),
            )
        ]
    )));
    assert_eq!(results.next(), Some((
        "article:0".to_owned(),
        vec![(
            MatchRange { start: 945, end: 1200 },
            // NOTE: “may be fined up to €20” is a funny example of truncation happening in a suboptimal place ^^'
            "responsible for managing compliance with the GDPR. Data controllers must report data breaches to national supervisory authorities within 72 hours if they have an adverse effect on user privacy. In some cases, violators of the GDPR may be fined up to €20".to_owned(),
        )]
    )));
    assert_eq!(results.next(), None);
}

/// Tests that Sonic supports choosing the maximum size of returned snippets.
#[test]
fn test_snippet_matching_max_window_size() {
    let collection = "test_snippet_matching_max_window_size";

    init_logging();
    let executor = make_test_executor(|_| {});

    for (idx, paragraph) in WIKIPEDIA_GDPR_CONTROLLER_AND_PROCESSOR.iter().enumerate() {
        let oid = &format!("article:{idx}");
        exec!(executor -> PUSH collection "default" oid paragraph);
    }

    exec!(executor -> TRIGGER consolidate);

    let response =
        exec!(executor -> WITH SNIPPETS(3, 4) QUERY collection "default" "GDPR" LIMIT(2));
    let mut results = (response.into_iter()).map(|item| {
        (
            item.oid,
            (item.snippets.into_iter())
                .map(|snippet| (snippet.range, snippet.text))
                .collect::<Vec<_>>(),
        )
    });
    #[rustfmt::skip]
    assert_eq!(results.next(), Some((
        "article:1".to_owned(),
        vec![
            (MatchRange { start:   37, end:   55 }, "with the GDPR, the".to_owned()),
            (MatchRange { start: 1048, end: 1073 }, "under the GDPR, including".to_owned()),
        ]
    )));
    #[rustfmt::skip]
    assert_eq!(results.next(), Some((
        "article:0".to_owned(),
        vec![
            (MatchRange { start:  981, end: 1000 }, "with the GDPR. Data".to_owned()),
            (MatchRange { start: 1164, end: 1179 }, "of the GDPR may".to_owned()),
        ]
    )));
    assert_eq!(results.next(), None);
}

/// Tests that Sonic supports choosing the maximum padding of returned snippets.
///
/// (Padding is the number of non-matching tokens around matches at the edge of
/// a window.)
#[test]
fn test_snippet_matching_max_window_padding() {
    let collection = "test_snippet_matching_max_window_padding";

    init_logging();
    let executor = make_test_executor(|_| {});

    for (idx, paragraph) in WIKIPEDIA_GDPR_CONTROLLER_AND_PROCESSOR.iter().enumerate() {
        let oid = &format!("article:{idx}");
        exec!(executor -> PUSH collection "default" oid paragraph);
    }

    exec!(executor -> TRIGGER consolidate);

    let response =
        exec!(executor -> WITH SNIPPETS(3, 4, 2) QUERY collection "default" "GDPR" LIMIT(2));
    let mut results = (response.into_iter()).map(|item| {
        (
            item.oid,
            (item.snippets.into_iter())
                .map(|snippet| (snippet.range, snippet.text))
                .collect::<Vec<_>>(),
        )
    });
    #[rustfmt::skip]
    assert_eq!(results.next(), Some((
        "article:1".to_owned(),
        vec![
            (MatchRange { start:   42, end:   55 }, "the GDPR, the".to_owned()),
            (MatchRange { start: 1054, end: 1073 }, "the GDPR, including".to_owned()),
        ]
    )));
    #[rustfmt::skip]
    assert_eq!(results.next(), Some((
        "article:0".to_owned(),
        vec![
            (MatchRange { start:  986, end: 1000 }, "the GDPR. Data".to_owned()),
            (MatchRange { start: 1167, end: 1179 }, "the GDPR may".to_owned()),
        ]
    )));
    assert_eq!(results.next(), None);
}

/// Tests that Sonic supports returning fragment IDs along with snippets.
#[test]
fn test_snippet_matching_fragments() {
    let collection = "test_snippet_matching_fragments";

    init_logging();
    let executor = make_test_executor(|_| {});

    for (idx, paragraph) in WIKIPEDIA_GDPR_CONTROLLER_AND_PROCESSOR.iter().enumerate() {
        let fid = &format!("paragraph:{idx}");
        exec!(executor -> PUSH collection "default" "article:0" fid paragraph);
    }

    exec!(executor -> TRIGGER consolidate);

    let response =
        exec!(executor -> WITH SNIPPETS(3, 3) QUERY collection "default" "GDPR" LIMIT(2));
    let mut results = (response.into_iter()).map(|item| {
        (item.snippets.into_iter())
            .map(|snippet| (snippet.fragment, snippet.range, snippet.text))
            .collect::<Vec<_>>()
    });
    #[rustfmt::skip]
    assert_eq!(results.next(), Some(vec![
        (Some("paragraph:1".to_owned()), MatchRange { start:   42, end:   55 }, "the GDPR, the".to_owned()),
        (Some("paragraph:0".to_owned()), MatchRange { start:  986, end: 1000 }, "the GDPR. Data".to_owned()),
    ]));
    assert_eq!(results.next(), None);
}

/// Tests that Sonic supports returning fragment IDs in their ingestion order.
///
/// While the default behavior makes sense for a chat-like system, it’s not
/// exactly what users would expect in a knowledge base / wiki / blog, etc.,
/// where earlier sections are expected to be returned first.
///
/// Note that this is only a tie-breaker when snippets have the same score.
/// Better snippets are always returned first.
#[test]
fn test_snippet_matching_fragments_reverse() {
    let collection = "test_snippet_matching_fragments_reverse";

    init_logging();
    let executor = make_test_executor(|_| {});

    for (idx, paragraph) in WIKIPEDIA_GDPR_CONTROLLER_AND_PROCESSOR.iter().enumerate() {
        let fid = &format!("paragraph:{idx}");
        exec!(executor -> PUSH collection "default" "article:0" fid paragraph);
    }

    exec!(executor -> TRIGGER consolidate);

    let response =
        exec!(executor -> WITH SNIPPETS_REV(3, 3) QUERY collection "default" "GDPR" LIMIT(2));
    let mut results = (response.into_iter()).map(|item| {
        (item.snippets.into_iter())
            .map(|snippet| (snippet.fragment, snippet.range, snippet.text))
            .collect::<Vec<_>>()
    });
    #[rustfmt::skip]
    assert_eq!(results.next(), Some(vec![
        (Some("paragraph:0".to_owned()), MatchRange { start:  986, end: 1000 }, "the GDPR. Data".to_owned()),
        (Some("paragraph:1".to_owned()), MatchRange { start:   42, end:   55 }, "the GDPR, the".to_owned()),
    ]));
    assert_eq!(results.next(), None);
}
