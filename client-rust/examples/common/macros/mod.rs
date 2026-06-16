// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

pub(crate) mod logging;

#[allow(unused_macros)]
macro_rules! timed {
    ($code:block) => {{
        let start = std::time::Instant::now();
        let res = $code;
        eprintln!("Took {:.3?}", start.elapsed());
        res
    }};
}
pub(crate) use timed;
