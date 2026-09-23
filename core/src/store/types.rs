// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

// MARK: IID

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct StoreObjectIid(u32);

impl_u32_wrapper_utils!(StoreObjectIid);

impl StoreObjectIid {
    #[inline]
    pub const fn saturating_add(self, rhs: u32) -> Self {
        Self(self.0.saturating_add(rhs))
    }

    // NOTE: We went for `into_inner` here instead of marking `.0` `pub(super)`
    //   so it’s easier to identify call sites and keep constuction via `From`.
    #[inline]
    pub(super) const fn into_inner(self) -> u32 {
        self.0
    }
}

// MARK: OID

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct StoreObjectOid<'a>(StoreItemPart<'a>);

crate::util::impl_transparent_wrapper_utils!(Deref for StoreObjectOid<'a>(StoreItemPart<'a>));
crate::util::impl_transparent_wrapper_utils!(From for StoreObjectOid<'a>(StoreItemPart<'a>));
crate::util::impl_transparent_wrapper_utils!(Debug for StoreObjectOid<'a>(StoreItemPart<'a>));
crate::util::impl_transparent_wrapper_utils!(Display for StoreObjectOid<'a>(StoreItemPart<'a>));

// MARK: Bucket

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Bucket<'a>(StoreItemPart<'a>);

impl<'a> Bucket<'a> {
    pub fn to_bytes(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }
}

crate::util::impl_transparent_wrapper_utils!(Deref for Bucket<'a>(StoreItemPart<'a>));
crate::util::impl_transparent_wrapper_utils!(From for Bucket<'a>(StoreItemPart<'a>));
crate::util::impl_transparent_wrapper_utils!(Debug for Bucket<'a>(StoreItemPart<'a>));
crate::util::impl_transparent_wrapper_utils!(Display for Bucket<'a>(StoreItemPart<'a>));

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct BucketOwned(String);

crate::util::impl_transparent_wrapper_utils!(Deref for BucketOwned(String));
crate::util::impl_transparent_wrapper_utils!(Debug for BucketOwned(String));
crate::util::impl_transparent_wrapper_utils!(Display for BucketOwned(String));

impl<'a> From<Bucket<'a>> for BucketOwned {
    fn from(value: Bucket<'a>) -> Self {
        Self(value.0.0.to_owned())
    }
}

impl<'a> From<&'a BucketOwned> for Bucket<'a> {
    fn from(value: &'a BucketOwned) -> Self {
        Self(StoreItemPart(value.0.as_str()))
    }
}

impl std::str::FromStr for BucketOwned {
    type Err = std::io::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match StoreItemPart::from_str(s) {
            Ok(part) => Ok(Self(part.0.to_owned())),
            Err(()) => Err(std::io::Error::other("Invalid bucket")),
        }
    }
}

// MARK: Term hash

/// Remember to use [`crate::util::hash::NoopU32HasherBuilder`], as
/// `StoreTermHash` values are already hashed (by xxHash)!
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct StoreTermHash(u32);

impl_u32_wrapper_utils!(StoreTermHash);

impl StoreTermHash {
    // NOTE: We went for `into_inner` here instead of marking `.0` `pub(super)`
    //   so it’s easier to identify call sites and keep constuction via `From`.
    #[inline]
    pub(super) const fn into_inner(self) -> u32 {
        self.0
    }
}

impl From<&str> for StoreTermHash {
    fn from(term: &str) -> Self {
        use std::hash::Hasher as _;
        use twox_hash::XxHash32;

        let mut hasher = XxHash32::with_seed(0);

        hasher.write(term.as_bytes());

        Self(hasher.finish() as u32)
    }
}

#[cfg(test)]
mod tests_store_term_hash {
    use super::*;

    #[test]
    fn it_hashes_term() {
        assert_eq!(StoreTermHash::from("hash:1").0, 3637660813);
        assert_eq!(StoreTermHash::from("hash:2").0, 3577985381);
        assert_eq!(StoreTermHash::from("hash:3").0, 0724158430);
        assert_eq!(StoreTermHash::from("hash:4").0, 0618576593);
    }
}

// MARK: Item part

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct StoreItemPart<'a>(&'a str);

crate::util::impl_transparent_wrapper_utils!(Deref for StoreItemPart<'a>(&'a str));
crate::util::impl_transparent_wrapper_utils!(Debug for StoreItemPart<'a>(&'a str));
crate::util::impl_transparent_wrapper_utils!(Display for StoreItemPart<'a>(&'a str));

const STORE_ITEM_PART_LEN_MIN: usize = 1;
const STORE_ITEM_PART_LEN_MAX: usize = 128;

impl<'a> StoreItemPart<'a> {
    #[allow(clippy::should_implement_trait)]
    fn from_str(part: &'a str) -> Result<Self, ()> {
        let len = part.len();

        if (STORE_ITEM_PART_LEN_MIN..=STORE_ITEM_PART_LEN_MAX).contains(&len) && part.is_ascii() {
            Ok(StoreItemPart(part))
        } else {
            Err(())
        }
    }

