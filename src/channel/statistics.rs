// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::ops::Deref;
use std::sync::RwLock;
use std::time::Instant;

use crate::store::fst::StoreFSTPool;
use crate::store::kv::StoreKVPool;

lazy_static! {
    static ref START_TIME: Instant = Instant::now();
    pub static ref CLIENTS_CONNECTED: RwLock<u32> = RwLock::new(0);
    pub static ref COMMANDS_TOTAL: RwLock<u64> = RwLock::new(0);
    pub static ref COMMAND_LATENCY_BEST: RwLock<u32> = RwLock::new(0);
    pub static ref COMMAND_LATENCY_WORST: RwLock<u32> = RwLock::new(0);
}

#[derive(Default)]
pub struct ChannelStatistics {
    pub uptime: u64,
    pub clients_connected: u32,
    pub commands_total: u64,
    pub command_latency_best: u32,
    pub command_latency_worst: u32,
    pub kv_open_count: usize,
    pub fst_open_count: usize,
    pub fst_consolidate_count: usize,
}

pub fn ensure_states() {
    // Ensure all statics are initialized (a `deref` is enough to lazily initialize them)
    let (_, _, _, _, _) = (
        START_TIME.deref(),
        CLIENTS_CONNECTED.deref(),
        COMMANDS_TOTAL.deref(),
        COMMAND_LATENCY_BEST.deref(),
        COMMAND_LATENCY_WORST.deref(),
    );
}

impl ChannelStatistics {
    pub fn gather() -> Result<ChannelStatistics, ()> {
        let (kv_count, fst_count) = (StoreKVPool::count(), StoreFSTPool::count());

        Ok(ChannelStatistics {
            uptime: START_TIME.elapsed().as_secs(),
            clients_connected: *CLIENTS_CONNECTED.read().unwrap(),
            commands_total: *COMMANDS_TOTAL.read().unwrap(),
            command_latency_best: *COMMAND_LATENCY_BEST.read().unwrap(),
            command_latency_worst: *COMMAND_LATENCY_WORST.read().unwrap(),
            kv_open_count: kv_count,
            fst_open_count: fst_count.0,
            fst_consolidate_count: fst_count.1,
        })
    }
}
