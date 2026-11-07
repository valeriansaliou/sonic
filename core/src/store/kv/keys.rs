// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::store::types::*;
use crate::util::impl_int_enum;

use super::encoding::*;
use super::{IidToTermsEntry, TermToIidsEntry};

use KvStoreKeyDiscriminator as D;

macro_rules! declare_store_keys {
    (
        $vis:vis $t:ident($repr:ty):
        $(
            [$const:ident = $value:expr]
            $case:ident<$lt:lifetime$(, $assoc_ty:ident: $assoc_ty_trait:ident)?> {
                $($field:ident: $(&$field_ref_lt:lifetime)? $field_type:ident$(<$field_lt:lifetime>)?),+ $(,)?
            },
            Value = $value_type:path,
            route($($route_display:expr),+) = $route:block,
        )+
    ) => {
        impl_int_enum!($vis $t($repr):
            $($case ($const) = $value,)+
        );

        $(
            #[doc = concat!("`", stringify!($const), "` (`", stringify!($value), "`)")]
            #[derive(Clone, Copy)]
            pub struct $case<$lt$(, $assoc_ty)?> {
                pub bucket: Bucket<$lt>,
                $(pub $field: $(&$field_ref_lt)? $field_type$(<$field_lt>)?,)+
            }

            impl<$lt$(, $assoc_ty)?> TypedKvStoreKey for $case<$lt$(, $assoc_ty)?>
            $(where $assoc_ty: $assoc_ty_trait)?
            {
                type Value<'db> = $value_type;

                #[inline]
                fn disciminator() -> KvStoreKeyDiscriminator {
                    D::$case
                }

                #[inline]
                fn to_bytes(&self) -> Vec<u8> {
                    #[allow(unused_variables)]
                    let Self { bucket, $($field),+ } = self;
                    let route = $route;

                    KvStoreKey::make(Self::disciminator(), bucket, route.as_ref()).0
                }
            }

            impl<$lt$(, $assoc_ty)?> std::fmt::Display for $case<'a$(, $assoc_ty)?>
            $(where $assoc_ty: $assoc_ty_trait)?
            {
                #[inline]
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    #[allow(unused_variables)]
                    let Self { bucket, $($field),+ } = self;
                    let discriminator = Self::disciminator();

                    write!(f, "{bucket:?}:{discriminator}:")?;
                    write!(f, $($route_display),+)
                }
            }

            impl<$lt$(, $assoc_ty)?> std::fmt::Debug for $case<'a$(, $assoc_ty)?>
            $(where $assoc_ty: $assoc_ty_trait)?
            {
                #[inline]
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    let bytes = self.to_bytes();
                    write!(f, "'{self}' {bytes:?}")
                }
            }
        )+
    };
}

