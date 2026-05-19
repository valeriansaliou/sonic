// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::sync::Arc;

#[macro_use]
mod macros;

pub mod count;
pub mod flushb;
pub mod flushc;
pub mod flusho;
pub mod list;
pub mod pop;
pub mod push;
pub mod search;
pub mod suggest;

pub struct Executor {
    pub app_conf: Arc<crate::Config>,
    pub kv_pool: crate::store::kv::StoreKVPool,
    pub fst_pool: crate::store::fst::StoreFSTPool,
}
