// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::store::StoreItemPart;

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

        crate::util::impl_transparent_wrapper_utils!(base for $t(u32));
        crate::util::impl_transparent_wrapper_utils!(Debug for $t(u32));
    };
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct StoreObjectIID(u32);

impl_u32_wrapper_utils!(StoreObjectIID);

impl StoreObjectIID {
    pub fn saturating_add(self, rhs: u32) -> Self {
        Self(self.0.saturating_add(rhs))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct StoreObjectOID<'a>(pub(super) StoreItemPart<'a>);

crate::util::impl_transparent_wrapper_utils!(base for StoreObjectOID<'a>(StoreItemPart<'a>));
crate::util::impl_transparent_wrapper_utils!(Debug for StoreObjectOID<'a>(StoreItemPart<'a>));
crate::util::impl_transparent_wrapper_utils!(Display for StoreObjectOID<'a>(StoreItemPart<'a>));

impl<'a, T> From<T> for StoreObjectOID<'a>
where
    StoreItemPart<'a>: From<T>,
{
    fn from(value: T) -> Self {
        Self(value.into())
    }
}

/// Remember to use [`crate::util::hash::NoopU32HasherBuilder`], as
/// `StoreTermHash` values are already hashed (by xxHash)!
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct StoreTermHash(u32);

impl_u32_wrapper_utils!(StoreTermHash);

impl From<&str> for StoreTermHash {
    fn from(term: &str) -> Self {
        use std::hash::Hasher as _;
        use twox_hash::XxHash32;

        let mut hasher = XxHash32::with_seed(0);

        hasher.write(term.as_bytes());

        Self(hasher.finish() as u32)
    }
}

pub enum StoreMetaKey {
    IIDIncr,
}

pub enum StoreMetaValue {
    IIDIncr(StoreObjectIID),
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
    fn it_converts_meta_key_to_u32() {
        assert_eq!(StoreMetaKey::IIDIncr.as_u32(), 0);
    }

    #[test]
    fn it_hashes_term() {
        assert_eq!(StoreTermHash::from("hash:1"), 3637660813.into());
        assert_eq!(StoreTermHash::from("hash:2"), 3577985381.into());
    }
}
