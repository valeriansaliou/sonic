// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::ops::Deref;

use crate::Leveled as _;
use crate::store::types::*;

use super::keys::*;

pub(crate) trait ToKvKeyPart {
    type Repr: AsRef<[u8]>;

    fn to_kv_key_part(self) -> Self::Repr;
}

pub(super) trait FromKvKeyPart: ToKvKeyPart {
    fn from_kv_key_part(repr: Self::Repr) -> Self;
}

pub(super) trait TryFromKvKeyPart: Sized {
    type Err;

    fn try_from_kv_key_part(bytes: &[u8]) -> Result<Self, Self::Err>;
}

impl<T: ToKvKeyPart + Copy> ToKvKeyPart for &T {
    type Repr = T::Repr;

    #[inline]
    fn to_kv_key_part(self) -> Self::Repr {
        T::to_kv_key_part(*self)
    }
}

pub(super) trait TryFromKvValue<Value>: Sized + std::fmt::Debug {
    fn try_from_kv_value(value: Value) -> Result<Self, crate::Error>;
}

impl<'db, T> TryFromKvValue<rocksdb::DBPinnableSlice<'db>> for T
where
    T: for<'a> TryFromKvValue<&'a [u8]>,
{
    fn try_from_kv_value(value: rocksdb::DBPinnableSlice<'db>) -> Result<Self, crate::Error> {
        <Self as TryFromKvValue<&[u8]>>::try_from_kv_value(value.deref())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BadLength {
    #[error("Missing bytes")]
    TooShort,

    #[error("Too many bytes")]
    TooLong,
}

/// A wrapper around `zerocopy::SizeError` that’s not tied to a lifetime.
#[derive(Debug, thiserror::Error)]
#[repr(transparent)]
#[error("{0}")]
pub struct SizeError(String);

impl<'a, T> From<zerocopy::SizeError<&'a [u8], T>> for SizeError
where
    zerocopy::SizeError<&'a [u8], T>: std::fmt::Display,
{
    fn from(value: zerocopy::SizeError<&'a [u8], T>) -> Self {
        Self(value.to_string())
    }
}

// MARK: IID

impl ToKvKeyPart for GenericIid {
    type Repr = [u8; RAW_IID_SIZE];

    #[inline]
    fn to_kv_key_part(self) -> Self::Repr {
        RawIidKey::from(self.into_inner().get()).to_bytes()
    }
}

impl FromKvKeyPart for GenericIid {
    #[inline]
    fn from_kv_key_part(repr: Self::Repr) -> Self {
        Self::new(RawIidKey::from_bytes(repr).get().into())
    }
}

impl<'a> TryFromKvValue<&'a [u8]> for GenericIid {
    fn try_from_kv_value(bytes: &'a [u8]) -> Result<Self, crate::Error> {
        zerocopy::FromBytes::read_from_bytes(bytes)
            .map_err(SizeError::from)
            .as_error()
    }
}

// MARK: OID

impl<'a> ToKvKeyPart for &'a GenericOid<'a> {
    type Repr = Vec<u8>;

    #[inline]
    fn to_kv_key_part(self) -> Self::Repr {
        let bytes = self.as_bytes();

        let mut res = Vec::with_capacity(bytes.len() + 1);

        res.push(u8::try_from(bytes.len()).expect(ITEM_PART_MAX_LEN_ERROR));
        res.extend_from_slice(bytes);

        res
    }
}

impl TryFromKvValue<&[u8]> for String {
    fn try_from_kv_value(bytes: &[u8]) -> Result<Self, crate::Error> {
        str::from_utf8(bytes).map(str::to_string).as_error()
    }
}

// MARK: Term hash

impl ToKvKeyPart for StoreTermHash {
    type Repr = [u8; 4];

    #[inline]
    fn to_kv_key_part(self) -> Self::Repr {
        zerocopy::little_endian::U32::from(self.into_inner().get()).to_bytes()
    }
}

impl FromKvKeyPart for StoreTermHash {
    #[inline]
    fn from_kv_key_part(repr: Self::Repr) -> Self {
        Self::new(zerocopy::little_endian::U32::from_bytes(repr).get().into())
    }
}

impl TryFromKvKeyPart for StoreTermHash {
    type Err = BadLength;

    fn try_from_kv_key_part(bytes: &[u8]) -> Result<Self, Self::Err> {
        match bytes.split_first_chunk() {
            Some((chunk, [])) => Ok(Self::from_kv_key_part(*chunk)),
            Some(_) => Err(BadLength::TooLong),
            None => Err(BadLength::TooShort),
        }
    }
}

// MARK: Bucket meta key

impl ToKvKeyPart for bucket_meta::BucketMetaKeyDiscriminator {
    type Repr = [u8; 4];

    #[inline]
    fn to_kv_key_part(self) -> Self::Repr {
        (self as u32).to_be_bytes()
    }
}

impl TryFromKvKeyPart for bucket_meta::BucketMetaKeyDiscriminator {
    type Err = String;

    fn try_from_kv_key_part(bytes: &[u8]) -> Result<Self, Self::Err> {
        match bytes.split_first_chunk() {
            Some((chunk, [])) => Self::try_from(u32::from_be_bytes(*chunk)),
            Some(_) => Err("Too many bytes".to_owned()),
            None => Err("Missing bytes".to_owned()),
        }
    }
}

// MARK: Object meta key

impl ToKvKeyPart for object_meta::ObjectMetaKeyDiscriminator {
    type Repr = [u8; 1];

    #[inline]
    fn to_kv_key_part(self) -> Self::Repr {
        (self as u8).to_be_bytes()
    }
}

// MARK: Bucket

impl<'a> ToKvKeyPart for &'a Bucket<'a> {
    type Repr = Vec<u8>;

    fn to_kv_key_part(self) -> Self::Repr {
        let bytes = self.as_bytes();

        let mut res = Vec::with_capacity(bytes.len() + 1);

        res.push(u8::try_from(bytes.len()).expect(ITEM_PART_MAX_LEN_ERROR));
        res.extend_from_slice(bytes);

        res
    }
}

