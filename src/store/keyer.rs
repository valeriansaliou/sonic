// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use radix_fmt::{radix, Radix};
use std::hash::Hasher;
use twox_hash::XxHash32;

use super::identifiers::*;

pub struct StoreKeyerBuilder;

pub struct StoreKeyer<'a> {
    idx: StoreKeyerIdx<'a>,
    bucket: StoreKeyerBucket<'a>,
}

enum StoreKeyerIdx<'a> {
    MetaToValue(&'a StoreMetaKey),
    TermToIIDs(StoreTermHashed),
    OIDToIID(&'a StoreObjectOID),
    IIDToOID(StoreObjectIID),
    IIDToTerms(StoreObjectIID),
}

type StoreKeyerBucket<'a> = &'a str;

const STORE_KEYER_COMPACT_BASE: u32 = 36;

impl<'a> StoreKeyerIdx<'a> {
    pub fn to_index(&self) -> u8 {
        match self {
            StoreKeyerIdx::MetaToValue(_) => 0,
            StoreKeyerIdx::TermToIIDs(_) => 1,
            StoreKeyerIdx::OIDToIID(_) => 2,
            StoreKeyerIdx::IIDToOID(_) => 3,
            StoreKeyerIdx::IIDToTerms(_) => 4,
        }
    }
}

impl StoreKeyerBuilder {
    pub fn meta_to_value<'a>(bucket: &'a str, meta: &'a StoreMetaKey) -> StoreKeyer<'a> {
        StoreKeyer {
            idx: StoreKeyerIdx::MetaToValue(meta),
            bucket: bucket,
        }
    }

    pub fn term_to_iids<'a>(bucket: &'a str, term_hash: StoreTermHashed) -> StoreKeyer<'a> {
        StoreKeyer {
            idx: StoreKeyerIdx::TermToIIDs(term_hash),
            bucket: bucket,
        }
    }

    pub fn oid_to_iid<'a>(bucket: &'a str, oid: &'a StoreObjectOID) -> StoreKeyer<'a> {
        StoreKeyer {
            idx: StoreKeyerIdx::OIDToIID(oid),
            bucket: bucket,
        }
    }

    pub fn iid_to_oid<'a>(bucket: &'a str, iid: StoreObjectIID) -> StoreKeyer<'a> {
        StoreKeyer {
            idx: StoreKeyerIdx::IIDToOID(iid),
            bucket: bucket,
        }
    }

    pub fn iid_to_terms<'a>(bucket: &'a str, iid: StoreObjectIID) -> StoreKeyer<'a> {
        StoreKeyer {
            idx: StoreKeyerIdx::IIDToTerms(iid),
            bucket: bucket,
        }
    }
}

impl<'a> StoreKeyer<'a> {
    pub fn to_string(&self) -> String {
        let compact_bucket = radix(
            Self::hash_compact(self.bucket.as_bytes()),
            STORE_KEYER_COMPACT_BASE,
        );

        format!(
            "{}:{}:{}",
            self.idx.to_index(),
            compact_bucket,
            self.route_to_compact(),
        )
    }

    pub fn route_to_compact(&self) -> Radix<u64> {
        let value = match &self.idx {
            StoreKeyerIdx::MetaToValue(route) => route.as_u64(),
            StoreKeyerIdx::TermToIIDs(route) => *route as u64,
            StoreKeyerIdx::OIDToIID(route) => Self::hash_compact(route.as_bytes()),
            StoreKeyerIdx::IIDToOID(route) => *route,
            StoreKeyerIdx::IIDToTerms(route) => *route,
        };

        radix(value, STORE_KEYER_COMPACT_BASE)
    }

    fn hash_compact(part: &[u8]) -> u64 {
        let mut hasher = XxHash32::with_seed(0);

        hasher.write(part);
        hasher.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_keys_meta_to_value() {
        assert_eq!(
            StoreKeyerBuilder::meta_to_value("user:0dcde3a6", &StoreMetaKey::IIDIncr).to_string(),
            "0:vngsgj:0"
        );
    }

    #[test]
    fn it_keys_term_to_iids() {
        assert_eq!(
            StoreKeyerBuilder::term_to_iids("user:0dcde3a6", 772137347).to_string(),
            "1:vngsgj:crpkzn"
        );
        assert_eq!(
            StoreKeyerBuilder::term_to_iids("default", 3582484684).to_string(),
            "1:tlegv5:1n8x2vg"
        );
    }

    #[test]
    fn it_keys_oid_to_iid() {
        assert_eq!(
            StoreKeyerBuilder::oid_to_iid("user:0dcde3a6", &"conversation:6501e83a".to_string())
                .to_string(),
            "2:vngsgj:1n884db"
        );
    }

    #[test]
    fn it_keys_iid_to_oid() {
        assert_eq!(
            StoreKeyerBuilder::iid_to_oid("user:0dcde3a6", 10292198).to_string(),
            "3:vngsgj:64lie"
        );
    }

    #[test]
    fn it_keys_iid_to_terms() {
        assert_eq!(
            StoreKeyerBuilder::iid_to_terms("user:0dcde3a6", 1).to_string(),
            "4:vngsgj:1"
        );
        assert_eq!(
            StoreKeyerBuilder::iid_to_terms("user:0dcde3a6", 20).to_string(),
            "4:vngsgj:k"
        );
    }
}
