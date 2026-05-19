// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod defaults;
mod env_var;

pub mod logger;
pub mod options;
pub mod reader;

pub use self::defaults::defaults;
pub use self::options::*;
