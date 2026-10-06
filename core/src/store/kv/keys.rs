// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::store::generic::KEY_SEPARATOR;
use crate::store::types::*;

use super::encoding::*;

use KvStoreKeyDiscriminator as D;

// WARN: Don’t change values here, it would break the index! Only add new cases.
impl_int_enum!(pub(super) KvStoreKeyDiscriminator(u8):
    MetaToValue (META_TO_VALUE) = 0,
    TermToIids (TERM_TO_IIDS) = 1,
    OidToIid (OID_TO_IID) = 2,
    IidToOid (IID_TO_OID) = 3,
    IidToTerms (IID_TO_TERMS) = 4,
);

// WARN: Don’t change values here, it would break the index! Only add new cases.
impl_int_enum!(pub StoreMetaKey(u32):
    IIDIncr (IID_INCR) = 0,
    ObjectCount (OBJECT_COUNT) = 1,
);

#[derive(Clone, PartialEq, Eq)]
#[repr(transparent)]
pub(super) struct KvStoreKey(Vec<u8>);

impl KvStoreKey {
    #[inline]
    const fn new(value: Vec<u8>) -> Self {
        debug_assert!(value.len() > 6);
        Self(value)
    }
}

pub(super) struct KvStoreKeyParts<'a> {
    pub(super) bucket: &'a [u8],
    pub(super) discriminator: u8,
    pub(super) route: &'a [u8],
}

impl<'a> TryFrom<&'a [u8]> for KvStoreKeyParts<'a> {
    type Error = &'static str;

    fn try_from(bytes: &'a [u8]) -> Result<Self, Self::Error> {
        if bytes.is_empty() {
            return Err("key empty");
        }

        // TODO: Move this logic near `Bucket` key encoding.
        let Some(prefix_len) = bytes.iter().position(|c| *c == KEY_SEPARATOR) else {
            return Err("missing separator after bucket part");
        };

        if bytes.len() < prefix_len + 2 {
            return Err("key missing route");
        };

        Ok(Self {
            bucket: &bytes[..prefix_len],
            discriminator: bytes[prefix_len + 1],
            route: &bytes[(prefix_len + 2)..],
        })
    }
}

impl KvStoreKey {
    pub(super) fn meta_to_value(bucket: &Bucket, meta: StoreMetaKey) -> KvStoreKey {
        Self::make(D::MetaToValue, bucket, meta)
    }

    pub(super) fn term_to_iids(bucket: &Bucket, term_hash: &StoreTermHash) -> KvStoreKey {
        Self::make(D::TermToIids, bucket, term_hash)
    }

    pub(super) fn oid_to_iid(bucket: &Bucket, oid: StoreObjectOid) -> KvStoreKey {
        Self::make(D::OidToIid, bucket, &oid)
    }

    pub(super) fn iid_to_oid(bucket: &Bucket, iid: StoreObjectIid) -> KvStoreKey {
        Self::make(D::IidToOid, bucket, iid)
    }

    pub(super) fn iid_to_terms(bucket: &Bucket, iid: StoreObjectIid) -> KvStoreKey {
        Self::make(D::IidToTerms, bucket, iid)
    }

    /// Key format: `[bucket<?B> | discriminator<1B> | route<?B>]`
    fn make(
        idx: KvStoreKeyDiscriminator,
        bucket: impl ToKvKeyPart,
        route: impl ToKvKeyPart,
    ) -> KvStoreKey {
        let bucket_repr = bucket.to_kv_key_part();
        let bucket_bytes = bucket_repr.as_ref();

        let route_repr = route.to_kv_key_part();
        let route_bytes = route_repr.as_ref();

        let mut key_bytes = Vec::with_capacity(bucket_bytes.len() + 1 + route_bytes.len());

        key_bytes.extend_from_slice(bucket_bytes);
        key_bytes.push(idx as u8);
        key_bytes.extend_from_slice(route_bytes);

        KvStoreKey::new(key_bytes)
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

// MARK: Boilerplate

impl AsRef<[u8]> for KvStoreKey {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Display for KvStoreKey {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let Self(bytes) = self;

        let KvStoreKeyParts {
            bucket,
            discriminator,
            route,
        } = KvStoreKeyParts::try_from(bytes.as_slice()).unwrap();

        let bucket_str = str::from_utf8(bucket).unwrap();

        let discriminator = D::try_from(discriminator).unwrap();

        match discriminator {
            D::MetaToValue => {
                let meta = StoreMetaKey::try_from_kv_key_part(route).unwrap();

                write!(f, "{bucket_str:?}:{discriminator}:{meta}")
            }
            D::TermToIids => {
                let term_hash = StoreTermHash::try_from_kv_key_part(route).unwrap();

                write!(f, "{bucket_str:?}:{discriminator}:{term_hash}")
            }
            D::OidToIid => {
                let route_str = str::from_utf8(route).unwrap();

                write!(f, "{bucket_str:?}:{discriminator}:{route_str:?}")
            }
            D::IidToOid | D::IidToTerms => {
                let iid = StoreObjectIid::from_kv_key_part(*route.first_chunk().unwrap());

                write!(f, "{bucket_str:?}:{discriminator}:{iid}")
            }
        }
    }
}

impl std::fmt::Debug for KvStoreKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let bytes = &self.0;
        write!(f, "'{self}' {bytes:?}")
    }
}

