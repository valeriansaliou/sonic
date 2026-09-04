// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

#[derive(Default)]
pub(crate) struct NoopU32HasherBuilder;

impl std::hash::BuildHasher for NoopU32HasherBuilder {
    type Hasher = NoopU32Hasher;

    fn build_hasher(&self) -> Self::Hasher {
        NoopU32Hasher { res: 0 }
    }
}

/// A no-op hasher that yields `u32` as is (avoids re-hashing hashed terms when
/// using them in `HashSet`s).
pub(crate) struct NoopU32Hasher {
    res: u32,
}

impl std::hash::Hasher for NoopU32Hasher {
    fn finish(&self) -> u64 {
        self.res as u64
    }

    fn write(&mut self, _bytes: &[u8]) {
        unreachable!()
    }

    fn write_u32(&mut self, i: u32) {
        assert_eq!(self.res, 0);
        self.res = i;
    }
}
