// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

pub(crate) fn bm25_lite_idf(document_count: u64, document_frequency: u64) -> f32 {
    debug_assert!(
        document_frequency <= document_count,
        "{document_frequency} > {document_count}"
    );

    let n = document_count.max(document_frequency) as f32;
    let df = document_frequency as f32;

    (1.0 + (n - df + 0.5) / (df + 0.5)).ln()
}

pub(crate) fn bm25_lite(idf: f64, term_frequency: u8) -> f32 {
    if term_frequency == 0 {
        return 0.0;
    }

    let tf = term_frequency as f64;

    // Saturating TF component, with k1 = 1.2.
    let tf_weight = tf * (1.2 + 1.0) / (tf + 1.2);

    (idf * tf_weight) as f32
}
