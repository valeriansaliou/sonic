// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::store::generic::KEY_SEPARATOR;
use crate::store::types::*;

use self::constants::*;

// WARN: Don’t update values here, it would break the index! Only add new cases.
pub(super) mod constants {
    pub(in crate::store::kv) const META_TO_VALUE: u8 = 0;
    pub(in crate::store::kv) const TERM_TO_IIDS: u8 = 1;
    pub(in crate::store::kv) const OID_TO_IID: u8 = 2;
    pub(in crate::store::kv) const IID_TO_OID: u8 = 3;
    pub(in crate::store::kv) const IID_TO_TERMS: u8 = 4;
}

#[derive(Clone, PartialEq, Eq)]
#[repr(transparent)]
pub(super) struct KvStoreKey(Vec<u8>);

impl KvStoreKey {
    pub(super) fn meta_to_value(bucket: &Bucket, meta: &StoreMetaKey) -> KvStoreKey {
        Self::make(META_TO_VALUE, bucket, meta.as_u32())
    }

    pub(super) fn term_to_iids(bucket: &Bucket, term_hash: StoreTermHash) -> KvStoreKey {
        Self::make(TERM_TO_IIDS, bucket, term_hash.into())
    }

    pub(super) fn oid_to_iid(bucket: &Bucket, oid: StoreObjectOid) -> KvStoreKey {
        Self::make(OID_TO_IID, bucket, oid.to_compact())
    }

    pub(super) fn iid_to_oid(bucket: &Bucket, iid: StoreObjectIid) -> KvStoreKey {
        Self::make(IID_TO_OID, bucket, iid.into())
    }

    pub(super) fn iid_to_terms(bucket: &Bucket, iid: StoreObjectIid) -> KvStoreKey {
        Self::make(IID_TO_TERMS, bucket, iid.into())
    }

    /// Key format: `[bucket<?B> | separator<1B> | idx<1B> | route<4B>]`
    fn make(idx: u8, bucket: &Bucket, route: u32) -> KvStoreKey {
        // Encode key bucket + key route from u32 to array of u8 (i.e. binary).
        let bucket_bytes = bucket.as_bytes();

        let mut key_bytes = Vec::with_capacity(bucket_bytes.len() + 6);

        key_bytes.extend_from_slice(bucket_bytes); // [bucket<?B>]
        key_bytes.push(KEY_SEPARATOR); // [separator<1B>]
        key_bytes.push(idx); // [idx<1B>]
        key_bytes.extend_from_slice(&route.to_be_bytes()); // [route<4B>]

        KvStoreKey::from(key_bytes)
    }

    pub(super) fn prefix_range(bucket: &Bucket) -> std::ops::Range<Vec<u8>> {
        let bucket_bytes = bucket.as_bytes();

        let mut start = Vec::with_capacity(bucket_bytes.len() + 1);
        start.extend_from_slice(bucket_bytes);
        start.push(KEY_SEPARATOR);

        let mut end = Vec::with_capacity(bucket_bytes.len() + 1);
        end.extend_from_slice(bucket_bytes);
        end.push(KEY_SEPARATOR + 1);

        start..end
    }
}

impl From<Vec<u8>> for KvStoreKey {
    fn from(value: Vec<u8>) -> Self {
        debug_assert!(value.len() > 6);
        Self(value)
    }
}

impl AsRef<[u8]> for KvStoreKey {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Display for KvStoreKey {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let Self(bytes) = self;

        // WARN: `splitn` is important as `KEY_SEPARATOR` might appear later as a normal byte.
        let mut splits = bytes.splitn(2, |c| *c == KEY_SEPARATOR);

        let bucket_bytes = splits.next().unwrap();
        let key_bucket = str::from_utf8(bucket_bytes).unwrap();

        let rest = splits.next().unwrap();
        debug_assert_eq!(rest.len(), 5);
        debug_assert!(splits.next().is_none());

        let key_idx = rest[0];

        let route_bytes = &rest[1..];
        let key_route = u32::from_be_bytes([
            route_bytes[0],
            route_bytes[1],
            route_bytes[2],
            route_bytes[3],
        ]);

        write!(f, "{key_bucket:?}:{key_idx}:{key_route:x}")
    }
}

impl std::fmt::Debug for KvStoreKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let bytes = &self.0;
        write!(f, "'{self}' {bytes:?}")
    }
}

pub enum StoreMetaKey {
    IIDIncr,
    ObjectCount,
}

