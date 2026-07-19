// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Crisp IM SAS
// License: Mozilla Public License v2.0 (MPL v2.0)

#![deny(
    clippy::all,
    dead_code,
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
)]

pub mod admin;
pub mod config;
pub mod directory;
pub mod error;
pub mod protocol;
pub mod proxy;
pub mod shutdown;
