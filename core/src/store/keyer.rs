// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use byteorder::{BigEndian, ByteOrder};
use std::fmt;
use std::hash::Hasher;
use std::mem::size_of;
use std::str;
use twox_hash::XxHash32;

use super::identifiers::*;

pub struct StoreKeyerBuilder;

pub struct StoreKeyer {
    key: StoreKeyerKey,
}

pub struct StoreKeyerHasher;

pub type StoreKeyerKey = Vec<u8>;
pub type StoreKeyerPrefix = Vec<u8>;

const IDX_META_TO_VALUE: u8 = 0;
const IDX_TERM_POSTING: u8 = 1;
const IDX_OID_TO_IID: u8 = 2;
const IDX_IID_TO_OID: u8 = 3;
const IDX_IID_TO_TERMS: u8 = 4;
const IDX_BUCKET_NAME_TO_ID: u8 = 5;
const IDX_BUCKET_ID_TO_NAME: u8 = 6;
const IDX_IID_TO_TIMESTAMP: u8 = 7;
const IDX_TIME_POSTING: u8 = 8;
const IDX_TERM_FREQUENCY: u8 = 9;

impl StoreKeyerBuilder {
    pub fn family(key: &[u8]) -> Option<u8> {
        key.get(size_of::<StoreBucketID>()).copied()
    }

    pub fn decode_u32_ordered(encoded: &[u8]) -> Option<u32> {
        encoded.try_into().ok().map(u32::from_be_bytes)
    }

    pub fn meta_to_value(bucket_id: StoreBucketID, meta: &StoreMetaKey) -> StoreKeyer {
        Self::fixed(IDX_META_TO_VALUE, bucket_id, meta.as_u32())
    }

    pub fn term_posting(
        bucket_id: StoreBucketID,
        term: &str,
        iid_shard: StoreIIDShard,
    ) -> StoreKeyer {
        let mut key = Self::term_posting_prefix(bucket_id, term);
        key.extend_from_slice(&Self::encode_u16_ordered(iid_shard));
        StoreKeyer { key }
    }

    pub fn term_posting_prefix(bucket_id: StoreBucketID, term: &str) -> StoreKeyerPrefix {
        Self::term_key(IDX_TERM_POSTING, bucket_id, term).key
    }

    pub fn term_posting_family_prefix(bucket_id: StoreBucketID) -> StoreKeyerPrefix {
        Self::bucket_prefix(IDX_TERM_POSTING, bucket_id)
    }

    pub fn term_frequency(bucket_id: StoreBucketID, term: &str) -> StoreKeyer {
        Self::term_key(IDX_TERM_FREQUENCY, bucket_id, term)
    }

    pub fn decode_term_route_with_suffix(route: &[u8], suffix_len: usize) -> Option<&str> {
        let length = route
            .get(..4)
            .and_then(Self::decode_u32_ordered)
            .and_then(|length| usize::try_from(length).ok())?;
        let term = route.get(4..4 + length)?;
        (route.len() == 4 + length + suffix_len)
            .then(|| str::from_utf8(term).ok())
            .flatten()
    }

    pub fn iid_to_timestamp(bucket_id: StoreBucketID, iid: StoreObjectIID) -> StoreKeyer {
        Self::fixed(IDX_IID_TO_TIMESTAMP, bucket_id, iid)
    }

    pub fn time_posting(
        bucket_id: StoreBucketID,
        time_slice: u64,
        iid_shard: StoreIIDShard,
    ) -> StoreKeyer {
        let mut key = Self::bucket_prefix(IDX_TIME_POSTING, bucket_id);
        key.extend_from_slice(&time_slice.to_be_bytes());
        key.extend_from_slice(&Self::encode_u16_ordered(iid_shard));
        StoreKeyer { key }
    }

    pub fn time_posting_prefix(bucket_id: StoreBucketID) -> StoreKeyerPrefix {
        Self::bucket_prefix(IDX_TIME_POSTING, bucket_id)
    }

    pub fn document(bucket_id: StoreBucketID, iid: StoreObjectIID) -> StoreKeyer {
        let mut key = Self::document_prefix(bucket_id);
        key.extend_from_slice(&Self::encode_u32_ordered(iid));
        StoreKeyer { key }
    }

    pub fn document_prefix(bucket_id: StoreBucketID) -> StoreKeyerPrefix {
        Self::encode_u32_ordered(bucket_id).to_vec()
    }

    pub fn oid_to_iid(bucket_id: StoreBucketID, oid: StoreObjectOID<'_>) -> StoreKeyer {
        Self::variable(IDX_OID_TO_IID, bucket_id, oid.as_bytes())
    }

    pub fn iid_to_oid(bucket_id: StoreBucketID, iid: StoreObjectIID) -> StoreKeyer {
        Self::fixed(IDX_IID_TO_OID, bucket_id, iid)
    }

    pub fn iid_to_terms(bucket_id: StoreBucketID, iid: StoreObjectIID) -> StoreKeyer {
        Self::fixed(IDX_IID_TO_TERMS, bucket_id, iid)
    }

    pub fn bucket_name_to_id(bucket: &str) -> StoreKeyer {
        let mut key = Self::bucket_name_prefix();
        key.extend_from_slice(bucket.as_bytes());
        StoreKeyer { key }
    }

    pub fn bucket_name_prefix() -> StoreKeyerPrefix {
        Self::bucket_prefix(IDX_BUCKET_NAME_TO_ID, 0)
    }

