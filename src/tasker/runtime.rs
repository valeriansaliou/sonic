// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::thread;
use std::time::{Duration, Instant};

use crate::store::fst::StoreFSTPool;
use crate::store::kv::StoreKVPool;

pub struct TaskerBuilder;
pub struct Tasker;

const TASKER_TICK_INTERVAL: Duration = Duration::from_secs(30);

impl TaskerBuilder {
    pub fn new() -> Tasker {
        Tasker {}
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

            Self::tick();

            let tick_took = tick_start.elapsed();

            info!(
                "ran tasker tick (took {}s + {}ms)",
                tick_took.as_secs(),
                tick_took.subsec_millis()
            );
        }
    }

    fn tick() {
        // Proceed all tick actions

        // #1: Janitors
        StoreKVPool::janitor();
        StoreFSTPool::janitor();

        // #2: Others
        StoreFSTPool::consolidate();
    }
}
