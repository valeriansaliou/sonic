// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

// TODO(major): Change index structure so bucket comes first.

use crate::store::*;

use self::constants::*;

// WARN: Don’t update values here, it would break the index! Only add new cases.
pub(super) mod constants {
    pub(in crate::store::kv) const META_TO_VALUE: u8 = 0;
    pub(in crate::store::kv) const TERM_TO_IIDS: u8 = 1;
    pub(in crate::store::kv) const OID_TO_IID: u8 = 2;
    pub(in crate::store::kv) const IID_TO_OID: u8 = 3;
    pub(in crate::store::kv) const IID_TO_TERMS: u8 = 4;
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub(super) struct StoreKVKey([u8; 9]);

impl StoreKVKey {
    pub(super) fn meta_to_value(bucket: &StoreItemPart, meta: &StoreMetaKey) -> StoreKVKey {
        Self::make(META_TO_VALUE, bucket, meta.as_u32())
    }

    pub(super) fn term_to_iids(bucket: &StoreItemPart, term_hash: StoreTermHash) -> StoreKVKey {
        Self::make(TERM_TO_IIDS, bucket, term_hash.into())
    }

    pub(super) fn oid_to_iid(bucket: &StoreItemPart, oid: StoreObjectOID) -> StoreKVKey {
        Self::make(OID_TO_IID, bucket, oid.into_compact())
    }

    pub(super) fn iid_to_oid(bucket: &StoreItemPart, iid: StoreObjectIID) -> StoreKVKey {
        Self::make(IID_TO_OID, bucket, iid.into())
    }

    pub(super) fn iid_to_terms(bucket: &StoreItemPart, iid: StoreObjectIID) -> StoreKVKey {
        Self::make(IID_TO_TERMS, bucket, iid.into())
    }

    /// Key format: `[idx<1B> | bucket<4B> | route<4B>]`
    fn make(idx: u8, bucket: &StoreItemPart, route: u32) -> StoreKVKey {
        // Encode key bucket + key route from u32 to array of u8 (i.e. binary).
        let [b0, b1, b2, b3] = bucket.into_compact().to_le_bytes();
        let [r0, r1, r2, r3] = route.to_le_bytes();

        // Generate final binary key.
        StoreKVKey::from([
            idx, // [idx<1B>]
            b0, b1, b2, b3, // [bucket<4B>]
            r0, r1, r2, r3, // [route<4B>]
        ])
    }

    /// Prefix format: `[idx<1B> | bucket<4B>]`
    pub(super) fn into_prefix(self) -> [u8; 5] {
        [self.0[0], self.0[1], self.0[2], self.0[3], self.0[4]]
    }
}

impl From<[u8; 9]> for StoreKVKey {
    fn from(value: [u8; 9]) -> Self {
        Self(value)
    }
}

impl AsRef<[u8]> for StoreKVKey {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Display for StoreKVKey {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let Self(bytes @ [key_idx, b1, b2, b3, b4, b5, b6, b7, b8]) = self;

        // Convert to number
        let key_bucket = u32::from_le_bytes([*b1, *b2, *b3, *b4]);
        let key_route = u32::from_le_bytes([*b5, *b6, *b7, *b8]);

        write!(f, "'{key_idx}:{key_bucket:x}:{key_route:x}' {bytes:?}")
    }
}

impl std::fmt::Debug for StoreKVKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.0, f)
    }
}

pub enum StoreMetaKey {
    IIDIncr,
}

