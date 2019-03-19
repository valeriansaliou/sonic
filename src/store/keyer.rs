// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use byteorder::{ByteOrder, LittleEndian, ReadBytesExt};
use std::fmt;
use std::hash::Hasher;
use std::io::Cursor;
use twox_hash::XxHash32;

use super::identifiers::*;

pub struct StoreKeyerBuilder;

pub struct StoreKeyer {
    key: StoreKeyerKey,
}

pub struct StoreKeyerHasher;

enum StoreKeyerIdx<'a> {
    MetaToValue(&'a StoreMetaKey),
    TermToIIDs(StoreTermHashed),
    OIDToIID(StoreObjectOID<'a>),
    IIDToOID(StoreObjectIID),
    IIDToTerms(StoreObjectIID),
}

type StoreKeyerKey = [u8; 5];

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
    pub fn meta_to_value<'a>(meta: &'a StoreMetaKey) -> StoreKeyer {
        Self::make(StoreKeyerIdx::MetaToValue(meta))
    }

    pub fn term_to_iids<'a>(term_hash: StoreTermHashed) -> StoreKeyer {
        Self::make(StoreKeyerIdx::TermToIIDs(term_hash))
    }

    pub fn oid_to_iid<'a>(oid: StoreObjectOID<'a>) -> StoreKeyer {
        Self::make(StoreKeyerIdx::OIDToIID(oid))
    }

    pub fn iid_to_oid<'a>(iid: StoreObjectIID) -> StoreKeyer {
        Self::make(StoreKeyerIdx::IIDToOID(iid))
    }

    pub fn iid_to_terms<'a>(iid: StoreObjectIID) -> StoreKeyer {
        Self::make(StoreKeyerIdx::IIDToTerms(iid))
    }

    fn make<'a>(idx: StoreKeyerIdx<'a>) -> StoreKeyer {
        StoreKeyer {
            key: Self::build_key(idx),
        }
    }

    fn build_key<'a>(idx: StoreKeyerIdx<'a>) -> StoreKeyerKey {
        // Key format: [idx<1B> | route<4B>]

        // Encode key route from u32 to array of u8 (ie. binary)
        let mut route_encoded = [0; 4];

        LittleEndian::write_u32(&mut route_encoded, Self::route_to_compact(&idx));

        // Generate final binary key
        [
            // [idx<1B>]
            idx.to_index(),
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
            StoreKeyerIdx::OIDToIID(route) => StoreKeyerHasher::to_compact(route),
            StoreKeyerIdx::IIDToOID(route) => *route,
            StoreKeyerIdx::IIDToTerms(route) => *route,
        }
    }
}

impl StoreKeyer {
    pub fn as_bytes(&self) -> StoreKeyerKey {
        self.key
    }
}

impl StoreKeyerHasher {
    pub fn to_compact(part: &str) -> u32 {
        let mut hasher = XxHash32::with_seed(0);

        hasher.write(part.as_bytes());
        hasher.finish() as u32
    }
}

impl fmt::Display for StoreKeyer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // Convert to number
        let key_route = Cursor::new(&self.key[1..5])
            .read_u32::<LittleEndian>()
            .unwrap_or(0);

        write!(f, "'{}:{:x?}' {:?}", self.key[0], key_route, self.key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_keys_meta_to_value() {
        assert_eq!(
            StoreKeyerBuilder::meta_to_value(&StoreMetaKey::IIDIncr).as_bytes(),
            [0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn it_keys_term_to_iids() {
        assert_eq!(
            StoreKeyerBuilder::term_to_iids(772137347).as_bytes(),
            [1, 131, 225, 5, 46]
        );
        assert_eq!(
            StoreKeyerBuilder::term_to_iids(3582484684).as_bytes(),
            [1, 204, 96, 136, 213]
        );
    }

    #[test]
    fn it_keys_oid_to_iid() {
        assert_eq!(
            StoreKeyerBuilder::oid_to_iid(&"conversation:6501e83a".to_string()).as_bytes(),
            [2, 31, 156, 118, 213]
        );
    }

    #[test]
    fn it_keys_iid_to_oid() {
        assert_eq!(
            StoreKeyerBuilder::iid_to_oid(10292198).as_bytes(),
            [3, 230, 11, 157, 0]
        );
    }

    #[test]
    fn it_keys_iid_to_terms() {
        assert_eq!(
            StoreKeyerBuilder::iid_to_terms(1).as_bytes(),
            [4, 1, 0, 0, 0]
        );
        assert_eq!(
            StoreKeyerBuilder::iid_to_terms(20).as_bytes(),
            [4, 20, 0, 0, 0]
        );
    }

    #[test]
    fn it_hashes_compact() {
        assert_eq!(StoreKeyerHasher::to_compact("key:1"), 3370353088);
        assert_eq!(StoreKeyerHasher::to_compact("key:2"), 1042559698);
    }

    #[test]
    fn it_formats_key() {
        assert_eq!(
            &format!("{}", StoreKeyerBuilder::term_to_iids(772137347)),
            "'1:2e05e183' [1, 131, 225, 5, 46]"
        );
        assert_eq!(
            &format!(
                "{}",
                StoreKeyerBuilder::meta_to_value(&StoreMetaKey::IIDIncr)
            ),
            "'0:0' [0, 0, 0, 0, 0]"
        );
    }
}

#[cfg(all(feature = "benchmark", test))]
mod benches {
    extern crate test;

    use super::*;
    use test::Bencher;

    #[bench]
    fn bench_hash_compact_short(b: &mut Bencher) {
        b.iter(|| StoreKeyerHasher::to_compact("key:bench:1"));
    }

    #[bench]
    fn bench_hash_compact_long(b: &mut Bencher) {
        b.iter(|| {
            StoreKeyerHasher::to_compact(
                "key:bench:2:long:long:long:long:long:long:long:long:long:long:long:long:long:long",
            )
        });
    }

    #[bench]
    fn bench_key_meta_to_value(b: &mut Bencher) {
        b.iter(|| StoreKeyerBuilder::meta_to_value(&StoreMetaKey::IIDIncr));
    }

    #[bench]
    fn bench_key_term_to_iids(b: &mut Bencher) {
        b.iter(|| StoreKeyerBuilder::term_to_iids(772137347));
    }

    #[bench]
    fn bench_key_oid_to_iid(b: &mut Bencher) {
        let key = "conversation:6501e83a".to_string();

        b.iter(|| StoreKeyerBuilder::oid_to_iid(&key));
    }

    #[bench]
    fn bench_key_iid_to_oid(b: &mut Bencher) {
        b.iter(|| StoreKeyerBuilder::iid_to_oid(10292198));
    }

    #[bench]
    fn bench_key_iid_to_terms(b: &mut Bencher) {
        b.iter(|| StoreKeyerBuilder::iid_to_terms(1));
    }
}
