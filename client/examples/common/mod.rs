// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

#![allow(dead_code, unused_imports)]

pub mod data;
pub mod logging_transport;
pub mod macros;

pub(crate) use crate::common::macros::{logging, timed};
