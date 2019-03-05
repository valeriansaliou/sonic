// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::hash::Hasher;
use radix_fmt::{radix, Radix};
use twox_hash::XxHash;

use super::identifiers::*;

pub struct StoreKeyerBuilder;

pub struct StoreKeyer<'a> {
    idx: StoreKeyerIdx<'a>,
    bucket: StoreKeyerBucket<'a>,
}

enum StoreKeyerIdx<'a> {
    TermToIIDs(&'a str),
    OIDToIID(StoreObjectOID),
    IIDToOID(StoreObjectIID),
    IIDToTerms(StoreObjectIID),
}

type StoreKeyerBucket<'a> = &'a str;
type StoreKeyerRouteCompacted = Radix<u64>;

const STORE_KEYER_ROUTE_COMPACT_BASE: u32 = 36;

impl<'a> StoreKeyerIdx<'a> {
    pub fn to_index(&self) -> u8 {
        match self {
            StoreKeyerIdx::TermToIIDs(_) => 0,
            StoreKeyerIdx::OIDToIID(_) => 1,
            StoreKeyerIdx::IIDToOID(_) => 2,
            StoreKeyerIdx::IIDToTerms(_) => 3,
        }
    }
}

impl StoreKeyerBuilder {
    pub fn term_to_iids<'a>(bucket: &'a str, route: &'a str) -> StoreKeyer<'a> {
        StoreKeyer {
            idx: StoreKeyerIdx::TermToIIDs(route),
            bucket: bucket
        }
    }

    pub fn oid_to_iid<'a>(bucket: &'a str, route: StoreObjectOID) -> StoreKeyer<'a> {
        StoreKeyer {
            idx: StoreKeyerIdx::OIDToIID(route),
            bucket: bucket
        }
    }

    pub fn iid_to_oid<'a>(bucket: &'a str, route: StoreObjectIID) -> StoreKeyer<'a> {
        StoreKeyer {
            idx: StoreKeyerIdx::IIDToOID(route),
            bucket: bucket
        }
    }

    pub fn iid_to_terms<'a>(bucket: &'a str, route: StoreObjectIID) -> StoreKeyer<'a> {
        StoreKeyer {
            idx: StoreKeyerIdx::IIDToTerms(route),
            bucket: bucket
        }
    }
}

impl<'a> StoreKeyer<'a> {
    pub fn to_string(&self) -> String {
        format!("{}:{}:{}", self.idx.to_index(), self.bucket, self.route_to_compact())
    }

    pub fn route_to_compact(&self) -> StoreKeyerRouteCompacted {
        let value = match &self.idx {
            StoreKeyerIdx::TermToIIDs(route) => Self::hash_route_text(route),
            StoreKeyerIdx::OIDToIID(route) => Self::hash_route_text(route),
            StoreKeyerIdx::IIDToOID(route) => *route,
            StoreKeyerIdx::IIDToTerms(route) => *route,
        };

        radix(value, STORE_KEYER_ROUTE_COMPACT_BASE)
    }

    fn hash_route_text(text: &str) -> u64 {
        let mut hasher = XxHash::with_seed(0);

        hasher.write(text.as_bytes());
        hasher.finish()
    }
}
