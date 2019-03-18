// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::hash::Hasher;
use twox_hash::XxHash32;

pub type StoreObjectIID = u32;
pub type StoreObjectOID = String;
pub type StoreTermHashed = u32;

pub struct StoreTermHash;

pub enum StoreMetaKey {
    IIDIncr,
}

pub enum StoreMetaValue {
    IIDIncr(StoreObjectIID),
}

impl StoreMetaKey {
    pub fn as_u32(&self) -> u32 {
        match self {
            StoreMetaKey::IIDIncr => 0,
        }
    }
}

impl StoreTermHash {
    pub fn from(term: &str) -> StoreTermHashed {
        let mut hasher = XxHash32::with_seed(0);

        hasher.write(term.as_bytes());

        hasher.finish() as u32
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
        assert_eq!(StoreTermHash::from("hash:1"), 3637660813);
        assert_eq!(StoreTermHash::from("hash:2"), 3577985381);
    }
}
