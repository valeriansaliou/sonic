// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use hashbrown::HashSet;

use crate::store::encoding::*;

use super::keys::StoreMetaKey;

pub(super) fn kv_merge_operator(
    key: &[u8],
    existing_val: Option<&[u8]>,
    operands: &rocksdb::MergeOperands,
) -> Option<Vec<u8>> {
    use super::keys::constants::*;

    match key[key.len() - 5] {
        META_TO_VALUE => match &key[(key.len() - 4)..] {
            v if v == encode_kv_key_part(StoreMetaKey::IIDIncr.as_u32()) => {
                u32_max(existing_val, operands)
            }
            v if v == encode_kv_key_part(StoreMetaKey::ObjectCount.as_u32()) => {
                i32_counter(existing_val, operands)
            }
            v => panic!("Unrecognized meta key: {v:?}"),
        },
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

/// This keeps only the maximum `u32`.
///
/// It’s used for `IIDIncr`, where we can’t guarantee the order in which
/// incremental values will effectively be written.
fn u32_max(existing_val: Option<&[u8]>, operands: &rocksdb::MergeOperands) -> Option<Vec<u8>> {
    let mut res = match existing_val {
        Some(bytes) if bytes.len() == 4 => {
            // SAFETY: `bytes` is guaranteed to be 4 bytes long.
            decode_u32_counter([bytes[0], bytes[1], bytes[2], bytes[3]])
        }
        Some(_) => panic!("u32_max: initial value isn’t a u32"),
        None if operands.is_empty() => return None,
        None => 0,
    };

    for op in operands {
        for chunk in op.chunks(4) {
            // SAFETY: `chunk` is guaranteed to be 4 bytes long.
            let new_val = decode_u32_counter([chunk[0], chunk[1], chunk[2], chunk[3]]);

            if new_val > res {
                res = new_val;
            }
        }
    }

    Some(encode_u32_counter(res).to_vec())
}

/// This implements a counter (as `i32`).
///
/// It’s used for `ObjectCount`, where we have to add **and remove** `1`.
///
/// We can’t keep `u32` as value space, because of how operands are merged
/// together. If we used a `u32` accumulator and `i32` operands, the last merge
/// operation would yield incorrect results. On `n` iterations, `existing_val`
/// would be `None` and `operands` filled with `i32` values. Those values would
/// be merged into `0u32` and returned as a `u32` counter. On last iteration,
/// all `n` intermediate counters would be passed as operands, and we’d have no
/// way to know that they’re now encoded as `u32`. In addition, if one merge
/// operation gets `(None, [-1, -1])` and another `(None, [1, 1, 1])`, the
/// final counter value would be `3`; which is incorrect (expected: `1`).
fn i32_counter(existing_val: Option<&[u8]>, operands: &rocksdb::MergeOperands) -> Option<Vec<u8>> {
    let mut res = match existing_val {
        Some(bytes) if bytes.len() == 4 => {
            // SAFETY: `bytes` is guaranteed to be 4 bytes long.
            decode_i32_counter([bytes[0], bytes[1], bytes[2], bytes[3]])
        }
        Some(_) => panic!("i32_counter: initial value isn’t a u32"),
        None if operands.is_empty() => return None,
        None => 0,
    };

    for op in operands {
        for chunk in op.chunks(4) {
            // SAFETY: `chunk` is guaranteed to be 4 bytes long.
            let diff = decode_i32_counter([chunk[0], chunk[1], chunk[2], chunk[3]]);

            res = res.saturating_add(diff);
        }
    }

    Some(encode_i32_counter(res).to_vec())
}
