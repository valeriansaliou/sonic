// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::hash::Hasher;
use twox_hash::XxHash32;

macro_rules! impl_u32_wrapper_utils {
    ($t:ty) => {
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

        impl std::fmt::Debug for $t {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                std::fmt::Debug::fmt(&self.0, f)
            }
        }
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

impl std::str::FromStr for StoreObjectIID {
    type Err = <u32 as std::str::FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        u32::from_str(s).map(Self)
    }
}

pub type StoreObjectOID<'a> = &'a str;

/// Remember to use [`crate::util::hash::NoopU32HasherBuilder`], as
/// `StoreTermHash` values are already hashed (by xxHash)!
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct StoreTermHash(u32);

impl_u32_wrapper_utils!(StoreTermHash);

pub enum StoreMetaKey {
    IIDIncr,
}

pub enum StoreMetaValue {
    IIDIncr(StoreObjectIID),
}

impl StoreMetaKey {
    pub fn as_u32(&self) -> u32 {
        // WARN: Don’t update values here, it would break stuff
        //   (e.g. `default_merge_operator`)! Only add new cases.
        match self {
            StoreMetaKey::IIDIncr => 0,
        }
    }
}

impl From<&str> for StoreTermHash {
    fn from(term: &str) -> Self {
        let mut hasher = XxHash32::with_seed(0);

        hasher.write(term.as_bytes());

        Self(hasher.finish() as u32)
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