macro_rules! impl_int_enum {
    (
        $vis:vis $t:ident($repr:ty):
        $($case:ident ($const:ident) = $value:expr),+ $(,)?
    ) => {
        $(const $const: $repr = $value;)+

        #[repr($repr)]
        $vis enum $t {
            $($case = $const,)+
        }

        impl std::fmt::Display for $t {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    $(Self::$case => f.write_str(stringify!($const)),)+
                }
            }
        }

        impl TryFrom<$repr> for $t {
            type Error = String;

            fn try_from(value: $repr) -> Result<Self, Self::Error> {
                match value {
                    $($const => Ok(Self::$case),)+
                    n => Err(format!("Invalid `{ty}`: {n}", ty = std::any::type_name::<$t>())),
                }
            }
        }
    };
}
use impl_int_enum;

// MARK: - Tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_keys_meta_to_value() {
        assert_eq!(
            KvStoreKey::meta_to_value(&"b:1".into(), StoreMetaKey::IIDIncr).0,
            [b'b', b':', b'1', KEY_SEPARATOR, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn it_keys_term_to_iids() {
        assert_eq!(
            KvStoreKey::term_to_iids(&"b:2".into(), &772137347.into()).0,
            [b'b', b':', b'2', KEY_SEPARATOR, 1, 131, 225, 5, 46]
        );
        assert_eq!(
            KvStoreKey::term_to_iids(&"b:2".into(), &3582484684.into()).0,
            [b'b', b':', b'2', KEY_SEPARATOR, 1, 204, 96, 136, 213]
        );
    }

    #[test]
    fn it_keys_oid_to_iid() {
        #[rustfmt::skip]
        assert_eq!(
            KvStoreKey::oid_to_iid(&"b:3".into(), "conv:6501e83a".into()).0,
            [b'b', b':', b'3', KEY_SEPARATOR, 2, b'c', b'o', b'n', b'v', b':', b'6', b'5', b'0', b'1', b'e', b'8', b'3', b'a']
        );
    }

    #[test]
    fn it_keys_iid_to_oid() {
        #[rustfmt::skip]
        assert_eq!(
            KvStoreKey::iid_to_oid(&"b:4".into(), 10292198.into()).0,
            [b'b', b':', b'4', KEY_SEPARATOR, 3, 0, 0, 0, 0, 0, 157, 11, 230]
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
            [b'b', b':', b'5', KEY_SEPARATOR, 4, 0, 0, 0, 0, 0, 0, 0, 1]
        );
        assert_eq!(
            KvStoreKey::iid_to_terms(&"b:5".into(), 20.into()).0,
            [b'b', b':', b'5', KEY_SEPARATOR, 4, 0, 0, 0, 0, 0, 0, 0, 20]
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
                KvStoreKey::term_to_iids(&"b:6".into(), &72137347.into())
            ),
            r#""b:6":TERM_TO_IIDS:<44cba83>"#
        );
        assert_eq!(
            &format!(
                "{}",
                KvStoreKey::meta_to_value(&"b:6".into(), StoreMetaKey::IIDIncr)
            ),
            r#""b:6":META_TO_VALUE:IID_INCR"#
        );
    }

    #[test]
    fn it_computes_key_ranges() {
        let range = KvStoreKey::prefix_range(&Bucket::from("ABC"));
        assert_eq!(range.start, &[b'A', b'B', b'C', KEY_SEPARATOR]);
        assert_eq!(range.end, &[b'A', b'B', b'C', KEY_SEPARATOR + 1]);
    }
}

// MARK: - Benchmarks

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
