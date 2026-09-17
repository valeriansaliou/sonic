// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::store::identifiers::StoreObjectOID;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct StoreItemPart<'a>(pub(super) &'a str);

crate::util::impl_transparent_wrapper_utils!(base for StoreItemPart<'a>(&'a str));
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

impl<'a> AsRef<str> for StoreItemPart<'a> {
    fn as_ref(&self) -> &str {
        self.0
    }
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
    ) -> Result<(StoreItemPart<'a>, StoreItemPart<'a>), StoreItemError> {
        // Validate & box collection + bucket
        match (
            StoreItemPart::from_str(collection),
            StoreItemPart::from_str(bucket),
        ) {
            (Ok(collection_item), Ok(bucket_item)) => Ok((collection_item, bucket_item)),
            (Err(_), _) => Err(StoreItemError::InvalidCollection),
            (_, Err(_)) => Err(StoreItemError::InvalidBucket),
        }
    }

    pub fn from_depth_3<'a>(
        collection: &'a str,
        bucket: &'a str,
        object: &'a str,
    ) -> Result<(StoreItemPart<'a>, StoreItemPart<'a>, StoreObjectOID<'a>), StoreItemError> {
        // Validate & box collection + bucket + object
        match (
            StoreItemPart::from_str(collection),
            StoreItemPart::from_str(bucket),
            StoreItemPart::from_str(object),
        ) {
            (Ok(collection_item), Ok(bucket_item), Ok(object_item)) => Ok((
                collection_item,
                bucket_item,
                StoreObjectOID::from(object_item),
            )),
            (Err(_), _, _) => Err(StoreItemError::InvalidCollection),
            (_, Err(_), _) => Err(StoreItemError::InvalidBucket),
            (_, _, Err(_)) => Err(StoreItemError::InvalidObject),
        }
    }
}

#[cfg(test)]
mod tests {
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
            Ok((StoreItemPart("c:test:2"), StoreItemPart("b:test:2")))
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
                StoreItemPart("b:test:3"),
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
