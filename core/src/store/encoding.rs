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
pub(super) const fn encode_i32_counter(value: i32) -> [u8; 4] {
    value.to_le_bytes()
}

#[inline]
pub(super) const fn decode_i32_counter(bytes: [u8; 4]) -> i32 {
    i32::from_le_bytes(bytes)
}

#[inline]
pub(super) const fn encode_u32_counter(value: u32) -> [u8; 4] {
    value.to_le_bytes()
}

#[inline]
pub(super) const fn decode_u32_counter(bytes: [u8; 4]) -> u32 {
    u32::from_le_bytes(bytes)
}

#[inline]
pub(super) const fn encode_iid(value: StoreObjectIid) -> [u8; 4] {
    value.into_inner().to_le_bytes()
}

#[inline]
pub(super) const fn decode_iid(bytes: [u8; 4]) -> StoreObjectIid {
    StoreObjectIid::new(u32::from_le_bytes(bytes))
}

#[inline]
pub(super) const fn try_decode_iid(bytes: &[u8]) -> Result<StoreObjectIid, ()> {
    if bytes.len() == 4 {
        // SAFETY: `bytes` is guaranteed to be 4 bytes long.
        Ok(StoreObjectIid::new(u32::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3],
        ])))
    } else {
        Err(())
    }
}

#[inline]
pub(super) const fn encode_term_hash(value: StoreTermHash) -> [u8; 4] {
    value.into_inner().to_le_bytes()
}

#[inline]
pub(super) const fn decode_term_hash(bytes: [u8; 4]) -> StoreTermHash {
    StoreTermHash::new(u32::from_le_bytes(bytes))
}

fn encode_u32_list_mapped<T>(
    decoded: impl ExactSizeIterator<Item = T>,
    encode: fn(T) -> [u8; 4],
) -> Vec<u8> {
    // Pre-reserve required capacity as to avoid heap resizes (50%
    // performance gain relative to initializing this with no capacity).
    let mut encoded = Vec::with_capacity(decoded.len() * 4);

    for decoded_item in decoded {
        encoded.extend(&encode(decoded_item))
    }

    encoded
}

fn decode_u32_list_mapped<T>(encoded: &[u8], decode: fn([u8; 4]) -> T) -> Result<Vec<T>, ()> {
    // Pre-reserve required capacity as to avoid heap resizes (50%
    // performance gain relative to initializing this with no capacity).
    let mut decoded = Vec::with_capacity(encoded.len() / 4);

    for chunk in encoded.chunks(4) {
        // SAFETY: `chunk` is guaranteed to be 4 bytes long.
        let decoded_chunk = decode([chunk[0], chunk[1], chunk[2], chunk[3]]);
        decoded.push(decoded_chunk);
    }

    Ok(decoded)
}

#[inline]
pub(super) fn encode_terms_list(terms: impl ExactSizeIterator<Item = StoreTermHash>) -> Vec<u8> {
    encode_u32_list_mapped(terms, encode_term_hash)
}

#[inline]
pub(super) fn decode_terms_list(encoded: &[u8]) -> Result<Vec<StoreTermHash>, ()> {
    decode_u32_list_mapped(encoded, decode_term_hash)
}

#[inline]
pub(super) fn encode_iids_list(iids: impl ExactSizeIterator<Item = StoreObjectIid>) -> Vec<u8> {
    encode_u32_list_mapped(iids, encode_iid)
}

#[inline]
pub(super) fn decode_iids_list(encoded: &[u8]) -> Result<Vec<StoreObjectIid>, ()> {
    decode_u32_list_mapped(encoded, decode_iid)
}
