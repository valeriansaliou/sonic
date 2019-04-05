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

pub type StoreKeyerKey = [u8; 9];
pub type StoreKeyerPrefix = [u8; 5];

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

    pub fn term_to_iids(bucket: &str, term_hash: StoreTermHashed) -> StoreKeyer {
        Self::make(StoreKeyerIdx::TermToIIDs(term_hash), bucket)
    }

    pub fn oid_to_iid<'a>(bucket: &'a str, oid: StoreObjectOID<'a>) -> StoreKeyer {
        Self::make(StoreKeyerIdx::OIDToIID(oid), bucket)
    }

    pub fn iid_to_oid(bucket: &str, iid: StoreObjectIID) -> StoreKeyer {
        Self::make(StoreKeyerIdx::IIDToOID(iid), bucket)
    }

    pub fn iid_to_terms(bucket: &str, iid: StoreObjectIID) -> StoreKeyer {
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

        LittleEndian::write_u32(&mut bucket_encoded, StoreKeyerHasher::to_compact(bucket));
        LittleEndian::write_u32(&mut route_encoded, Self::route_to_compact(&idx));

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

    fn route_to_compact(idx: &StoreKeyerIdx) -> u32 {
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

    pub fn as_prefix(&self) -> StoreKeyerPrefix {
        // Prefix format: [idx<1B> | bucket<4B>]

        [
            self.key[0],
            self.key[1],
            self.key[2],
            self.key[3],
            self.key[4],
        ]
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
        let (key_bucket, key_route) = (
            Cursor::new(&self.key[1..5])
                .read_u32::<LittleEndian>()
                .unwrap_or(0),
            Cursor::new(&self.key[5..9])
                .read_u32::<LittleEndian>()
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
            StoreKeyerBuilder::meta_to_value("bucket:1", &StoreMetaKey::IIDIncr).as_bytes(),
            [0, 108, 244, 29, 93, 0, 0, 0, 0]
        );
    }

    #[test]
    fn it_keys_term_to_iids() {
        assert_eq!(
            StoreKeyerBuilder::term_to_iids("bucket:2", 772137347).as_bytes(),
            [1, 50, 220, 166, 65, 131, 225, 5, 46]
        );
        assert_eq!(
            StoreKeyerBuilder::term_to_iids("bucket:2", 3582484684).as_bytes(),
            [1, 50, 220, 166, 65, 204, 96, 136, 213]
        );
    }

    #[test]
    fn it_keys_oid_to_iid() {
        assert_eq!(
            StoreKeyerBuilder::oid_to_iid("bucket:3", &"conversation:6501e83a".to_string())
                .as_bytes(),
            [2, 171, 194, 213, 57, 31, 156, 118, 213]
        );
    }

    #[test]
    fn it_keys_iid_to_oid() {
        assert_eq!(
            StoreKeyerBuilder::iid_to_oid("bucket:4", 10292198).as_bytes(),
            [3, 105, 12, 54, 147, 230, 11, 157, 0]
        );
    }

    #[test]
    fn it_keys_iid_to_terms() {
        assert_eq!(
            StoreKeyerBuilder::iid_to_terms("bucket:5", 1).as_bytes(),
            [4, 137, 142, 73, 67, 1, 0, 0, 0]
        );
        assert_eq!(
            StoreKeyerBuilder::iid_to_terms("bucket:5", 20).as_bytes(),
            [4, 137, 142, 73, 67, 20, 0, 0, 0]
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
            &format!("{}", StoreKeyerBuilder::term_to_iids("bucket:6", 72137347)),
            "'1:71198b49:44cba83' [1, 73, 139, 25, 113, 131, 186, 76, 4]"
        );
        assert_eq!(
            &format!(
                "{}",
                StoreKeyerBuilder::meta_to_value("bucket:6", &StoreMetaKey::IIDIncr)
            ),
            "'0:71198b49:0' [0, 73, 139, 25, 113, 0, 0, 0, 0]"
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
        b.iter(|| StoreKeyerBuilder::meta_to_value("bucket:bench:1", &StoreMetaKey::IIDIncr));
    }

    #[bench]
    fn bench_key_term_to_iids(b: &mut Bencher) {
        b.iter(|| StoreKeyerBuilder::term_to_iids("bucket:bench:2", 772137347));
    }

    #[bench]
    fn bench_key_oid_to_iid(b: &mut Bencher) {
        let key = "conversation:6501e83a".to_string();

        b.iter(|| StoreKeyerBuilder::oid_to_iid("bucket:bench:3", &key));
    }

    #[bench]
    fn bench_key_iid_to_oid(b: &mut Bencher) {
        b.iter(|| StoreKeyerBuilder::iid_to_oid("bucket:bench:4", 10292198));
    }

    #[bench]
    fn bench_key_iid_to_terms(b: &mut Bencher) {
        b.iter(|| StoreKeyerBuilder::iid_to_terms("bucket:bench:5", 1));
    }
}
