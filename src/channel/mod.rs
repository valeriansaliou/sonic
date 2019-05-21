// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

#[macro_use]
mod macros;

mod command;
mod command_pool;
mod format;
mod handle;
mod message;
mod mode;

pub mod listen;
pub mod statistics;
