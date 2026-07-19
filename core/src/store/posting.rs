// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Crisp IM SAS
// License: Mozilla Public License v2.0 (MPL v2.0)

use byteorder::{ByteOrder, LittleEndian};

use super::identifiers::{StoreIIDShard, StoreObjectIID};

pub const POSTING_SHARD_BITS: u32 = 16;
const BITMAP_BYTES: usize = (u16::MAX as usize + 1) / 8;
const SPARSE_TO_DENSE: usize = BITMAP_BYTES / size_of::<u16>();
const DENSE_TO_SPARSE: usize = SPARSE_TO_DENSE / 2;
const FORMAT_SPARSE_RAW: u8 = 0;
const FORMAT_DENSE: u8 = 1;
const FORMAT_SPARSE_DELTA: u8 = 2;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorePosting {
    representation: StorePostingRepresentation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum StorePostingRepresentation {
    Sparse(Vec<u16>),
    Dense(Box<[u8; BITMAP_BYTES]>),
}

impl Default for StorePosting {
    fn default() -> Self {
        Self {
            representation: StorePostingRepresentation::Sparse(Vec::new()),
        }
    }
}

impl StorePosting {
    pub fn shard(iid: StoreObjectIID) -> StoreIIDShard {
        (iid >> POSTING_SHARD_BITS) as StoreIIDShard
    }

    pub fn offset(iid: StoreObjectIID) -> u16 {
        iid as u16
    }

    pub fn iid(shard: StoreIIDShard, offset: u16) -> StoreObjectIID {
        (u32::from(shard) << POSTING_SHARD_BITS) | u32::from(offset)
    }

    pub fn decode(encoded: &[u8]) -> Result<Self, ()> {
        let Some((&format, payload)) = encoded.split_first() else {
            return Err(());
        };

        let representation = match format {
            FORMAT_SPARSE_RAW if payload.len() % size_of::<u16>() == 0 => {
                let mut offsets = Vec::with_capacity(payload.len() / size_of::<u16>());
                for chunk in payload.chunks_exact(size_of::<u16>()) {
                    offsets.push(LittleEndian::read_u16(chunk));
                }
                if offsets.windows(2).any(|pair| pair[0] >= pair[1])
                    || offsets.len() >= SPARSE_TO_DENSE
                {
                    return Err(());
                }
                StorePostingRepresentation::Sparse(offsets)
            }
            FORMAT_SPARSE_DELTA => {
                let mut offsets = Vec::new();
                let mut cursor = 0;
                let mut previous = 0u16;
                while cursor < payload.len() {
                    let mut delta = 0u32;
                    let mut shift = 0;
                    loop {
                        let byte = *payload.get(cursor).ok_or(())?;
                        cursor += 1;
                        delta |= u32::from(byte & 0x7f) << shift;
                        if byte & 0x80 == 0 {
                            break;
                        }
                        shift += 7;
                        if shift > 14 {
                            return Err(());
                        }
                    }
                    let delta = u16::try_from(delta).map_err(|_| ())?;
                    if !offsets.is_empty() && delta == 0 {
                        return Err(());
                    }
                    let offset = if offsets.is_empty() {
                        delta
                    } else {
                        previous.checked_add(delta).ok_or(())?
                    };
                    offsets.push(offset);
                    previous = offset;
                }
                if offsets.len() >= SPARSE_TO_DENSE {
                    return Err(());
                }
                StorePostingRepresentation::Sparse(offsets)
            }
            FORMAT_DENSE if payload.len() == BITMAP_BYTES => {
                let bitmap: Box<[u8; BITMAP_BYTES]> = payload
                    .to_vec()
                    .into_boxed_slice()
                    .try_into()
                    .map_err(|_| ())?;
                StorePostingRepresentation::Dense(bitmap)
            }
            _ => return Err(()),
        };

        Ok(Self { representation })
    }

    pub fn encode(&self) -> Vec<u8> {
        match &self.representation {
            StorePostingRepresentation::Sparse(offsets) => {
                let mut delta_encoded = Vec::with_capacity(1 + offsets.len());
                delta_encoded.push(FORMAT_SPARSE_DELTA);
                let mut previous = 0u16;
                for (index, offset) in offsets.iter().copied().enumerate() {
                    let mut delta = if index == 0 {
                        offset
                    } else {
                        offset - previous
                    };
                    loop {
                        let mut byte = (delta & 0x7f) as u8;
                        delta >>= 7;
                        if delta != 0 {
                            byte |= 0x80;
                        }
                        delta_encoded.push(byte);
                        if delta == 0 {
                            break;
                        }
                    }
                    previous = offset;
                }
                let raw_size = 1 + offsets.len() * size_of::<u16>();
                if delta_encoded.len() < raw_size {
                    delta_encoded
                } else {
                    let mut raw_encoded = Vec::with_capacity(raw_size);
                    raw_encoded.push(FORMAT_SPARSE_RAW);
                    for offset in offsets {
                        raw_encoded.extend_from_slice(&offset.to_le_bytes());
                    }
                    raw_encoded
                }
            }
            StorePostingRepresentation::Dense(bitmap) => {
                let mut encoded = Vec::with_capacity(1 + BITMAP_BYTES);
                encoded.push(FORMAT_DENSE);
                encoded.extend_from_slice(bitmap.as_slice());
                encoded
            }
        }
    }

    pub fn len(&self) -> usize {
        match &self.representation {
            StorePostingRepresentation::Sparse(offsets) => offsets.len(),
            StorePostingRepresentation::Dense(bitmap) => {
                bitmap.iter().map(|byte| byte.count_ones() as usize).sum()
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(crate) fn is_dense(&self) -> bool {
        matches!(self.representation, StorePostingRepresentation::Dense(_))
    }

    pub fn contains(&self, offset: u16) -> bool {
        match &self.representation {
            StorePostingRepresentation::Sparse(offsets) => offsets.binary_search(&offset).is_ok(),
            StorePostingRepresentation::Dense(bitmap) => {
                bitmap[usize::from(offset) / 8] & (1 << (offset % 8)) != 0
            }
        }
    }

    pub fn insert(&mut self, offset: u16) -> bool {
        match &mut self.representation {
            StorePostingRepresentation::Sparse(offsets) => match offsets.binary_search(&offset) {
                Ok(_) => false,
                Err(index) => {
                    offsets.insert(index, offset);
                    if offsets.len() >= SPARSE_TO_DENSE {
                        self.make_dense();
                    }
                    true
                }
            },
            StorePostingRepresentation::Dense(bitmap) => {
                let byte = &mut bitmap[usize::from(offset) / 8];
                let mask = 1 << (offset % 8);
                let inserted = *byte & mask == 0;
                *byte |= mask;
                inserted
            }
        }
    }

    pub fn remove(&mut self, offset: u16) -> bool {
        let removed = match &mut self.representation {
            StorePostingRepresentation::Sparse(offsets) => {
                let Ok(index) = offsets.binary_search(&offset) else {
                    return false;
                };
                offsets.remove(index);
                true
            }
            StorePostingRepresentation::Dense(bitmap) => {
                let byte = &mut bitmap[usize::from(offset) / 8];
                let mask = 1 << (offset % 8);
                let removed = *byte & mask != 0;
                *byte &= !mask;
                removed
            }
        };

        if removed && matches!(self.representation, StorePostingRepresentation::Dense(_)) {
            if self.len() <= DENSE_TO_SPARSE {
                self.make_sparse();
            }
        }
        removed
    }

    pub fn offsets_desc(&self) -> Box<dyn Iterator<Item = u16> + '_> {
        match &self.representation {
            StorePostingRepresentation::Sparse(offsets) => Box::new(offsets.iter().rev().copied()),
            StorePostingRepresentation::Dense(_) => {
                Box::new((0..=u16::MAX).rev().filter(|offset| self.contains(*offset)))
            }
        }
    }

    pub fn intersection_offsets_desc<'a>(
        &'a self,
        other: &'a Self,
    ) -> Box<dyn Iterator<Item = u16> + 'a> {
        let source = if self.len() <= other.len() {
            self
        } else {
            other
        };
        let target = if std::ptr::eq(source, self) {
            other
        } else {
            self
        };
        Box::new(
            source
                .offsets_desc()
                .filter(move |offset| target.contains(*offset)),
        )
    }

    pub(crate) fn union_with(&mut self, other: &Self) {
        for offset in other.offsets_desc() {
            self.insert(offset);
        }
    }

    fn make_dense(&mut self) {
        let StorePostingRepresentation::Sparse(offsets) = &self.representation else {
            return;
        };
        let mut bitmap = Box::new([0; BITMAP_BYTES]);
        for offset in offsets {
            bitmap[usize::from(*offset) / 8] |= 1 << (*offset % 8);
        }
        self.representation = StorePostingRepresentation::Dense(bitmap);
    }

    fn make_sparse(&mut self) {
        let offsets = self.offsets_desc().collect::<Vec<_>>();
        self.representation =
            StorePostingRepresentation::Sparse(offsets.into_iter().rev().collect());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_maps_iids_at_shard_boundaries() {
        assert_eq!(StorePosting::shard(0), 0);
        assert_eq!(StorePosting::offset(0), 0);
        assert_eq!(StorePosting::shard(65_535), 0);
        assert_eq!(StorePosting::offset(65_535), u16::MAX);
        assert_eq!(StorePosting::shard(65_536), 1);
        assert_eq!(StorePosting::offset(65_536), 0);
        assert_eq!(StorePosting::iid(u16::MAX, u16::MAX), u32::MAX);
    }

    #[test]
    fn it_round_trips_sparse_postings_in_descending_order() {
        let mut posting = StorePosting::default();
        assert!(posting.insert(7));
        assert!(posting.insert(1));
        assert!(!posting.insert(7));

        let encoded = posting.encode();
        assert_eq!(encoded, [FORMAT_SPARSE_DELTA, 1, 6]);
        let decoded = StorePosting::decode(&encoded).unwrap();
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded.offsets_desc().collect::<Vec<_>>(), [7, 1]);
        assert!(decoded.contains(1));
    }

    #[test]
    fn it_round_trips_dense_postings_and_shrinks() {
        let mut posting = StorePosting::default();
        for offset in 0..SPARSE_TO_DENSE as u16 {
            assert!(posting.insert(offset));
        }
        assert!(matches!(
            posting.representation,
            StorePostingRepresentation::Dense(_)
        ));

        let mut decoded = StorePosting::decode(&posting.encode()).unwrap();
        assert_eq!(decoded.len(), SPARSE_TO_DENSE);
        for offset in DENSE_TO_SPARSE as u16..SPARSE_TO_DENSE as u16 {
            assert!(decoded.remove(offset));
        }
        assert!(matches!(
            decoded.representation,
            StorePostingRepresentation::Sparse(_)
        ));
        assert_eq!(StorePosting::decode(&decoded.encode()), Ok(decoded));
    }

    #[test]
    fn it_rejects_invalid_sparse_postings() {
        assert!(StorePosting::decode(&[]).is_err());
        assert!(StorePosting::decode(&[FORMAT_SPARSE_RAW, 1]).is_err());
        assert!(StorePosting::decode(&[FORMAT_SPARSE_RAW, 1, 0, 1, 0]).is_err());
        assert!(StorePosting::decode(&[FORMAT_SPARSE_DELTA, 0x80]).is_err());
        assert!(StorePosting::decode(&[FORMAT_SPARSE_DELTA, 0xff, 0xff, 0x7f]).is_err());
    }

    #[test]
    fn it_uses_raw_sparse_encoding_when_smaller() {
        let mut posting = StorePosting::default();
        posting.insert(u16::MAX);
        assert_eq!(posting.encode(), [FORMAT_SPARSE_RAW, 0xff, 0xff]);
    }

    #[test]
    fn it_intersects_postings_in_descending_order() {
        let mut left = StorePosting::default();
        let mut right = StorePosting::default();
        for offset in [1, 7, 9, 20] {
            left.insert(offset);
        }
        for offset in [2, 7, 20, 30] {
            right.insert(offset);
        }
        assert_eq!(
            left.intersection_offsets_desc(&right).collect::<Vec<_>>(),
            [20, 7]
        );
    }

    #[test]
    fn it_unions_postings() {
        let mut left = StorePosting::default();
        let mut right = StorePosting::default();
        for offset in [1, 7, 9] {
            left.insert(offset);
        }
        for offset in [2, 7, 20] {
            right.insert(offset);
        }
        left.union_with(&right);
        assert_eq!(left.offsets_desc().collect::<Vec<_>>(), [20, 9, 7, 2, 1]);
    }

    #[test]
    fn it_unions_into_dense_postings() {
        let mut dense = StorePosting::default();
        for offset in 0..SPARSE_TO_DENSE as u16 {
            dense.insert(offset);
        }
        let mut delta = StorePosting::default();
        delta.insert(10_000);
        dense.union_with(&delta);
        assert!(dense.is_dense());
        assert!(dense.contains(10_000));
        assert_eq!(dense.len(), SPARSE_TO_DENSE + 1);
    }
}