// WARN: Don’t change values here, it would break the index! Only add new cases.
declare_store_keys!(pub(super) KvStoreKeyDiscriminator(u8):
    [META_TO_VALUE = 0]
    BucketMetaToValue<'a, SubKey: TypedBucketMetaKey> { meta: SubKey },
    Value = SubKey::Value<'db>,
    route("{}", SubKey::disciminator()) = { SubKey::disciminator().to_kv_key_part() },

    [TERM_TO_IIDS = 1]
    TermToIids<'a> { term_hash: &'a StoreTermHash },
    Value = TermToIidsIter<'db>,
    route("{term_hash}") = { term_hash.to_kv_key_part() },

    [OID_TO_IID = 2]
    OidToIid<'a> { oid: GenericOid<'a> },
    Value = GenericIid,
    route("{oid:?}") = { oid.to_kv_key_part() },

    [IID_TO_OID = 3]
    IidToOid<'a> { iid: GenericIid },
    Value = GenericOidOwned,
    route("{iid}") = { iid.to_kv_key_part() },

    [IID_TO_TERMS = 4]
    IidToTerms<'a> { iid: GenericIid },
    Value = IidToTermsIter<'a>,
    route("{iid}") = { iid.to_kv_key_part() },

    [OBJECT_META_TO_VALUE = 5]
    ObjectMetaToValue<'a, SubKey: TypedObjectMetaKey> { object_iid: GenericIid, meta: SubKey },
    Value = SubKey::Value<'db>,
    route("{object_iid}:{}", SubKey::disciminator()) = {
        let capacity = 9;
        let mut res = Vec::with_capacity(capacity);

        res.extend_from_slice(&object_iid.to_kv_key_part());
        res.extend_from_slice(&SubKey::disciminator().to_kv_key_part());

        debug_assert_eq!(res.len(), capacity);

        res
    },

    [TERM_TO_FRAGMENT_IIDS = 6]
    TermToFragmentIids<'a> { object_iid: GenericIid, term_hash: &'a StoreTermHash },
    Value = TermToIidsIter<'db>,
    route("{object_iid}:{term_hash}") = {
        let capacity = 12;
        let mut res = Vec::with_capacity(capacity);

        res.extend_from_slice(&object_iid.to_kv_key_part());
        res.extend_from_slice(&term_hash.to_kv_key_part());

        debug_assert_eq!(res.len(), capacity);

        res
    },

    [FRAGMENT_OID_TO_IID = 7]
    FragmentOidToIid<'a> { object_oid: GenericOid<'a>, fragment_oid: GenericOid<'a> },
    Value = GenericIid,
    route("{object_oid:?}:{fragment_oid:?}") = {
        let capacity = object_oid.len() + fragment_oid.len() + 2;
        let mut res = Vec::with_capacity(capacity);

        res.extend_from_slice(&object_oid.to_kv_key_part());
        res.extend_from_slice(&fragment_oid.to_kv_key_part());

        debug_assert_eq!(res.len(), capacity);

        res
    },

    [FRAGMENT_IID_TO_OID = 8]
    FragmentIidToOid<'a> { object_iid: GenericIid, fragment_iid: GenericIid },
    Value = GenericOidOwned,
    route("{object_iid}:{fragment_iid}") = {
        let capacity = 16;
        let mut res = Vec::with_capacity(capacity);

        res.extend_from_slice(&object_iid.to_kv_key_part());
        res.extend_from_slice(&fragment_iid.to_kv_key_part());

        debug_assert_eq!(res.len(), capacity);

        res
    },
);

macro_rules! declare_meta_keys {
    (
        $vis:vis $t:ident($repr:ty) / $trait:ty:
        $($case:ident ($const:ident) -> $value_type:path = $value:expr),+ $(,)?
    ) => {
        impl_int_enum!($vis $t($repr):
            $($case ($const) = $value,)+
        );

        $(
            #[derive(Clone, Copy)]
            $vis struct $case;

            impl $trait for $case {
                type Value<'db> = $value_type;

                #[inline]
                fn disciminator() -> $t {
                    $t::$case
                }
            }

            impl std::fmt::Display for $case {
                #[inline]
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    std::fmt::Display::fmt(&Self::disciminator(), f)
                }
            }
        )+
    };
}

pub mod bucket_meta {
    use super::*;

    // WARN: Don’t change values here, it would break the index! Only add new cases.
    declare_meta_keys!(pub BucketMetaKeyDiscriminator(u32) / TypedBucketMetaKey:
        IidIncr (IID_INCR) -> crate::store::IidIncr = 0,
        ObjectCount (OBJECT_COUNT) -> crate::store::ObjectCount = 1,
    );
}

pub mod object_meta {
    use super::*;

    // WARN: Don’t change values here, it would break the index! Only add new cases.
    declare_meta_keys!(pub ObjectMetaKeyDiscriminator(u8) / TypedObjectMetaKey:
        IidIncr (IID_INCR) -> crate::store::IidIncr = 0,
        FragmentCount (OBJECT_COUNT) -> crate::store::FragmentCount = 1,
    );
}

pub(super) trait TypedKvStoreKey: std::fmt::Display {
    type Value<'db>;

