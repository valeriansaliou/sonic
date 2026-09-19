// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use hashbrown::HashSet;

use crate::store::*;

use super::keys::StoreMetaKey;

impl StoreObjectIID {
    #[inline]
    pub(super) const fn into_bytes(self) -> [u8; 4] {
        encode_u32(self.into_inner())
    }
}

impl StoreTermHash {
    #[inline]
    pub(super) const fn into_bytes(self) -> [u8; 4] {
        encode_u32(self.into_inner())
    }
}

#[inline]
const fn encode_u32(decoded: u32) -> [u8; 4] {
    decoded.to_le_bytes()
}

#[inline]
pub(super) fn decode_u32_mapped<T: From<u32>>(encoded: &[u8]) -> Result<T, ()> {
    decode_u32(encoded).map(T::from)
}

const fn decode_u32(encoded: &[u8]) -> Result<u32, ()> {
    if encoded.len() == 4 {
        Ok(u32::from_le_bytes([
            encoded[0], encoded[1], encoded[2], encoded[3],
        ]))
    } else {
        Err(())
    }
}

pub(super) fn encode_u32_list_mapped<T: Into<u32>>(
    decoded: impl ExactSizeIterator<Item = T>,
) -> Vec<u8> {
    // Pre-reserve required capacity as to avoid heap resizes (50%
    // performance gain relative to initializing this with a zero-capacity)
    let mut encoded = Vec::with_capacity(decoded.len() * 4);

    for decoded_item in decoded {
        encoded.extend(&encode_u32(decoded_item.into()))
    }

    encoded
}

pub(super) fn decode_u32_list_mapped<T: From<u32>>(encoded: &[u8]) -> Result<Vec<T>, ()> {
    // Pre-reserve required capacity as to avoid heap resizes (50%
    // performance gain relative to initializing this with a zero-capacity)
    let mut decoded = Vec::with_capacity(encoded.len() / 4);

    for encoded_chunk in encoded.chunks(4) {
        match decode_u32(encoded_chunk) {
            Ok(decoded_chunk) => {
                decoded.push(T::from(decoded_chunk));
            }
            Err(_err) => return Err(()),
        }
    }

    Ok(decoded)
}

pub(super) fn default_merge_operator(
    key: &[u8],
    existing_val: Option<&[u8]>,
    operands: &rocksdb::MergeOperands,
) -> Option<Vec<u8>> {
    use super::keys::constants::*;

    match key[0] {
        META_TO_VALUE if key[5..9] == encode_u32(StoreMetaKey::IIDIncr.as_u32()) => {
            u32_max(existing_val, operands)
        }
        TERM_TO_IIDS | IID_TO_TERMS => {
            // eprintln!(
            //     "prepend_u32_list({}): {}/{}",
            //     &key[0],
            //     existing_val.map_or(0, <[u8]>::len),
            //     operands.len()
            // );
            prepend_u32_list(existing_val, operands)
        }
        _ => unreachable!(),
    }
}

/// This efficiently prepends new u32 values to an existing slice, removing
/// duplicates along the way.
fn prepend_u32_list(
    existing_val: Option<&[u8]>,
    operands: &rocksdb::MergeOperands,
) -> Option<Vec<u8>> {
    const WORD_LEN: usize = 4;

    let current: &[u8] = existing_val.unwrap_or_default();

    let operands_total_len = operands.iter().fold(0, |acc, op| acc + op.len());

    let mut res: Vec<u8> = Vec::with_capacity(current.len() + operands_total_len);

    // PERF: This is just a fancy way to preprend without extra allocation nor
    //   reverse iteration.
    let mut cursor = operands_total_len;
    res.extend_from_slice(vec![0; cursor].as_slice());

    // TODO(perf): We might be able to make this a tiny bit faster by using a
    //   custom hasher that only maps `&[u8]` to a `u32`. When there is a high
    //   chance that values are close to each other (e.g. for IIDs), we could
    //   use `% capacity` to spread the values better. BENCHMARK THIS ANYWAY!
    let mut seen: HashSet<&[u8]> = HashSet::with_capacity(operands_total_len / WORD_LEN);

    for op in operands {
        for chunk in op.chunks(WORD_LEN) {
            // Filter duplicate operands.
            // NOTE: In benchmarks, `operands` showed a length of `13761` for
            //   example, so we _have_ to keep this at most `O(n*log(n))`!
            if seen.insert(chunk) {
                let start = cursor.checked_sub(WORD_LEN).unwrap();
                res[start..cursor].copy_from_slice(chunk);
                cursor = start;
            }
        }
    }

    // Trim unused bytes at the start (because of duplicate operands).
    res = res.split_off(cursor);

    for existing in current.chunks(WORD_LEN) {
        // Skip already inserted operands.
        // See reason in <https://github.com/valeriansaliou/sonic/issues/389#issuecomment-5374968203>.
        if !seen.contains(existing) {
            res.extend_from_slice(existing);
        }
    }

    assert!(!res.is_empty());

    Some(res)
}

