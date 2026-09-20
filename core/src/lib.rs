// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

#![deny(
    clippy::all,
    dead_code,
    unnameable_types,
    unreachable_pub,
    unstable_features,
    unused_imports,
    unused_qualifications
)]
#![warn(
    clippy::inline_always, // Do not use unless benchmarked (explicit allow).
)]
#![allow(
    clippy::collapsible_if, // Style preference.
    clippy::explicit_auto_deref, // Style preference.
    clippy::needless_as_bytes, // Style preference. Better make those things explicit.
    clippy::needless_borrow, // Style preference.
    clippy::needless_borrows_for_generic_args, // Style preference.
    clippy::result_unit_err, // TODO: Re-enable (deny).
)]

pub mod config;
pub mod executor;
pub mod lexer;
mod stopwords;
pub mod store;
pub mod util;

// Re-export `rocksdb` as it’s part of the public API.
pub use rocksdb;

pub use self::config::Config;
pub use self::executor::DynamicConfig;
pub use self::executor::Executor;