// MARK: IIDIncr counter

impl<'a> TryFromKvValue<&'a [u8]> for IidIncr {
    fn try_from_kv_value(bytes: &'a [u8]) -> Result<Self, crate::Error> {
        zerocopy::FromBytes::read_from_bytes(bytes)
            .map_err(SizeError::from)
            .as_error()
    }
}

// MARK: ObjectCount counter

impl<'a> TryFromKvValue<&'a [u8]> for ObjectCount {
    fn try_from_kv_value(bytes: &'a [u8]) -> Result<Self, crate::Error> {
        zerocopy::FromBytes::read_from_bytes(bytes)
            .map_err(SizeError::from)
            .as_error()
    }
}

// MARK: FragmentCount counter

impl<'a> TryFromKvValue<&'a [u8]> for FragmentCount {
    fn try_from_kv_value(bytes: &'a [u8]) -> Result<Self, crate::Error> {
        zerocopy::FromBytes::read_from_bytes(bytes)
            .map_err(SizeError::from)
            .as_error()
    }
}

// Iterators

impl<'db> TryFromKvValue<rocksdb::DBPinnableSlice<'db>> for IidToTermsIter<'db> {
    fn try_from_kv_value(slice: rocksdb::DBPinnableSlice<'db>) -> Result<Self, crate::Error> {
        crate::store::rocksdb::util::RocksDbPinnableSliceIter::<super::IidToTermsEntry>::forward(
            slice,
        )
        .map(Self)
        .as_error()
    }
}

impl<'db> TryFromKvValue<rocksdb::DBPinnableSlice<'db>> for TermToIidsIter<'db> {
    fn try_from_kv_value(slice: rocksdb::DBPinnableSlice<'db>) -> Result<Self, crate::Error> {
        crate::store::rocksdb::util::RocksDbPinnableSliceIter::<super::TermToIidsEntry>::forward(
            slice,
        )
        .map(Self)
        .as_error()
    }
}
