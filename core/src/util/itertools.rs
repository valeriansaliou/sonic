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
