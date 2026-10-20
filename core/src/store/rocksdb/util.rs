// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// Copyright: 2026, Claude Sonnet 5.5
// License: Mozilla Public License v2.0 (MPL v2.0)

use rocksdb::DBPinnableSlice;
use zerocopy::{FromBytes, Immutable};

#[allow(dead_code)]
#[derive(Clone, Copy)]
enum Direction {
    Forward,
    Reverse,
}

#[derive(Debug, thiserror::Error)]
#[error("Too many bytes")]
pub(crate) struct BadLength(#[source] std::io::Error);

pub(crate) struct RocksDbPinnableSliceIter<'db, T> {
    // Owns the pinned memory. Never read directly after construction.
    _pinned: DBPinnableSlice<'db>,
    ptr: *const T,
    len: usize,   // total number of elements, validated in `new`
    front: usize, // next index from the front
    back: usize,  // one past the next index from the back
    dir: Direction,
}

impl<'db, T> RocksDbPinnableSliceIter<'db, T>
where
    T: FromBytes + Immutable + Copy,
{
    pub(crate) fn forward(pinned: DBPinnableSlice<'db>) -> Result<Self, BadLength> {
        Self::new(pinned, Direction::Forward)
    }

    #[allow(dead_code)]
    pub(crate) fn reverse(pinned: DBPinnableSlice<'db>) -> Result<Self, BadLength> {
        Self::new(pinned, Direction::Reverse)
    }

    fn new(pinned: DBPinnableSlice<'db>, dir: Direction) -> Result<Self, BadLength> {
        let slice = <[T]>::ref_from_bytes(&pinned)
            .map_err(|e| BadLength(std::io::Error::other(e.to_string())))?;
        let (ptr, len) = (slice.as_ptr(), slice.len());
        Ok(Self {
            _pinned: pinned,
            ptr,
            len,
            front: 0,
            back: len,
            dir,
        })
    }

    #[inline]
    fn take_front(&mut self) -> Option<T> {
        if self.front == self.back {
            return None;
        }
        let i = self.front;
        self.front += 1;
        Some(self.read(i))
    }

    #[inline]
    fn take_back(&mut self) -> Option<T> {
        if self.front == self.back {
            return None;
        }
        self.back -= 1;
        Some(self.read(self.back))
    }

    #[inline]
    fn read(&self, i: usize) -> T {
        debug_assert!(i < self.len);
        // SAFETY: `i < self.len`, the length validated in `new`; `ptr` points into
        // memory owned by `_pinned` (alive as long as `self`); and `ref_from_bytes`
        // already checked size and alignment for `T`, with `T: FromBytes + Copy`
        // guaranteeing any bit pattern is a valid value.
        unsafe { *self.ptr.add(i) }
    }
}

impl<T> Iterator for RocksDbPinnableSliceIter<'_, T>
where
    T: FromBytes + Immutable + Copy,
{
    type Item = T;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        match self.dir {
            Direction::Forward => self.take_front(),
            Direction::Reverse => self.take_back(),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.back - self.front;
        (n, Some(n))
    }
}

impl<T> DoubleEndedIterator for RocksDbPinnableSliceIter<'_, T>
where
    T: FromBytes + Immutable + Copy,
{
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        match self.dir {
            Direction::Forward => self.take_back(),
            Direction::Reverse => self.take_front(),
        }
    }
}

impl<T> ExactSizeIterator for RocksDbPinnableSliceIter<'_, T> where T: FromBytes + Immutable + Copy {}
