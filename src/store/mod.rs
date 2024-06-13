// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

#[cfg(all(feature = "redb", feature = "rocksdb"))]
compile_error!("Features `redb` and `rocksdb` are mutually exclusive and cannot be enabled together.");

#[cfg(not(any(feature = "redb", feature = "rocksdb")))]
compile_error!("Features `redb` or `rocksdb` should be enabled at least one.");

#[cfg(feature = "rocksdb")]
pub mod kv;
#[cfg(feature = "redb")]
#[path ="kv_redb.rs"]
pub mod kv;

#[macro_use]
mod macros;

mod generic;
mod keyer;

pub mod fst;
pub mod identifiers;
pub mod item;
pub mod operation;


