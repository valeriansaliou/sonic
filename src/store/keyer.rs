// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use byteorder::{ByteOrder, NativeEndian, ReadBytesExt};
use std::fmt;
use std::hash::Hasher;
use std::io::Cursor;
use twox_hash::XxHash32;

use super::identifiers::*;

pub struct StoreKeyerBuilder;

pub struct StoreKeyer {
    key: StoreKeyerKey,
}

enum StoreKeyerIdx<'a> {
    MetaToValue(&'a StoreMetaKey),
    TermToIIDs(StoreTermHashed),
    OIDToIID(&'a StoreObjectOID),
    IIDToOID(StoreObjectIID),
    IIDToTerms(StoreObjectIID),
}

type StoreKeyerKey = [u8; 9];

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
    pub fn meta_to_value<'a>(bucket: &'a str, meta: &'a StoreMetaKey) -> StoreKeyer {
        Self::make(StoreKeyerIdx::MetaToValue(meta), bucket)
    }

    pub fn term_to_iids<'a>(bucket: &'a str, term_hash: StoreTermHashed) -> StoreKeyer {
        Self::make(StoreKeyerIdx::TermToIIDs(term_hash), bucket)
    }

    pub fn oid_to_iid<'a>(bucket: &'a str, oid: &'a StoreObjectOID) -> StoreKeyer {
        Self::make(StoreKeyerIdx::OIDToIID(oid), bucket)
    }

    pub fn iid_to_oid<'a>(bucket: &'a str, iid: StoreObjectIID) -> StoreKeyer {
        Self::make(StoreKeyerIdx::IIDToOID(iid), bucket)
    }

    pub fn iid_to_terms<'a>(bucket: &'a str, iid: StoreObjectIID) -> StoreKeyer {
        Self::make(StoreKeyerIdx::IIDToTerms(iid), bucket)
    }

    fn make<'a>(idx: StoreKeyerIdx<'a>, bucket: &'a str) -> StoreKeyer {
        StoreKeyer {
            key: Self::build_key(idx, bucket),
        }
    }

    fn build_key<'a>(idx: StoreKeyerIdx<'a>, bucket: &'a str) -> StoreKeyerKey {
        // Key format: [idx<1B> | bucket<4B> | route<4B>]

        // Encode key bucket + key route from u32 to array of u8 (ie. binary)
        let (mut bucket_encoded, mut route_encoded) = ([0; 4], [0; 4]);

        NativeEndian::write_u32(&mut bucket_encoded, Self::hash_compact(bucket));
        NativeEndian::write_u32(&mut route_encoded, Self::route_to_compact(&idx));

        // Generate final binary key
        [
            // [idx<1B>]
            idx.to_index(),
            // [bucket<4B>]
            bucket_encoded[0],
            bucket_encoded[1],
            bucket_encoded[2],
            bucket_encoded[3],
            // [route<4B>]
            route_encoded[0],
            route_encoded[1],
            route_encoded[2],
            route_encoded[3],
        ]
    }

    fn route_to_compact<'a>(idx: &StoreKeyerIdx<'a>) -> u32 {
        match idx {
            StoreKeyerIdx::MetaToValue(route) => route.as_u32(),
            StoreKeyerIdx::TermToIIDs(route) => *route,
            StoreKeyerIdx::OIDToIID(route) => Self::hash_compact(route),
            StoreKeyerIdx::IIDToOID(route) => *route,
            StoreKeyerIdx::IIDToTerms(route) => *route,
        }
    }

    fn hash_compact(part: &str) -> u32 {
        let mut hasher = XxHash32::with_seed(0);

        hasher.write(part.as_bytes());
        hasher.finish() as u32
    }
}

impl StoreKeyer {
    pub fn as_bytes(&self) -> StoreKeyerKey {
        self.key
    }
}

impl fmt::Display for StoreKeyer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // Convert to number
        let (key_bucket, key_route) = (
            Cursor::new(&self.key[1..5])
                .read_u32::<NativeEndian>()
                .unwrap_or(0),
            Cursor::new(&self.key[5..9])
                .read_u32::<NativeEndian>()
                .unwrap_or(0),
        );

        write!(
            f,
            "'{}:{:x?}:{:x?}' {:?}",
            self.key[0], key_bucket, key_route, self.key
        )
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
