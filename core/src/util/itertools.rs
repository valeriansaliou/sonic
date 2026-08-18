// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

//! Subset of [`itertools`](https://docs.rs/itertools/0.14.0/itertools/index.html),
//! avoiding an additional dependency.

/// Inspired by <https://docs.rs/itertools/0.14.0/itertools/trait.Itertools.html>.
pub trait Itertools: Iterator {
    /// Inspired by <https://docs.rs/itertools/0.14.0/src/itertools/lib.rs.html#2413-2442>.
    fn join(&mut self, sep: &str) -> String
    where
        Self::Item: std::fmt::Display,
    {
        use std::fmt::Write as _;

        match self.next() {
            None => String::new(),
            Some(first_elt) => {
                // estimate lower bound of capacity needed
                let (lower, _) = self.size_hint();
                let mut result = String::with_capacity(sep.len() * lower);
                write!(&mut result, "{}", first_elt).unwrap();
                self.for_each(|elt| {
                    result.push_str(sep);
                    write!(&mut result, "{}", elt).unwrap();
                });
                result
            }
        }
    }
}

impl<T> Itertools for T where T: Iterator + ?Sized {}

pub struct Prepend<T, I: Iterator<Item = T> + ?Sized> {
    head: Option<T>,
    tail: I,
}

impl<T, I: Iterator<Item = T>> Iterator for Prepend<T, I> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.head.take().or_else(|| self.tail.next())
    }
}

impl<T, I: ExactSizeIterator<Item = T>> ExactSizeIterator for Prepend<T, I> {
    fn len(&self) -> usize {
        // SAFETY: This might overflow, but it’s just `1` so the chances are
        //   low. And this `Prepend` is intended to be used with an
        //   `ExactSizeIterator` (otherwise just use `Iterator::chain`), and
        //   we have a safety check in the constructor.
        self.tail.len() + 1
    }
}

pub trait ExactSizeIteratorExt<T>: ExactSizeIterator<Item = T> {
    fn prepend(self, head: T) -> Prepend<T, Self>;
}

impl<T, I: ExactSizeIterator<Item = T>> ExactSizeIteratorExt<T> for I {
    fn prepend(self, head: T) -> Prepend<T, I> {
        assert!(
            self.len() < usize::MAX,
            "iterator should have at least one slot available for the prefix"
        );

        Prepend {
            head: Some(head),
            tail: self,
        }
    }
}
