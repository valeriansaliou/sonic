// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use super::*;

#[inline]
pub(super) const fn encode_kv_key_part(value: u32) -> [u8; 4] {
    value.to_be_bytes()
}

#[inline]
pub(super) const fn decode_kv_key_part(bytes: [u8; 4]) -> u32 {
    u32::from_be_bytes(bytes)
}

#[inline]
pub(super) const fn encode_kv_key_part_long(value: u64) -> [u8; 8] {
    value.to_be_bytes()
}

#[inline]
pub(super) const fn decode_kv_key_part_long(bytes: [u8; 8]) -> u64 {
    u64::from_be_bytes(bytes)
}

#[inline]
pub(super) const fn encode_i64_counter(value: i64) -> [u8; 8] {
    value.to_le_bytes()
}

#[inline]
pub(super) const fn decode_i64_counter(bytes: [u8; 8]) -> i64 {
    i64::from_le_bytes(bytes)
}

#[inline]
pub(super) const fn encode_u64_counter(value: u64) -> [u8; 8] {
    value.to_le_bytes()
}

#[inline]
pub(super) const fn decode_u64_counter(bytes: [u8; 8]) -> u64 {
    u64::from_le_bytes(bytes)
}

#[inline]
pub(super) const fn encode_iid(value: StoreObjectIid) -> [u8; 8] {
    value.into_inner().to_le_bytes()
}

#[inline]
pub(super) const fn decode_iid(bytes: [u8; 8]) -> StoreObjectIid {
    StoreObjectIid::new(u64::from_le_bytes(bytes))
}

#[inline]
pub(super) const fn try_decode_iid(bytes: &[u8]) -> Result<StoreObjectIid, ()> {
    if bytes.len() == 8 {
        // SAFETY: `bytes` is guaranteed to be 8 bytes long.
        Ok(StoreObjectIid::new(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ])))
    } else {
        Err(())
    }
}

#[inline]
pub(crate) const fn encode_term_hash_key(value: &StoreTermHash) -> [u8; 4] {
    value.into_inner().to_le_bytes()
}

#[allow(dead_code)]
#[inline]
pub(super) const fn decode_term_hash_key(bytes: [u8; 4]) -> StoreTermHash {
    StoreTermHash::new(u32::from_le_bytes(bytes))
}

#[inline]
pub(crate) const fn encode_term_hash_value(value: &StoreTermHash) -> [u8; 4] {
    value.into_inner().to_le_bytes()
}

#[inline]
pub(super) const fn decode_term_hash_value(bytes: [u8; 4]) -> StoreTermHash {
    StoreTermHash::new(u32::from_le_bytes(bytes))
}

macro_rules! impl_encode_decode_list_mapped {
    ($encode_fn:ident / $decode_fn:ident, $chunk:ident($word_len:expr) -> $to_array:expr) => {
        fn $encode_fn<T>(
            decoded: impl ExactSizeIterator<Item = T>,
            encode: fn(T) -> [u8; $word_len],
        ) -> Vec<u8> {
            // Pre-reserve required capacity as to avoid heap resizes (50%
            // performance gain relative to initializing this with no capacity).
            let mut encoded = Vec::with_capacity(decoded.len() * $word_len);

            for decoded_item in decoded {
                encoded.extend(&encode(decoded_item))
            }

            encoded
        }

        fn $decode_fn<T>(encoded: &[u8], decode: fn([u8; $word_len]) -> T) -> Result<Vec<T>, ()> {
            // Pre-reserve required capacity as to avoid heap resizes (50%
            // performance gain relative to initializing this with no capacity).
            let mut decoded = Vec::with_capacity(encoded.len() / $word_len);

            for $chunk in encoded.chunks($word_len) {
                let decoded_chunk = decode($to_array);
                decoded.push(decoded_chunk);
            }

            Ok(decoded)
        }
    };
}

impl_encode_decode_list_mapped!(
    encode_u32_list_mapped / decode_u32_list_mapped,
    // SAFETY: `chunk` is guaranteed to be 4 bytes long.
    chunk(4) -> [chunk[0], chunk[1], chunk[2], chunk[3]]
);
impl_encode_decode_list_mapped!(
    encode_u64_list_mapped / decode_u64_list_mapped,
    // SAFETY: `chunk` is guaranteed to be 8 bytes long.
    chunk(8) -> [chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7]]
);

#[inline]
pub(super) fn encode_terms_list<'t>(
    terms: impl ExactSizeIterator<Item = &'t StoreTermHash>,
) -> Vec<u8> {
    encode_u32_list_mapped(terms, encode_term_hash_value)
}

#[inline]
pub(super) fn decode_terms_list(encoded: &[u8]) -> Result<Vec<StoreTermHash>, ()> {
    decode_u32_list_mapped(encoded, decode_term_hash_value)
}

#[inline]
pub(super) fn encode_iids_list(iids: impl ExactSizeIterator<Item = StoreObjectIid>) -> Vec<u8> {
    encode_u64_list_mapped(iids, encode_iid)
}

#[inline]
pub(super) fn decode_iids_list(encoded: &[u8]) -> Result<Vec<StoreObjectIid>, ()> {
    decode_u64_list_mapped(encoded, decode_iid)
}
