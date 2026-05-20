// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

#![deny(unstable_features, unused_qualifications, clippy::all)]
#![warn(
    clippy::inline_always, // Do not use unless benchmarked (explicit allow).
    clippy::result_unit_err, // TODO: Re-enable (deny).
    dead_code, // Ideally we’d deny this but at the moment the public API is messy.
)]
#![allow(
    clippy::collapsible_if, // Style preference.
    clippy::explicit_auto_deref, // Style preference.
    clippy::needless_as_bytes, // Style preference. Better make those things explicit.
    clippy::needless_borrow, // Style preference.
    clippy::needless_borrows_for_generic_args, // Style preference.
)]

#[macro_use]
extern crate log;
#[macro_use]
extern crate lazy_static;

pub mod config;
pub mod executor;
mod lexer;
pub mod query;
mod stopwords;
pub mod store;
pub mod util;

pub use self::config::Config;
pub use self::executor::Executor;
pub use self::query::Query;
