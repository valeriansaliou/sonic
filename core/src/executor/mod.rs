// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::sync::Arc;

#[macro_use]
mod macros;

mod count;
mod flushb;
mod flushc;
mod flusho;
mod list;
mod pop;
mod push;
mod search;
mod suggest;

pub struct Executor {
    pub app_conf: Arc<crate::Config>,
    pub kv_pool: crate::store::kv::StoreKVPool,
    pub fst_pool: crate::store::fst::StoreFSTPool,
}

impl std::fmt::Debug for Executor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // NOTE: Deconstructing to future-proof this function.
        let Self {
            kv_pool,
            fst_pool,
            // NOTE: We don’t care about the app configuration,
            //   we can see it elsewhere if needed.
            app_conf: _app_conf,
        } = self;

        f.debug_struct("Executor")
            .field("kv_pool", kv_pool)
            .field("fst_pool", fst_pool)
            .finish_non_exhaustive()
    }
}