    pub fn into_compact(&self) -> u32 {
        use std::hash::Hasher as _;
        use twox_hash::XxHash32;

        let mut hasher = XxHash32::with_seed(0);

        hasher.write(self.0.as_bytes());
        hasher.finish() as u32
    }
}

#[cfg(test)]
impl From<&'static str> for StoreItemPart<'static> {
    fn from(value: &'static str) -> Self {
        Self(value)
    }
}

pub fn bucket(str: &'static str) -> StoreItemPart<'static> {
    StoreItemPart::from_str(str).unwrap()
}

pub enum StoreItemBuilder {}

#[allow(clippy::enum_variant_names)]
#[derive(Debug, PartialEq)]
pub enum StoreItemError {
    InvalidCollection,
    InvalidBucket,
    InvalidObject,
}

impl StoreItemBuilder {
    pub fn from_depth_1<'a>(collection: &'a str) -> Result<StoreItemPart<'a>, StoreItemError> {
        // Validate & box collection
        if let Ok(collection_item) = StoreItemPart::from_str(collection) {
            Ok(collection_item)
        } else {
            Err(StoreItemError::InvalidCollection)
        }
    }

    pub fn from_depth_2<'a>(
        collection: &'a str,
        bucket: &'a str,
    ) -> Result<(StoreItemPart<'a>, Bucket<'a>), StoreItemError> {
        // Validate & box collection + bucket
        match (
            StoreItemPart::from_str(collection),
            StoreItemPart::from_str(bucket),
        ) {
            (Ok(collection_item), Ok(bucket_item)) => {
                Ok((collection_item, Bucket::from(bucket_item)))
            }
            (Err(_), _) => Err(StoreItemError::InvalidCollection),
            (_, Err(_)) => Err(StoreItemError::InvalidBucket),
        }
    }

    pub fn from_depth_3<'a>(
        collection: &'a str,
        bucket: &'a str,
        object: &'a str,
    ) -> Result<(StoreItemPart<'a>, Bucket<'a>, StoreObjectOid<'a>), StoreItemError> {
        // Validate & box collection + bucket + object
        match (
            StoreItemPart::from_str(collection),
            StoreItemPart::from_str(bucket),
            StoreItemPart::from_str(object),
        ) {
            (Ok(collection_item), Ok(bucket_item), Ok(object_item)) => Ok((
                collection_item,
                Bucket::from(bucket_item),
                StoreObjectOid::from(object_item),
            )),
            (Err(_), _, _) => Err(StoreItemError::InvalidCollection),
            (_, Err(_), _) => Err(StoreItemError::InvalidBucket),
            (_, _, Err(_)) => Err(StoreItemError::InvalidObject),
        }
    }
}

#[cfg(test)]
mod tests_store_item_builder {
    use super::*;

    #[test]
    fn it_builds_store_item_depth_1() {
        assert_eq!(
            StoreItemBuilder::from_depth_1("c:test:1"),
            Ok(StoreItemPart("c:test:1"))
        );
        assert_eq!(
            StoreItemBuilder::from_depth_1(""),
            Err(StoreItemError::InvalidCollection)
        );
    }

    #[test]
    fn it_builds_store_item_depth_2() {
        assert_eq!(
            StoreItemBuilder::from_depth_2("c:test:2", "b:test:2"),
            Ok((StoreItemPart("c:test:2"), StoreItemPart("b:test:2").into()))
        );
        assert_eq!(
            StoreItemBuilder::from_depth_2("", "b:test:2"),
            Err(StoreItemError::InvalidCollection)
        );
        assert_eq!(
            StoreItemBuilder::from_depth_2("c:test:2", ""),
            Err(StoreItemError::InvalidBucket)
        );
    }

    #[test]
    fn it_builds_store_item_depth_3() {
        assert_eq!(
            StoreItemBuilder::from_depth_3("c:test:3", "b:test:3", "o:test:3"),
            Ok((
                StoreItemPart("c:test:3"),
                StoreItemPart("b:test:3").into(),
                StoreItemPart("o:test:3").into()
            ))
        );
        assert_eq!(
            StoreItemBuilder::from_depth_3("", "b:test:3", "o:test:3"),
            Err(StoreItemError::InvalidCollection)
        );
        assert_eq!(
            StoreItemBuilder::from_depth_3("c:test:3", "", "o:test:3"),
            Err(StoreItemError::InvalidBucket)
        );
        assert_eq!(
            StoreItemBuilder::from_depth_3("c:test:3", "b:test:3", ""),
            Err(StoreItemError::InvalidObject)
        );
    }
}

// MARK: - Helpers

macro_rules! impl_u32_wrapper_utils {
    ($t:ident) => {
        impl From<u32> for $t {
            fn from(value: u32) -> Self {
                Self(value)
            }
        }

        impl From<$t> for u32 {
            fn from(value: $t) -> Self {
                value.0
            }
        }

        impl From<&$t> for u32 {
            fn from(value: &$t) -> Self {
                value.0
            }
        }

        crate::util::impl_transparent_wrapper_utils!(Deref for $t(u32));
        crate::util::impl_transparent_wrapper_utils!(Debug for $t(u32));
        crate::util::impl_transparent_wrapper_utils!(FromStr for $t(u32));
    };
}
use impl_u32_wrapper_utils;
