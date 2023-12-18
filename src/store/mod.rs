// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

#[macro_use]
mod macros;

mod generic;
mod keyer;

pub mod fst;
pub mod identifiers;
pub mod item;
pub mod operation;

#[cfg(not(feature = "redb"))]
pub mod kv;
#[cfg(feature = "redb")]
#[path ="kv_redb.rs"]
pub mod kv;