    fn disciminator() -> KvStoreKeyDiscriminator;

    fn to_bytes(&self) -> Vec<u8>;
}

pub trait TypedBucketMetaKey: std::fmt::Display + Copy {
    type Value<'db>;

    fn disciminator() -> bucket_meta::BucketMetaKeyDiscriminator;
}

pub trait TypedObjectMetaKey: std::fmt::Display + Copy {
    type Value<'db>;

    fn disciminator() -> object_meta::ObjectMetaKeyDiscriminator;
}

#[repr(transparent)]
pub(super) struct TermToIidsIter<'db>(
    pub(super) crate::store::rocksdb::util::RocksDbPinnableSliceIter<'db, TermToIidsEntry>,
);

impl<'db> std::fmt::Debug for TermToIidsIter<'db> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TermToIidsIter").finish_non_exhaustive()
    }
}

#[repr(transparent)]
pub(super) struct IidToTermsIter<'db>(
    pub(super) crate::store::rocksdb::util::RocksDbPinnableSliceIter<'db, IidToTermsEntry>,
);

impl<'db> std::fmt::Debug for IidToTermsIter<'db> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IidToTermsIter").finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq)]
#[repr(transparent)]
pub(super) struct KvStoreKey(Vec<u8>);

impl KvStoreKey {
    #[inline]
    const fn new(value: Vec<u8>) -> Self {
        debug_assert!(value.len() > 3);
        Self(value)
    }

    /// Key format: `[bucket<?B> | discriminator<1B> | route<?B>]`
    fn make(
        idx: KvStoreKeyDiscriminator,
        bucket: impl ToKvKeyPart,
        route_bytes: &[u8],
    ) -> KvStoreKey {
        let bucket_repr = bucket.to_kv_key_part();
        let bucket_bytes = bucket_repr.as_ref();

        let mut key_bytes = Vec::with_capacity(bucket_bytes.len() + 1 + route_bytes.len());

        key_bytes.extend_from_slice(bucket_bytes);
        key_bytes.push(idx as u8);
        key_bytes.extend_from_slice(route_bytes);

        KvStoreKey::new(key_bytes)
    }
}

pub(super) struct KvStoreKeyParts<'a> {
    #[allow(dead_code)]
    pub(super) bucket: &'a [u8],
    pub(super) discriminator: u8,
    pub(super) route: &'a [u8],
}

impl<'a> TryFrom<&'a [u8]> for KvStoreKeyParts<'a> {
    type Error = &'static str;

    fn try_from(bytes: &'a [u8]) -> Result<Self, Self::Error> {
        // TODO: Move this logic near `Bucket` key encoding.
        let Some((&bucket_len, rest)) = bytes.split_first() else {
            return Err("key empty");
        };

        let Some((bucket, rest)) = rest.split_at_checked(usize::from(bucket_len)) else {
            return Err("key missing bucket");
        };

        let Some((&discriminator, route)) = rest.split_first() else {
            return Err("key missing discriminator");
        };

        Ok(Self {
            bucket,
            discriminator,
            route,
        })
    }
}

pub(super) struct ObjectMetaKeyParts<'a> {
    #[allow(dead_code)]
    pub(super) object: &'a <GenericIid as ToKvKeyPart>::Repr,
    pub(super) discriminator: u8,
}

impl<'a> TryFrom<&'a [u8]> for ObjectMetaKeyParts<'a> {
    type Error = &'static str;

    fn try_from(bytes: &'a [u8]) -> Result<Self, Self::Error> {
        // TODO: Move this logic near `GenericIid` key encoding.
        let Some((object, rest)) = bytes.split_first_chunk() else {
            return Err("key missing object");
        };

        let Some((&discriminator, route)) = rest.split_first() else {
            return Err("key missing discriminator");
        };

        debug_assert!(route.is_empty(), "route={route:?} (bytes={bytes:?})");

        Ok(Self {
            object,
            discriminator,
        })
    }
}