    pub fn bucket_id_to_name(bucket_id: StoreBucketID) -> StoreKeyer {
        Self::fixed(IDX_BUCKET_ID_TO_NAME, 0, bucket_id)
    }

    pub fn bucket_prefix(index: u8, bucket_id: StoreBucketID) -> StoreKeyerPrefix {
        let mut prefix = Vec::with_capacity(5);
        prefix.extend_from_slice(&Self::encode_u32_ordered(bucket_id));
        prefix.push(index);
        prefix
    }

    pub fn bucket_indexes() -> &'static [u8] {
        &[
            IDX_META_TO_VALUE,
            IDX_TERM_POSTING,
            IDX_OID_TO_IID,
            IDX_IID_TO_OID,
            IDX_IID_TO_TERMS,
            IDX_IID_TO_TIMESTAMP,
            IDX_TIME_POSTING,
            IDX_TERM_FREQUENCY,
        ]
    }

    pub fn posting_indexes() -> &'static [u8] {
        &[IDX_TERM_POSTING, IDX_TIME_POSTING]
    }

    fn fixed(index: u8, bucket_id: StoreBucketID, route: u32) -> StoreKeyer {
        let mut key = Self::bucket_prefix(index, bucket_id);
        key.extend_from_slice(&Self::encode_u32_ordered(route));
        StoreKeyer { key }
    }

    fn variable(index: u8, bucket_id: StoreBucketID, route: &[u8]) -> StoreKeyer {
        let mut key = Self::bucket_prefix(index, bucket_id);
        key.extend_from_slice(route);
        StoreKeyer { key }
    }

    fn term_key(index: u8, bucket_id: StoreBucketID, term: &str) -> StoreKeyer {
        let mut key = Self::bucket_prefix(index, bucket_id);
        key.extend_from_slice(&Self::encode_u32_ordered(term.len() as u32));
        key.extend_from_slice(term.as_bytes());
        StoreKeyer { key }
    }

    fn encode_u32_ordered(value: u32) -> [u8; 4] {
        let mut encoded = [0; 4];
        BigEndian::write_u32(&mut encoded, value);
        encoded
    }

    fn encode_u16_ordered(value: u16) -> [u8; 2] {
        let mut encoded = [0; 2];
        BigEndian::write_u16(&mut encoded, value);
        encoded
    }
}

impl StoreKeyer {
    pub fn as_bytes(&self) -> &[u8] {
        &self.key
    }
}

impl StoreKeyerHasher {
    #![allow(clippy::wrong_self_convention)]
    pub fn to_compact(part: &str) -> u32 {
        let mut hasher = XxHash32::with_seed(0);
        hasher.write(part.as_bytes());
        hasher.finish() as u32
    }
}

impl fmt::Display for StoreKeyer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_keys_fixed_routes() {
        assert_eq!(
            StoreKeyerBuilder::term_posting(42, "hello", 9).as_bytes(),
            b"\0\0\0\x2a\x01\0\0\0\x05hello\0\x09"
        );
        assert_eq!(
            StoreKeyerBuilder::term_posting_prefix(42, "hello"),
            b"\0\0\0\x2a\x01\0\0\0\x05hello"
        );
        assert_eq!(
            StoreKeyerBuilder::term_frequency(42, "hello").as_bytes(),
            b"\0\0\0\x2a\x09\0\0\0\x05hello"
        );
        assert_eq!(
            StoreKeyerBuilder::iid_to_oid(42, 9).as_bytes(),
            &[0, 0, 0, 42, 3, 0, 0, 0, 9]
        );
        assert_eq!(
            StoreKeyerBuilder::time_posting(42, 7, 9).as_bytes(),
            &[0, 0, 0, 42, 8, 0, 0, 0, 0, 0, 0, 0, 7, 0, 9]
        );
        assert_eq!(
            StoreKeyerBuilder::document(42, 9).as_bytes(),
            &[0, 0, 0, 42, 0, 0, 0, 9]
        );
    }

    #[test]
    fn it_keeps_names_in_dictionary_keys() {
        assert_eq!(
            StoreKeyerBuilder::bucket_name_to_id("bucket:1").as_bytes(),
            b"\0\0\0\0\x05bucket:1"
        );
        assert_eq!(
            StoreKeyerBuilder::oid_to_iid(4, "message:123").as_bytes(),
            b"\0\0\0\x04\x02message:123"
        );
    }

    #[test]
    fn it_orders_numeric_key_parts_and_groups_buckets() {
        assert!(
            StoreKeyerBuilder::document(1, 256).as_bytes()
                < StoreKeyerBuilder::document(2, 1).as_bytes()
        );
        assert!(
            StoreKeyerBuilder::document(2, 255).as_bytes()
                < StoreKeyerBuilder::document(2, 256).as_bytes()
        );
        assert_eq!(
            StoreKeyerBuilder::family(StoreKeyerBuilder::term_posting(2, "hello", 0).as_bytes()),
            Some(IDX_TERM_POSTING)
        );
        assert_eq!(
            StoreKeyerBuilder::decode_u32_ordered(&[0, 0, 1, 0]),
            Some(256)
        );
    }

    #[test]
    fn it_hashes_collection_names() {
        assert_eq!(StoreKeyerHasher::to_compact("key:1"), 3370353088);
        assert_eq!(StoreKeyerHasher::to_compact("key:2"), 1042559698);
    }
}
