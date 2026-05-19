// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::thread;
use std::time::{Duration, Instant};

use crate::store::fst::StoreFSTPool;
use crate::store::kv::StoreKVPool;

#[derive(Clone)]
pub struct TaskerBuilder {
    pub kv_pool: StoreKVPool,
    pub fst_pool: StoreFSTPool,
}

pub struct Tasker {
    kv_pool: StoreKVPool,
    fst_pool: StoreFSTPool,
}

const TASKER_TICK_INTERVAL: Duration = Duration::from_secs(10);

impl TaskerBuilder {
    pub fn build(&self) -> Tasker {
        Tasker {
            kv_pool: self.kv_pool.clone(),
            fst_pool: self.fst_pool.clone(),
        }
    }
}

impl Tasker {
    pub fn run(&self) {
        info!("tasker is now active");

        loop {
            // Hold for next aggregate run
            thread::sleep(TASKER_TICK_INTERVAL);

            debug!("running a tasker tick...");

            let tick_start = Instant::now();

            self.tick();

            let tick_took = tick_start.elapsed();

            info!(
                "ran tasker tick (took {}s + {}ms)",
                tick_took.as_secs(),
                tick_took.subsec_millis()
            );
        }
    }

    fn tick(&self) {
        // Proceed all tick actions

        // #1: Janitors
        self.kv_pool.janitor();
        self.fst_pool.janitor();

        // #2: Others
        self.kv_pool.flush(false);
        self.fst_pool.consolidate(false);
    }
}