pub(super) fn prefix_range(bucket: &Bucket) -> std::ops::Range<Vec<u8>> {
    let bucket_bytes = bucket.as_bytes();

    let mut start = Vec::with_capacity(bucket_bytes.len() + 1);
    start.push(u8::try_from(bucket_bytes.len()).expect(ITEM_PART_MAX_LEN_ERROR));
    start.extend_from_slice(bucket_bytes);

    let mut end = Vec::clone(&start);
    // SAFETY: We’ve already pushed at least one byte.
    *end.last_mut().unwrap() += 1;

    start..end
}

// MARK: - Tests

#[cfg(test)]
#[rustfmt::skip]
mod tests {
    use super::*;

    #[test]
    fn it_keys_meta_to_value() {
        assert_eq!(
            BucketMetaToValue { bucket: "b:1".into(), meta: bucket_meta::IidIncr }.to_bytes(),
            [3, b'b', b':', b'1', 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn it_keys_term_to_iids() {
        assert_eq!(
            TermToIids { bucket: "b:2".into(), term_hash: &772137347u32.into() }.to_bytes(),
            [3, b'b', b':', b'2', 1, 131, 225, 5, 46]
        );
        assert_eq!(
            TermToIids { bucket: "b:2".into(), term_hash: &3582484684u32.into() }.to_bytes(),
            [3, b'b', b':', b'2', 1, 204, 96, 136, 213]
        );
    }

    #[test]
    fn it_keys_oid_to_iid() {
        assert_eq!(
            OidToIid { bucket: "b:3".into(), oid: "conv:6501e83a".into() }.to_bytes(),
            [3, b'b', b':', b'3', 2, 13, b'c', b'o', b'n', b'v', b':', b'6', b'5', b'0', b'1', b'e', b'8', b'3', b'a']
        );
    }

    #[test]
    fn it_keys_iid_to_oid() {
        assert_eq!(
            IidToOid { bucket: "b:4".into(), iid: 10292198.into() }.to_bytes(),
            [3, b'b', b':', b'4', 3, 0, 0, 0, 0, 0, 157, 11, 230]
        );
    }

    #[test]
    fn iid_to_oid_is_lexicographically_sorted() {
        assert!(
              IidToOid { bucket: "b:4".into(), iid: 1.into() }.to_bytes()
              < IidToOid { bucket: "b:4".into(), iid: 2.into() }.to_bytes()
        );
    }

    #[test]
    fn it_keys_iid_to_terms() {
        assert_eq!(
            IidToTerms { bucket: "b:5".into(), iid: 1.into() }.to_bytes(),
            [3, b'b', b':', b'5', 4, 0, 0, 0, 0, 0, 0, 0, 1]
        );
        assert_eq!(
            IidToTerms { bucket: "b:5".into(), iid: 20.into() }.to_bytes(),
            [3, b'b', b':', b'5', 4, 0, 0, 0, 0, 0, 0, 0, 20]
        );
    }

    #[test]
    fn it_hashes_compact() {
        assert_eq!(GenericOid::from("key:1").to_compact(), 3370353088);
        assert_eq!(GenericOid::from("key:2").to_compact(), 1042559698);
    }

    #[test]
    fn it_formats_key() {
        assert_eq!(
            &format!("{}", TermToIids { bucket: "b:6".into(), term_hash: &72137347u32.into() }),
            r#""b:6":TERM_TO_IIDS:<44cba83>"#
        );
        assert_eq!(
            &format!("{}", BucketMetaToValue { bucket: "b:6".into(), meta: bucket_meta::IidIncr }),
            r#""b:6":META_TO_VALUE:IID_INCR"#
        );
    }

    #[test]
    fn it_computes_key_ranges() {
        let range = prefix_range(&Bucket::from("ABC"));
        assert_eq!(range.start, &[3, b'A', b'B', b'C']);
        assert_eq!(range.end, &[3, b'A', b'B', b'C' + 1]);
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
