// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

//! Utility functions.
//!
//! Most are internal, but some can be public if it makes the library easier to
//! use.

pub(crate) mod fmt;
pub(crate) mod hash;
pub(crate) mod itertools;
pub mod serde;

macro_rules! impl_transparent_wrapper_utils {
    (base for $wrapper:ident$(<$($l1:lifetime),+>)?($(&$l2:lifetime)?$wrapped:ident$(<$($l3:lifetime),+>)?)) => {
        impl$(<$($l1),+>)? std::ops::Deref for $wrapper$(<$($l1),+>)? {
            type Target = $wrapped$(<$($l3),+>)?;

            #[inline(always)]
            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl$(<$($l1),+>)? std::str::FromStr for $wrapper$(<$($l1),+>)?
        where $(&$l2)?$wrapped$(<$($l3),+>)?: std::str::FromStr
        {
            type Err = <$(&$l2)?$wrapped$(<$($l3),+>)? as std::str::FromStr>::Err;

            #[inline(always)]
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                std::str::FromStr::from_str(s).map(Self)
            }
        }
    };

    (Debug for $wrapper:ident$(<$($l1:lifetime),+>)?($(&$l2:lifetime)?$wrapped:ident$(<$($l3:lifetime),+>)?)) => {
        impl$(<$($l1),+>)? std::fmt::Debug for $wrapper$(<$($l1),+>)?
        where $(&$l2)?$wrapped$(<$($l3),+>)?: std::fmt::Debug
        {
            #[inline(always)]
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                std::fmt::Debug::fmt(&self.0, f)
            }
        }
    };

    (Display for $wrapper:ident$(<$($l1:lifetime),+>)?($(&$l2:lifetime)?$wrapped:ident$(<$($l3:lifetime),+>)?)) => {
        impl$(<$($l1),+>)? std::fmt::Display for $wrapper$(<$($l1),+>)?
        where $(&$l2)?$wrapped$(<$($l3),+>)?: std::fmt::Display
        {
            #[inline(always)]
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                std::fmt::Display::fmt(&self.0, f)
            }
        }
    };
}
pub(crate) use impl_transparent_wrapper_utils;