/// This keeps only the maximum u32.
///
/// It’s used for `IIDIncr`, where we can’t guarantee the order in which
/// incremental values will effectively be written.
fn u32_max(existing_val: Option<&[u8]>, operands: &rocksdb::MergeOperands) -> Option<Vec<u8>> {
    let mut res = match existing_val {
        Some(bytes) if bytes.len() == 4 => {
            // SAFETY: `bytes` is guaranteed to be 4 bytes long.
            decode_u32(bytes).unwrap()
        }
        Some(_) => panic!("u32_max: initial value isn’t a u32"),
        None if operands.is_empty() => return None,
        None => 0,
    };

    for op in operands {
        for chunk in op.chunks(4) {
            // SAFETY: `chunk` is guaranteed to be 4 bytes long.
            let new_val = decode_u32(chunk).unwrap();

            if res > new_val {
                res = new_val;
            }
        }
    }

    Some(encode_u32(res).to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_encodes_atom() {
        assert_eq!(encode_u32(0), [0, 0, 0, 0]);
        assert_eq!(encode_u32(1), [1, 0, 0, 0]);
        assert_eq!(encode_u32(45402), [90, 177, 0, 0]);
    }

    #[test]
    fn it_decodes_atom() {
        assert_eq!(decode_u32(&[0, 0, 0, 0]), Ok(0));
        assert_eq!(decode_u32(&[1, 0, 0, 0]), Ok(1));
        assert_eq!(decode_u32(&[90, 177, 0, 0]), Ok(45402));
    }

    #[test]
    fn it_encodes_atom_list() {
        assert_eq!(
            encode_u32_list([0, 2, 3].into_iter()),
            [0, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0]
        );
        assert_eq!(encode_u32_list([45402].into_iter()), [90, 177, 0, 0]);
    }

    #[test]
    fn it_decodes_atom_list() {
        assert_eq!(
            decode_u32_list(&[0, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0]),
            Ok(vec![0, 2, 3])
        );
        assert_eq!(decode_u32_list(&[90, 177, 0, 0]), Ok(vec![45402]));
    }

    // MARK: Helpers

    #[inline(always)]
    fn encode_u32_list(decoded: impl ExactSizeIterator<Item = u32>) -> Vec<u8> {
        encode_u32_list_mapped(decoded)
    }

    #[inline(always)]
    fn decode_u32_list(encoded: &[u8]) -> Result<Vec<u32>, ()> {
        decode_u32_list_mapped(encoded)
    }
}

#[cfg(all(feature = "benchmark", test))]
mod benches {
    extern crate test;

    use super::*;
    use test::Bencher;

    #[bench]
    fn bench_encode_atom(b: &mut Bencher) {
        b.iter(|| encode_u32(0));
    }

    #[bench]
    fn bench_decode_atom(b: &mut Bencher) {
        let encoded_atom = [0, 0, 0, 0];

        b.iter(|| decode_u32(&encoded_atom));
    }

    #[bench]
    fn bench_encode_atom_list(b: &mut Bencher) {
        let atom_list = [0, 2, 3];

        b.iter(|| encode_u32_list(&atom_list));
    }

    #[bench]
    fn bench_decode_atom_list(b: &mut Bencher) {
        let encoded_atom_list = [0, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0];

        b.iter(|| decode_u32_list(&encoded_atom_list));
    }
}
