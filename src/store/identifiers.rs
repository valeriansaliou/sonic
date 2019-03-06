// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::hash::Hasher;
use twox_hash::XxHash32;

pub type StoreObjectIID = u64;
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
    pub fn as_u64(&self) -> u64 {
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
