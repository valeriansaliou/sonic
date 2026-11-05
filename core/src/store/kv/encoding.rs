// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::ops::Deref;

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
    type Err: std::error::Error;

    fn try_from_kv_value(value: Value) -> Result<Self, Self::Err>;
}

impl<'db, T, E> TryFromKvValue<rocksdb::DBPinnableSlice<'db>> for T
where
    T: for<'a> TryFromKvValue<&'a [u8], Err = E>,
    E: std::error::Error,
{
    type Err = E;

    fn try_from_kv_value(value: rocksdb::DBPinnableSlice<'db>) -> Result<Self, Self::Err> {
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

impl ToKvKeyPart for StoreObjectIid {
    type Repr = [u8; 8];

    #[inline]
    fn to_kv_key_part(self) -> Self::Repr {
        zerocopy::big_endian::U64::from(self.into_inner().get()).to_bytes()
    }
}

impl FromKvKeyPart for StoreObjectIid {
    #[inline]
    fn from_kv_key_part(repr: Self::Repr) -> Self {
        Self::new(zerocopy::big_endian::U64::from_bytes(repr).get().into())
    }
}

impl<'a> TryFromKvValue<&'a [u8]> for StoreObjectIid {
    type Err = SizeError;

    fn try_from_kv_value(bytes: &'a [u8]) -> Result<Self, Self::Err> {
        zerocopy::FromBytes::read_from_bytes(bytes).map_err(Into::into)
    }
}

// MARK: OID

impl<'a> ToKvKeyPart for &'a StoreObjectOid<'a> {
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
    type Err = std::str::Utf8Error;

    fn try_from_kv_value(bytes: &[u8]) -> Result<Self, Self::Err> {
        str::from_utf8(bytes).map(str::to_string)
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

// MARK: Meta key

impl ToKvKeyPart for meta::BucketMetaKeyDiscriminator {
    type Repr = [u8; 4];

    #[inline]
    fn to_kv_key_part(self) -> Self::Repr {
        (self as u32).to_be_bytes()
    }
}

impl TryFromKvKeyPart for meta::BucketMetaKeyDiscriminator {
    type Err = String;

    fn try_from_kv_key_part(bytes: &[u8]) -> Result<Self, Self::Err> {
        match bytes.split_first_chunk() {
            Some((chunk, [])) => Self::try_from(u32::from_be_bytes(*chunk)),
            Some(_) => Err("Too many bytes".to_owned()),
            None => Err("Missing bytes".to_owned()),
        }
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
    type Err = SizeError;

    fn try_from_kv_value(bytes: &'a [u8]) -> Result<Self, Self::Err> {
        zerocopy::FromBytes::read_from_bytes(bytes).map_err(Into::into)
    }
}

// MARK: ObjectCount counter

impl<'a> TryFromKvValue<&'a [u8]> for ObjectCount {
    type Err = SizeError;

    fn try_from_kv_value(bytes: &'a [u8]) -> Result<Self, Self::Err> {
        zerocopy::FromBytes::read_from_bytes(bytes).map_err(Into::into)
    }
}

// Iterators

impl<'db> TryFromKvValue<rocksdb::DBPinnableSlice<'db>> for IidToTermsIter<'db> {
    type Err = crate::store::rocksdb::util::BadLength;

    fn try_from_kv_value(slice: rocksdb::DBPinnableSlice<'db>) -> Result<Self, Self::Err> {
        crate::store::rocksdb::util::RocksDbPinnableSliceIter::<super::IidToTermsEntry>::forward(
            slice,
        )
        .map(Self)
    }
}

impl<'db> TryFromKvValue<rocksdb::DBPinnableSlice<'db>> for TermToIidsIter<'db> {
    type Err = crate::store::rocksdb::util::BadLength;

    fn try_from_kv_value(slice: rocksdb::DBPinnableSlice<'db>) -> Result<Self, Self::Err> {
        crate::store::rocksdb::util::RocksDbPinnableSliceIter::<super::TermToIidsEntry>::forward(
            slice,
        )
        .map(Self)
    }
}
