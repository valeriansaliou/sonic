// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::hash::Hash;

use hashbrown::HashSet;
use indexmap::IndexMap;
use zerocopy::little_endian;

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

pub(crate) const TF_TOMBSTONE: u8 = 0;

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
        D::TermToIids => merge_int_set_diffs::<TermToIidsEntry>(existing_val, operands.iter()),
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

trait Mergeable {
    type Key: Eq + Hash;
    type Value: Ord;

    fn merge_in(&self, map: &mut IndexMap<Self::Key, Self::Value>);

    fn cmp(
        k1: &Self::Key,
        v1: &Self::Value,
        k2: &Self::Key,
        v2: &Self::Value,
    ) -> std::cmp::Ordering;

    fn should_keep(&self) -> bool;

    fn as_bytes(&self) -> &[u8];
}

/// This efficiently adds **and removes** new values to an existing slice,
/// removing duplicates along the way.
fn merge_int_set_diffs<'op, T>(
    existing_val: Option<&[u8]>,
    operands: impl Iterator<Item = &'op [u8]>,
) -> Option<Vec<u8>>
where
    T: Mergeable,
    T: Sized + zerocopy::FromBytes + zerocopy::Immutable,
    T: From<(T::Key, T::Value)>,
{
    use zerocopy::FromBytes as _;

    // SAFETY: Diffs are encoded and decoded by `zerocopy`, which ensures layout stays coherent.

    let current: &[u8] = existing_val.unwrap_or_default();
    let current_states: &[T] = <[T]>::ref_from_bytes(current).unwrap();

    // NOTE: Chances are very high the final set will be at least `current_states.len()`
    //   long (unless most diffs are deletions). `IndexMap::with_capacity` is $O(n)$ and
    //   a lot of entries might get de-duplicated so we better not pre-allocate that much.
    let mut map: IndexMap<T::Key, T::Value> = IndexMap::with_capacity(current_states.len());

    for state in current_states {
        state.merge_in(&mut map);
    }

    for op in operands {
        let diffs: &[T] = <[T]>::ref_from_bytes(op).unwrap();

        for diff in diffs {
            diff.merge_in(&mut map);
        }
    }

    // NOTE: We reverse the sort here then reverse the iterator later so we can
    //   have a stable sort that gives higher precedence to latest values.
    map.sort_by(|k1, v1, k2, v2| T::cmp(k1, v1, k2, v2).reverse());

    let mut res = Vec::with_capacity(map.len());

    for diff in map.into_iter().rev().map(T::from).filter(T::should_keep) {
        res.extend_from_slice(diff.as_bytes());
    }

    Some(res)
}

#[cfg(test)]
#[test]
fn test_merge_int_set_diffs() {
    use zerocopy::IntoBytes as _;

    let existing = [
        TermToIidsEntry::new(0.into(), 11),
        TermToIidsEntry::new(1.into(), 12),
        TermToIidsEntry::new(2.into(), 13),
    ];

    let op1 = [
        TermToIidsEntry::new(0.into(), 3),            // PUSH again on IID 0
        TermToIidsEntry::new(3.into(), 14),           // PUSH IID 3 (new)
        TermToIidsEntry::new(2.into(), TF_TOMBSTONE), // FLUSHO IID 2
    ];
    let op2 = [
        TermToIidsEntry::new(0.into(), 4),            // PUSH again on IID 0
        TermToIidsEntry::new(1.into(), TF_TOMBSTONE), // FLUSHO IID 1
        TermToIidsEntry::new(2.into(), 2),            // PUSH again on IID 2 (re-index)
        TermToIidsEntry::new(0.into(), 5),            // PUSH again on IID 0
        TermToIidsEntry::new(3.into(), 9),            // PUSH again on IID 3
    ];

    let res = merge_int_set_diffs::<TermToIidsEntry>(
        Some(existing.as_bytes()),
        [op1.as_bytes(), op2.as_bytes()].into_iter(),
    )
    .unwrap();

    let expected = [
        // IID 1 should be gone
        TermToIidsEntry::new(3.into(), 23), // IID 3 should be the first (highest tf + most recent)
        TermToIidsEntry::new(0.into(), 23),
        TermToIidsEntry::new(2.into(), 2), // IID 2 should be last (lowest tf)
    ];

    assert_eq!(res, expected.as_bytes());
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

// MARK: Term-To-IIDs

// WARN: It’s important to use an encoding that’s compatible no matter the
//   system endianness!
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[derive(
    zerocopy::FromBytes,
    zerocopy::IntoBytes,
    zerocopy::Immutable,
    zerocopy::Unaligned
)]
#[repr(C, packed)]
pub struct TermToIidsEntry {
    iid: little_endian::U64,
    // NOTE: It’s okay to use a u8 here, as BM25 saturation causes tf values > 256
    //   to have almost the same score. Saving one byte here is a huge win in terms
    //   of index size so it’s a worth it trade-off.
    tf: u8,
}

impl Mergeable for TermToIidsEntry {
    type Key = little_endian::U64;
    type Value = u8;

    fn merge_in(&self, map: &mut IndexMap<Self::Key, Self::Value>) {
        map.entry(self.iid)
            .and_modify(|acc| {
                if self.tf == TF_TOMBSTONE {
                    *acc = TF_TOMBSTONE;
                } else {
                    *acc = acc.saturating_add(self.tf)
                }
            })
            .or_insert(self.tf);
    }

    fn cmp(
        _k1: &Self::Key,
        v1: &Self::Value,
        _k2: &Self::Key,
        v2: &Self::Value,
    ) -> std::cmp::Ordering {
        v1.cmp(v2).reverse()
    }

    #[inline]
    fn should_keep(&self) -> bool {
        // NOTE: This is where tombstones are taken into account.
        self.tf != TF_TOMBSTONE
    }

    #[inline]
    fn as_bytes(&self) -> &[u8] {
        zerocopy::IntoBytes::as_bytes(self)
    }
}

impl TermToIidsEntry {
    #[inline]
    pub const fn new(iid: StoreObjectIid, tf: u8) -> Self {
        Self {
            iid: little_endian::U64::new(iid.into_inner()),
            tf,
        }
    }

    #[inline]
    pub const fn iid(&self) -> StoreObjectIid {
        StoreObjectIid::new(self.iid.get())
    }

    #[inline]
    pub const fn tf(&self) -> u8 {
        self.tf
    }
}

impl From<(little_endian::U64, u8)> for TermToIidsEntry {
    #[inline]
    fn from((iid, tf): (little_endian::U64, u8)) -> Self {
        Self { iid, tf }
    }
}

// MARK: IID-To-Terms

// WARN: It’s important to use an encoding that’s compatible no matter the
//   system endianness!
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[derive(
    zerocopy::FromBytes,
    zerocopy::IntoBytes,
    zerocopy::Immutable,
    zerocopy::Unaligned
)]
#[repr(C, packed)]
pub struct IidToTermsEntry {
    term_hash: little_endian::U32,
}

impl IidToTermsEntry {
    #[inline]
    pub const fn new(term_hash: StoreTermHash) -> Self {
        Self {
            term_hash: little_endian::U32::new(term_hash.into_inner()),
        }
    }

    #[inline]
    pub const fn into_term_hash(self) -> StoreTermHash {
        let Self { term_hash, .. } = self;
        StoreTermHash::new(term_hash.get())
    }

    #[inline]
    pub const fn term_hash(&self) -> StoreTermHash {
        StoreTermHash::new(self.term_hash.get())
    }
}
