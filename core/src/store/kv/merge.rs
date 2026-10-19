// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use hashbrown::HashSet;

use crate::store::types::*;

use super::encoding::*;
use super::keys::StoreMetaKey;

/// This is an empirical value, which represents the average
/// amount of compression KV merge operations result to.
///
/// Don’t use it; it’s only used by Sonic Server to do some niche experimental
/// computation.
///
/// We just put it here hoping it’ll stay in sync with its related code.
pub const AVERAGE_MERGE_RATIO: u8 = 4;

pub(super) fn kv_merge_operator(
    key: &[u8],
    existing_val: Option<&[u8]>,
    operands: &rocksdb::MergeOperands,
) -> Option<Vec<u8>> {
    use super::keys::KvStoreKeyDiscriminator as D;
    use super::keys::KvStoreKeyParts;

    let KvStoreKeyParts {
        bucket: _,
        discriminator,
        route,
    } = KvStoreKeyParts::try_from(key).ok()?;

    let discriminator = D::try_from(discriminator)
        .inspect_err(|error| tracing::error!("kv_merge_operator: Invalid discriminator: {error}"))
        .ok()?;

    match discriminator {
        D::MetaToValue => match StoreMetaKey::try_from_kv_key_part(route) {
            Ok(StoreMetaKey::IIDIncr) => merge_int_max::<8, IidIncr>(existing_val, operands),
            Ok(StoreMetaKey::ObjectCount) => merge_int_counter::<8, ObjectCount>(
                existing_val,
                operands,
                ObjectCount::saturating_add,
            ),
            Err(error) => panic!("Unrecognized meta key: {error}"),
        },
        D::TermToIids => unique_int_list::<8>(existing_val, operands),
        D::IidToTerms => unique_int_list::<4>(existing_val, operands),
        _ => unreachable!(),
    }
}

/// This efficiently adds new values to an existing slice, removing duplicates
/// along the way.
fn unique_int_list<const WORD_LEN: usize>(
    existing_val: Option<&[u8]>,
    operands: &rocksdb::MergeOperands,
) -> Option<Vec<u8>> {
    let current: &[u8] = existing_val.unwrap_or_default();

    let operands_total_len = operands.iter().fold(0, |acc, op| acc + op.len());

    let mut res: Vec<u8> = Vec::with_capacity(current.len() + operands_total_len);

    // PERF: Assume values are already unique, to `memcpy` only once.
    res.extend_from_slice(current);

    let mut seen: HashSet<&[u8]> = HashSet::with_capacity(operands_total_len / WORD_LEN);

    for chunk in current.as_chunks::<WORD_LEN>().0 {
        seen.insert(chunk);
    }

    for op in operands {
        for chunk in op.as_chunks::<WORD_LEN>().0 {
            // Filter duplicate operands.
            if seen.insert(chunk) {
                res.extend_from_slice(chunk);
            }
        }
    }

    assert!(
        !res.is_empty(),
        "{existing_val:?}, {operands:?}",
        operands = operands.iter().collect::<Vec<_>>()
    );

    Some(res)
}

/// This keeps only the maximum integer.
///
/// It’s used for `IIDIncr`, where we can’t guarantee the order in which
/// incremental values will effectively be written.
fn merge_int_max<const WORD_LEN: usize, T>(
    existing_val: Option<&[u8]>,
    operands: &rocksdb::MergeOperands,
) -> Option<Vec<u8>>
where
    T: ToKvValue<Repr = [u8; WORD_LEN]> + FromKvValue + TryFromKvValue + Default + PartialOrd,
    <T as TryFromKvValue>::Err: std::fmt::Display,
{
    let mut res = match existing_val {
        Some(bytes) => match T::try_from_kv_value(bytes) {
            Ok(initial_value) => initial_value,
            Err(error) => panic!("merge_int_max: Invalid initial value: {error}"),
        },
        None if operands.is_empty() => return None,
        None => T::default(),
    };

    for op in operands {
        let (chunks, remainder) = op.as_chunks();

        const ERROR: &str = "merge_int_max received incorrect operand: Remainder not empty.";
        debug_assert!(remainder.is_empty(), "{ERROR}");

        if !remainder.is_empty() {
            tracing::error!("{ERROR} Ignoring.");
            continue;
        }

        for chunk in chunks {
            let new_val = T::from_kv_value(*chunk);

            if new_val > res {
                res = new_val;
            }
        }
    }

    Some(res.to_kv_value().to_vec())
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
fn merge_int_counter<const WORD_LEN: usize, T>(
    existing_val: Option<&[u8]>,
    operands: &rocksdb::MergeOperands,
    saturating_add: fn(T, T) -> T,
) -> Option<Vec<u8>>
where
    T: ToKvValue<Repr = [u8; WORD_LEN]> + FromKvValue + TryFromKvValue + Default,
    <T as TryFromKvValue>::Err: std::fmt::Display,
{
    let mut res = match existing_val {
        Some(bytes) => match T::try_from_kv_value(bytes) {
            Ok(initial_value) => initial_value,
            Err(error) => panic!("merge_int_counter: Invalid initial value: {error}"),
        },
        None if operands.is_empty() => return None,
        None => T::default(),
    };

    for op in operands {
        let (chunks, remainder) = op.as_chunks();

        const ERROR: &str = "merge_int_counter received incorrect operand: Remainder not empty.";
        debug_assert!(remainder.is_empty(), "{ERROR}");

        if !remainder.is_empty() {
            tracing::error!("{ERROR}");
            return None;
        }

        for chunk in chunks {
            let diff = T::from_kv_value(*chunk);

            res = saturating_add(res, diff);
        }
    }

    Some(res.to_kv_value().to_vec())
}
