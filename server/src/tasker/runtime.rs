// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use sonic::executor::DynamicConfigStore;
use sonic::store::fst::FstStorePool;
use sonic::store::kv::KvStorePool;
use sonic::store::object::ObjectStorePool;

#[derive(Clone)]
pub struct TaskerBuilder {
    pub kv_pool: KvStorePool,
    pub fst_pool: FstStorePool,
    pub object_store_pool: ObjectStorePool,
    pub dynamic_conf_store: Arc<DynamicConfigStore>,
}

pub struct Tasker {
    kv_pool: KvStorePool,
    fst_pool: FstStorePool,
    object_store_pool: ObjectStorePool,
    dynamic_conf_store: Arc<DynamicConfigStore>,
}

const TASKER_TICK_INTERVAL: Duration = Duration::from_secs(10);

impl TaskerBuilder {
    pub fn build(&self) -> Tasker {
        Tasker {
            kv_pool: self.kv_pool.clone(),
            fst_pool: self.fst_pool.clone(),
            object_store_pool: self.object_store_pool.clone(),
            dynamic_conf_store: Arc::clone(&self.dynamic_conf_store),
        }
    }
}

impl Tasker {
    pub fn run(&self) {
        tracing::info!("tasker is now active");

        loop {
            // Hold for next aggregate run
            thread::sleep(TASKER_TICK_INTERVAL);

            tracing::debug!("running a tasker tick...");

            let tick_start = Instant::now();

            self.tick();

            let tick_took = tick_start.elapsed();

            tracing::info!(
                "ran tasker tick (took {}s + {}ms)",
                tick_took.as_secs(),
                tick_took.subsec_millis()
            );
        }
    }

    /// Proceed all tick actions
    fn tick(&self) {
        const TASK_DISABLED: &str = "Disabled per dynamic configuration";

        let dynamic_conf_store_read_guard = self.dynamic_conf_store.read();

        // #1: Janitors
        {
            let disabled = dynamic_conf_store_read_guard
                .iter()
                .filter_map(|(&collection, dynamic_conf)| {
                    if dynamic_conf.sonic.disable_janitor_tasks.unwrap_or(false) {
                        Some(collection)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();

            self.kv_pool.janitor(|store_id| {
                let skip = disabled.contains(store_id.as_collection_hash());
                if skip {
                    tracing::info!(
                        "Not running janitor task for KV store {store_id:?}: {TASK_DISABLED}"
                    );
                }
                !skip
            });
            self.fst_pool.janitor(|store_id| {
                let skip = disabled.contains(store_id.as_collection_hash());
                if skip {
                    tracing::info!(
                        "Not running janitor task for FST store {store_id:?}: {TASK_DISABLED}"
                    );
                }
                !skip
            });
            self.object_store_pool.janitor(|store_id| {
                let skip = disabled.contains(store_id.as_collection_hash());
                if skip {
                    tracing::info!(
                        "Not running janitor task for Object store {store_id:?}: {TASK_DISABLED}"
                    );
                }
                !skip
            });
        }

        // #2: Others
        self.kv_pool.flush(false, |store_id| {
            let skip = dynamic_conf_store_read_guard
                .get(store_id.as_collection_hash())
                .and_then(|conf| conf.sonic.disable_kv_flush_task)
                .unwrap_or(false);
            if skip {
                tracing::info!("Not running flush task for KV store {store_id:?}: {TASK_DISABLED}");
            }
            !skip
        });
        self.fst_pool.consolidate(false, |store_id| {
            let skip = dynamic_conf_store_read_guard
                .get(store_id.as_collection_hash())
                .and_then(|conf| conf.sonic.disable_fst_consolidate_task)
                .unwrap_or(false);
            if skip {
                tracing::info!(
                    "Not running consolidate task for FST store {store_id:?}: {TASK_DISABLED}"
                );
            }
            !skip
        });
        self.object_store_pool.flush(false, |store_id| {
            let skip = dynamic_conf_store_read_guard
                .get(store_id.as_collection_hash())
                .and_then(|conf| conf.sonic.disable_kv_flush_task)
                .unwrap_or(false);
            if skip {
                tracing::info!(
                    "Not running flush task for Object store {store_id:?}: {TASK_DISABLED}"
                );
            }
            !skip
        });
    }
}