impl StoreMetaKey {
    pub const fn as_u32(&self) -> u32 {
        // WARN: Don’t update values here, it would break the index! Only add new cases.
        match self {
            StoreMetaKey::IIDIncr => 0,
            StoreMetaKey::ObjectCount => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_keys_meta_to_value() {
        assert_eq!(
            KvStoreKey::meta_to_value(&"b:1".into(), &StoreMetaKey::IIDIncr).0,
            [b'b', b':', b'1', KEY_SEPARATOR, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn it_keys_term_to_iids() {
        assert_eq!(
            KvStoreKey::term_to_iids(&"b:2".into(), 772137347.into()).0,
            [b'b', b':', b'2', KEY_SEPARATOR, 1, 46, 5, 225, 131]
        );
        assert_eq!(
            KvStoreKey::term_to_iids(&"b:2".into(), 3582484684.into()).0,
            [b'b', b':', b'2', KEY_SEPARATOR, 1, 213, 136, 96, 204]
        );
    }

    #[test]
    fn it_keys_oid_to_iid() {
        assert_eq!(
            KvStoreKey::oid_to_iid(&"b:3".into(), "conversation:6501e83a".into()).0,
            [b'b', b':', b'3', KEY_SEPARATOR, 2, 213, 118, 156, 31]
        );
    }

    #[test]
    fn it_keys_iid_to_oid() {
        assert_eq!(
            KvStoreKey::iid_to_oid(&"b:4".into(), 10292198.into()).0,
            [b'b', b':', b'4', KEY_SEPARATOR, 3, 0, 157, 11, 230]
        );
    }

    #[test]
    fn iid_to_oid_is_lexicographically_sorted() {
        assert!(
            KvStoreKey::iid_to_oid(&"b:4".into(), 1.into()).0
                < KvStoreKey::iid_to_oid(&"b:4".into(), 2.into()).0
        );
    }

    #[test]
    fn it_keys_iid_to_terms() {
        assert_eq!(
            KvStoreKey::iid_to_terms(&"b:5".into(), 1.into()).0,
            [b'b', b':', b'5', KEY_SEPARATOR, 4, 0, 0, 0, 1]
        );
        assert_eq!(
            KvStoreKey::iid_to_terms(&"b:5".into(), 20.into()).0,
            [b'b', b':', b'5', KEY_SEPARATOR, 4, 0, 0, 0, 20]
        );
    }

    #[test]
    fn it_hashes_compact() {
        assert_eq!(StoreObjectOid::from("key:1").to_compact(), 3370353088);
        assert_eq!(StoreObjectOid::from("key:2").to_compact(), 1042559698);
    }

    #[test]
    fn it_formats_key() {
        assert_eq!(
            &format!(
                "{}",
                KvStoreKey::term_to_iids(&"b:6".into(), 72137347.into())
            ),
            r#""b:6":1:44cba83"#
        );
        assert_eq!(
            &format!(
                "{}",
                KvStoreKey::meta_to_value(&"b:6".into(), &StoreMetaKey::IIDIncr)
            ),
            r#""b:6":0:0"#
        );
    }

    #[test]
    fn it_computes_key_ranges() {
        let range = KvStoreKey::prefix_range(&Bucket::from("ABC"));
        assert_eq!(range.start, &[b'A', b'B', b'C', KEY_SEPARATOR]);
        assert_eq!(range.end, &[b'A', b'B', b'C', KEY_SEPARATOR + 1]);
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
        b.iter(|| KvStoreKey::meta_to_value("bucket:bench:1", &StoreMetaKey::IIDIncr));
    }

    #[bench]
    fn bench_key_term_to_iids(b: &mut Bencher) {
        b.iter(|| KvStoreKey::term_to_iids("bucket:bench:2", 772137347));
    }

    #[bench]
    fn bench_key_oid_to_iid(b: &mut Bencher) {
        let key = "conversation:6501e83a".to_string();

        b.iter(|| KvStoreKey::oid_to_iid("bucket:bench:3", &key));
    }

    #[bench]
    fn bench_key_iid_to_oid(b: &mut Bencher) {
        b.iter(|| KvStoreKey::iid_to_oid("bucket:bench:4", 10292198));
    }

    #[bench]
    fn bench_key_iid_to_terms(b: &mut Bencher) {
        b.iter(|| KvStoreKey::iid_to_terms("bucket:bench:5", 1));
    }
}