impl StoreMetaKey {
    pub const fn as_u32(&self) -> u32 {
        // WARN: Don’t update values here, it would break the index! Only add new cases.
        match self {
            StoreMetaKey::IIDIncr => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_keys_meta_to_value() {
        assert_eq!(
            StoreKVKey::meta_to_value(&"bucket:1".into(), &StoreMetaKey::IIDIncr).0,
            [0, 108, 244, 29, 93, 0, 0, 0, 0]
        );
    }

    #[test]
    fn it_keys_term_to_iids() {
        assert_eq!(
            StoreKVKey::term_to_iids(&"bucket:2".into(), 772137347.into()).0,
            [1, 50, 220, 166, 65, 131, 225, 5, 46]
        );
        assert_eq!(
            StoreKVKey::term_to_iids(&"bucket:2".into(), 3582484684.into()).0,
            [1, 50, 220, 166, 65, 204, 96, 136, 213]
        );
    }

    #[test]
    fn it_keys_oid_to_iid() {
        assert_eq!(
            StoreKVKey::oid_to_iid(&"bucket:3".into(), "conversation:6501e83a".into()).0,
            [2, 171, 194, 213, 57, 31, 156, 118, 213]
        );
    }

    #[test]
    fn it_keys_iid_to_oid() {
        assert_eq!(
            StoreKVKey::iid_to_oid(&"bucket:4".into(), 10292198.into()).0,
            [3, 105, 12, 54, 147, 230, 11, 157, 0]
        );
    }

    #[test]
    fn it_keys_iid_to_terms() {
        assert_eq!(
            StoreKVKey::iid_to_terms(&"bucket:5".into(), 1.into()).0,
            [4, 137, 142, 73, 67, 1, 0, 0, 0]
        );
        assert_eq!(
            StoreKVKey::iid_to_terms(&"bucket:5".into(), 20.into()).0,
            [4, 137, 142, 73, 67, 20, 0, 0, 0]
        );
    }

    #[test]
    fn it_hashes_compact() {
        assert_eq!(StoreObjectOID::from("key:1").into_compact(), 3370353088);
        assert_eq!(StoreObjectOID::from("key:2").into_compact(), 1042559698);
    }

    #[test]
    fn it_formats_key() {
        assert_eq!(
            &format!(
                "{}",
                StoreKVKey::term_to_iids(&"bucket:6".into(), 72137347.into())
            ),
            "'1:71198b49:44cba83' [1, 73, 139, 25, 113, 131, 186, 76, 4]"
        );
        assert_eq!(
            &format!(
                "{}",
                StoreKVKey::meta_to_value(&"bucket:6".into(), &StoreMetaKey::IIDIncr)
            ),
            "'0:71198b49:0' [0, 73, 139, 25, 113, 0, 0, 0, 0]"
        );
    }
}

#[cfg(all(feature = "benchmark", test))]
mod benches {
    extern crate test;

    use super::*;
    use test::Bencher;

    #[bench]
    fn bench_hash_compact_short(b: &mut Bencher) {
        b.iter(|| StoreKeyerHasher::to_compact("key:bench:1"));
    }

    #[bench]
    fn bench_hash_compact_long(b: &mut Bencher) {
        b.iter(|| {
            StoreKeyerHasher::to_compact(
                "key:bench:2:long:long:long:long:long:long:long:long:long:long:long:long:long:long",
            )
        });
    }

    #[bench]
    fn bench_key_meta_to_value(b: &mut Bencher) {
        b.iter(|| StoreKVKey::meta_to_value("bucket:bench:1", &StoreMetaKey::IIDIncr));
    }

    #[bench]
    fn bench_key_term_to_iids(b: &mut Bencher) {
        b.iter(|| StoreKVKey::term_to_iids("bucket:bench:2", 772137347));
    }

    #[bench]
    fn bench_key_oid_to_iid(b: &mut Bencher) {
        let key = "conversation:6501e83a".to_string();

        b.iter(|| StoreKVKey::oid_to_iid("bucket:bench:3", &key));
    }

    #[bench]
    fn bench_key_iid_to_oid(b: &mut Bencher) {
        b.iter(|| StoreKVKey::iid_to_oid("bucket:bench:4", 10292198));
    }

    #[bench]
    fn bench_key_iid_to_terms(b: &mut Bencher) {
        b.iter(|| StoreKVKey::iid_to_terms("bucket:bench:5", 1));
    }
}
