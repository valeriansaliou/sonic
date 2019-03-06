// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::thread;
use std::time::{Duration, Instant};

use crate::store::kv::StoreKVPool;

pub struct JanitorBuilder;
pub struct Janitor;

const JANITOR_TICK_INTERVAL: Duration = Duration::from_secs(5);

impl JanitorBuilder {
    pub fn new() -> Janitor {
        Janitor {}
    }
}

impl Janitor {
    pub fn run(&self) {
        info!("janitor is now active");

        loop {
            // Hold for next aggregate run
            thread::sleep(JANITOR_TICK_INTERVAL);

            debug!("running a janitor tick...");

            let tick_start = Instant::now();

            Self::tick();

            let tick_took = tick_start.elapsed();

            info!(
                "ran janitor tick (took {}s + {}ns)",
                tick_took.as_secs(),
                tick_took.subsec_nanos()
            );
        }
    }

    fn tick() {
        // Proceed all tick actions
        StoreKVPool::janitor();
    }
}
