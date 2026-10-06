// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use hashbrown::HashSet;

use crate::store::{encoding::*, generic::KEY_SEPARATOR};

use super::keys::StoreMetaKey;

pub(super) fn kv_merge_operator(
    key: &[u8],
    existing_val: Option<&[u8]>,
    operands: &rocksdb::MergeOperands,
) -> Option<Vec<u8>> {
    use super::keys::constants::*;

    let prefix_len = key.iter().position(|c| *c == KEY_SEPARATOR)?;

    match key[prefix_len + 1] {
        META_TO_VALUE => match &key[(prefix_len + 2)..] {
            v if v == encode_kv_key_part(StoreMetaKey::IIDIncr.as_u32()) => {
                u64_max(existing_val, operands)
            }
            v if v == encode_kv_key_part(StoreMetaKey::ObjectCount.as_u32()) => {
                i64_counter(existing_val, operands)
            }
            v => panic!("Unrecognized meta key: {v:?}"),
        },
        TERM_TO_IIDS => prepend_int_list::<8>(existing_val, operands),
        IID_TO_TERMS => prepend_int_list::<4>(existing_val, operands),
        _ => unreachable!(),
    }
}

/// This efficiently prepends new integer values to an existing slice, removing
/// duplicates along the way.
fn prepend_int_list<const WORD_LEN: usize>(
    existing_val: Option<&[u8]>,
    operands: &rocksdb::MergeOperands,
) -> Option<Vec<u8>> {
    let current: &[u8] = existing_val.unwrap_or_default();

    let operands_total_len = operands.iter().fold(0, |acc, op| acc + op.len());

    let mut res: Vec<u8> = Vec::with_capacity(current.len() + operands_total_len);

    // PERF: This is just a fancy way to preprend without extra allocation nor
    //   reverse iteration.
    let mut cursor = operands_total_len;
    res.resize(cursor, 0);

    // TODO(perf): We might be able to make this a tiny bit faster by using a
    //   custom hasher that only maps `&[u8]` to a uint. When there is a high
    //   chance that values are close to each other (e.g. for IIDs), we could
    //   use `% capacity` to spread the values better. BENCHMARK THIS ANYWAY!
    let mut seen: HashSet<&[u8]> = HashSet::with_capacity(operands_total_len / WORD_LEN);

    for op in operands {
        for chunk in op.as_chunks::<WORD_LEN>().0 {
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

    for existing in current.as_chunks::<WORD_LEN>().0 {
        // Skip already inserted operands.
        // See reason in <https://github.com/valeriansaliou/sonic/issues/389#issuecomment-5374968203>.
        if !seen.contains(existing as &[u8]) {
            res.extend_from_slice(existing);
        }
    }

    assert!(
        !res.is_empty(),
        "{existing_val:?}, {operands:?}",
        operands = operands.iter().collect::<Vec<_>>()
    );

    Some(res)
}

/// This keeps only the maximum `u64`.
///
/// It’s used for `IIDIncr`, where we can’t guarantee the order in which
/// incremental values will effectively be written.
fn u64_max(existing_val: Option<&[u8]>, operands: &rocksdb::MergeOperands) -> Option<Vec<u8>> {
    const WORD_LEN: usize = 8;

    let mut res = match existing_val {
        Some(bytes) => match bytes.split_first_chunk::<WORD_LEN>() {
            Some((chunk, remainder)) if remainder.is_empty() => decode_u64_counter(*chunk),
            _ => panic!("u64_max: initial value isn’t a u64"),
        },
        None if operands.is_empty() => return None,
        None => 0,
    };

    for op in operands {
        let (chunks, remainder) = op.as_chunks::<WORD_LEN>();

        const ERROR: &str = "u64_max received incorrect operand: remainder not empty.";
        debug_assert!(remainder.is_empty(), "{ERROR}");

        if !remainder.is_empty() {
            tracing::error!("{ERROR} Ignoring.");
            continue;
        }

        for chunk in chunks {
            let new_val = decode_u64_counter(*chunk);

            if new_val > res {
                res = new_val;
            }
        }
    }

    Some(encode_u64_counter(res).to_vec())
}

/// This implements a counter (as `i64`).
///
/// It’s used for `ObjectCount`, where we have to add **and remove** `1`.
///
/// We can’t keep `u64` as value space, because of how operands are merged
/// together. If we used a `u64` accumulator and `i64` operands, the last merge
/// operation would yield incorrect results. On `n` iterations, `existing_val`
/// would be `None` and `operands` filled with `i64` values. Those values would
/// be merged into `0u64` and returned as a `u64` counter. On last iteration,
/// all `n` intermediate counters would be passed as operands, and we’d have no
/// way to know that they’re now encoded as `u64`. In addition, if one merge
/// operation gets `(None, [-1, -1])` and another `(None, [1, 1, 1])`, the
/// final counter value would be `3`; which is incorrect (expected: `1`).
fn i64_counter(existing_val: Option<&[u8]>, operands: &rocksdb::MergeOperands) -> Option<Vec<u8>> {
    const WORD_LEN: usize = 8;

    let mut res = match existing_val {
        Some(bytes) => match bytes.split_first_chunk::<WORD_LEN>() {
            Some((chunk, remainder)) if remainder.is_empty() => decode_i64_counter(*chunk),
            _ => panic!("i64_counter: initial value isn’t a i64"),
        },
        None if operands.is_empty() => return None,
        None => 0,
    };

    for op in operands {
        let (chunks, remainder) = op.as_chunks::<WORD_LEN>();

        const ERROR: &str = "i64_counter received incorrect operand: remainder not empty.";
        debug_assert!(remainder.is_empty(), "{ERROR}");

        if !remainder.is_empty() {
            tracing::error!("{ERROR} Ignoring.");
            continue;
        }

        for chunk in chunks {
            let diff = decode_i64_counter(*chunk);

            res = res.saturating_add(diff);
        }
    }

    Some(encode_i64_counter(res).to_vec())
}
