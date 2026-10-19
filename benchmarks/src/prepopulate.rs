// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::RunContext;

pub fn no_prepopulate(_ctx: &RunContext) {}

pub fn prepopulate_gdpr(ctx: &RunContext) {
    use sonic_client::SonicMultiplexer;
    use sonic_client::control::SonicChannelControlBlocking;
    use sonic_client::ingest::SonicChannelIngestBlocking;
    use sonic_client::options::Lang;

    let multiplexer = SonicMultiplexer::new().unwrap();

    // NOTE: This is NOT legal advice. It is solely for example purposes.
    let articles = [
        "The GDPR applies to any organization—regardless of where it is located—\
        that processes the personal data of people in the European Union or \
        European Economic Area in connection with offering goods or services \
        to them or monitoring their behavior.",
        "GDPR compliance means implementing the technical, organizational, \
        and legal measures required by the GDPR to protect personal data \
        and uphold individuals’ privacy rights.",
        "The European Union establishes regulations and directives that create \
        common legal standards across its member states in areas such as \
        privacy, competition, consumer protection, and digital markets.",
        "Brussels is a major center for technology policy and digital \
        regulation, shaping rules that influence companies and software \
        services worldwide.",
    ];

    let sonic =
        SonicChannelIngestBlocking::connect(ctx.addr, "SecretString", &multiplexer).unwrap();
    for (i, text) in articles.into_iter().enumerate() {
        sonic
            .push_with_options(
                "articles",
                "default",
                format!("article:{}", i + 1),
                text,
                &[&Lang("eng")],
            )
            .unwrap();
    }
    drop(sonic);

    let sonic =
        SonicChannelControlBlocking::connect(ctx.addr, "SecretString", &multiplexer).unwrap();

    sonic.trigger_consolidate().unwrap();

    // NOTE: Do not flush even when Sonic supports `TRIGGER flush`, as
    //   it would cause differences between old and new versions of
    //   Sonic.

    drop(sonic);
}
