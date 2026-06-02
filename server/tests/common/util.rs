// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

// NOTE: Implementation cannot be time-based, even with nanosecond precision,
//   as tests are ran concurrently and such conflicts happen (very often).
//   When it does, one test cleaning up its temporary directory causes another
//   to fail. We don’t want that.
pub fn unique_hex() -> Result<String, std::io::Error> {
    use std::io::Read as _;

    let mut urandom = std::fs::File::open("/dev/urandom")?;
    let mut buf = [0u8; 4]; // 4 bytes = 8 hex chars
    urandom.read_exact(&mut buf)?;

    let hex = buf.iter().map(|b| format!("{:02x}", b)).collect();

    Ok(hex)
}

macro_rules! assert_contains {
    ($haystack:expr, $needle:expr) => {{
        use std::collections::HashSet;

        let haystack: HashSet<String> = HashSet::from_iter($haystack.into_iter());
        let needle: HashSet<String> = HashSet::from_iter($needle.into_iter().map(String::from));
        let missing: HashSet<&String> = HashSet::from_iter(needle.difference(&haystack));

        assert!(
            missing.is_empty(),
            "missing: {missing:?}, got: {haystack:?}"
        );
    }};
}
pub(crate) use assert_contains;
