// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::store::generic::KEY_SEPARATOR;
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

pub(super) trait ToKvValue {
    type Repr: AsRef<[u8]>;

    fn to_kv_value(self) -> Self::Repr;
}

impl<const WORD_LEN: usize, T, I> ToKvValue for I
where
    I: ExactSizeIterator<Item = T>,
    T: ToKvValue<Repr = [u8; WORD_LEN]>,
{
    type Repr = Vec<u8>;

    fn to_kv_value(self) -> Self::Repr {
        let mut encoded: Vec<u8> = Vec::with_capacity(self.len() * WORD_LEN);

        for item in self {
            encoded.extend_from_slice(item.to_kv_value().as_ref());
        }

        encoded
    }
}

pub(super) trait FromKvValue: ToKvValue {
    fn from_kv_value(repr: Self::Repr) -> Self;
}

pub(super) trait TryFromKvValue: Sized {
    type Err;

    fn try_from_kv_value(bytes: &[u8]) -> Result<Self, Self::Err>;
}

impl<const WORD_LEN: usize, T> TryFromKvValue for Vec<T>
where
    T: FromKvValue<Repr = [u8; WORD_LEN]>,
{
    type Err = &'static str;

    fn try_from_kv_value(encoded: &[u8]) -> Result<Self, Self::Err> {
        match encoded.as_chunks() {
            (chunks, []) => {
                let mut decoded: Vec<T> = Vec::with_capacity(encoded.len() / WORD_LEN);

                for chunk in chunks {
                    let decoded_chunk = T::from_kv_value(*chunk);
                    decoded.push(decoded_chunk);
                }

                Ok(decoded)
            }
            (_, _) => Err("Too many bytes"),
        }
    }
}

// MARK: IID

impl ToKvKeyPart for StoreObjectIid {
    type Repr = [u8; 8];

    #[inline]
    fn to_kv_key_part(self) -> Self::Repr {
        self.into_inner().to_be_bytes()
    }
}

impl FromKvKeyPart for StoreObjectIid {
    #[inline]
    fn from_kv_key_part(repr: Self::Repr) -> Self {
        Self::new(u64::from_be_bytes(repr))
    }
}

impl ToKvValue for StoreObjectIid {
    type Repr = [u8; 8];

    #[inline]
    fn to_kv_value(self) -> Self::Repr {
        self.into_inner().to_le_bytes()
    }
}

impl FromKvValue for StoreObjectIid {
    fn from_kv_value(repr: Self::Repr) -> Self {
        Self::new(u64::from_le_bytes(repr))
    }
}

impl TryFromKvValue for StoreObjectIid {
    type Err = &'static str;

    fn try_from_kv_value(bytes: &[u8]) -> Result<Self, Self::Err> {
        match bytes.split_first_chunk() {
            Some((chunk, [])) => Ok(Self::from_kv_value(*chunk)),
            Some(_) => Err("Too many bytes"),
            None => Err("Missing bytes"),
        }
    }
}

// MARK: OID

impl<'a> ToKvKeyPart for &'a StoreObjectOid<'a> {
    type Repr = &'a [u8];

    #[inline]
    fn to_kv_key_part(self) -> Self::Repr {
        self.as_bytes()
    }
}

// MARK: Term hash

impl ToKvKeyPart for StoreTermHash {
    type Repr = [u8; 4];

    #[inline]
    fn to_kv_key_part(self) -> Self::Repr {
        self.into_inner().to_le_bytes()
    }
}

impl FromKvKeyPart for StoreTermHash {
    #[inline]
    fn from_kv_key_part(repr: Self::Repr) -> Self {
        Self::new(u32::from_le_bytes(repr))
    }
}

impl TryFromKvKeyPart for StoreTermHash {
    type Err = &'static str;

    fn try_from_kv_key_part(bytes: &[u8]) -> Result<Self, Self::Err> {
        match bytes.split_first_chunk() {
            Some((chunk, [])) => Ok(Self::from_kv_key_part(*chunk)),
            Some(_) => Err("Too many bytes"),
            None => Err("Missing bytes"),
        }
    }
}

impl ToKvValue for &StoreTermHash {
    type Repr = <StoreTermHash as ToKvValue>::Repr;

    #[inline]
    fn to_kv_value(self) -> Self::Repr {
        (*self).to_kv_value()
    }
}

impl ToKvValue for StoreTermHash {
    type Repr = [u8; 4];

    #[inline]
    fn to_kv_value(self) -> Self::Repr {
        self.into_inner().to_le_bytes()
    }
}

impl FromKvValue for StoreTermHash {
    fn from_kv_value(repr: Self::Repr) -> Self {
        Self::new(u32::from_le_bytes(repr))
    }
}

// MARK: Meta key

impl ToKvKeyPart for StoreMetaKey {
    type Repr = [u8; 4];

    #[inline]
    fn to_kv_key_part(self) -> Self::Repr {
        (self as u32).to_be_bytes()
    }
}

impl TryFromKvKeyPart for StoreMetaKey {
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

        res.extend_from_slice(bytes);
        res.push(KEY_SEPARATOR);

        res
    }
}

// MARK: IIDIncr counter

impl ToKvValue for IidIncr {
    type Repr = [u8; 8];

    #[inline]
    fn to_kv_value(self) -> Self::Repr {
        self.0.to_le_bytes()
    }
}

impl FromKvValue for IidIncr {
    #[inline]
    fn from_kv_value(repr: Self::Repr) -> Self {
        Self(StoreObjectIid::new(u64::from_le_bytes(repr)))
    }
}

impl TryFromKvValue for IidIncr {
    type Err = &'static str;

    fn try_from_kv_value(bytes: &[u8]) -> Result<Self, Self::Err> {
        match bytes.split_first_chunk() {
            Some((chunk, [])) => Ok(Self::from_kv_value(*chunk)),
            Some(_) => Err("Too many bytes"),
            None => Err("Missing bytes"),
        }
    }
}

// MARK: ObjectCount counter

impl ToKvValue for ObjectCount {
    type Repr = [u8; 8];

    #[inline]
    fn to_kv_value(self) -> Self::Repr {
        self.0.to_le_bytes()
    }
}

impl FromKvValue for ObjectCount {
    #[inline]
    fn from_kv_value(repr: Self::Repr) -> Self {
        Self(i64::from_le_bytes(repr))
    }
}

impl TryFromKvValue for ObjectCount {
    type Err = &'static str;

    fn try_from_kv_value(bytes: &[u8]) -> Result<Self, Self::Err> {
        match bytes.split_first_chunk() {
            Some((chunk, [])) => Ok(Self::from_kv_value(*chunk)),
            Some(_) => Err("Too many bytes"),
            None => Err("Missing bytes"),
        }
    }
}
