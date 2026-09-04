// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::store::keyer::StoreKeyerHasher;
use crate::util::hash::NoopU32HasherBuilder;

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
    pub dynamic_conf_store: Arc<DynamicConfigStore>,
}

impl std::fmt::Debug for Executor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // NOTE: Deconstructing to future-proof this function.
        let Self {
            kv_pool,
            fst_pool,
            dynamic_conf_store,
            // NOTE: We don’t care about the app configuration,
            //   we can see it elsewhere if needed.
            app_conf: _app_conf,
        } = self;

        f.debug_struct("Executor")
            .field("kv_pool", kv_pool)
            .field("fst_pool", fst_pool)
            .field("dynamic_conf_store", &dynamic_conf_store)
            .finish_non_exhaustive()
    }
}

/// A wrapper over a `RwLock<HashMap>` that doesn’t leak private types.
#[derive(Default)]
pub struct DynamicConfigStore(RwLock<HashMap<u32, DynamicConfig, NoopU32HasherBuilder>>);

impl DynamicConfigStore {
    pub fn insert(&self, collection: &str, config: DynamicConfig) {
        (self.0.write().unwrap()).insert(StoreKeyerHasher::to_compact(collection), config);
    }

    pub fn get(&self, collection: &str) -> Option<DynamicConfig> {
        (self.0.read().unwrap())
            .get(&StoreKeyerHasher::to_compact(collection))
            .copied()
    }
}

impl std::fmt::Debug for DynamicConfigStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use crate::util::fmt::AsPrettyRwLock;

        f.debug_tuple("DynamicConfStore")
            .field(&AsPrettyRwLock(&self.0))
            .finish()
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DynamicConfig {
    pub rocksdb: DynamicConfigRocksDb,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DynamicConfigRocksDb {
    pub disable_auto_compactions: Option<bool>,
    pub unordered_write: Option<bool>,
    pub memtable: Option<RocksDbMemtable>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RocksDbMemtable {
    Default,
    Vector,
}

impl Executor {
    pub fn set_dynamic_conf(
        &self,
        collection: &str,
        new_conf: DynamicConfig,
    ) -> Result<(), Box<dyn std::error::Error>> {
        tracing::debug!(
            ?new_conf.rocksdb,
            "Re-opening KV store connection for {collection:?} with new dynamic configuration overrides…"
        );

        let mut kv_pool_write_guard = self.kv_pool.pool_write_guard();

        self.kv_pool
            .close(collection, Some(&mut kv_pool_write_guard))
            .map_err(|()| std::io::Error::other("Error closing connection"))?;

        self.kv_pool
            .acquire(
                crate::store::kv::StoreKVAcquireMode::Any,
                collection,
                Some(&mut kv_pool_write_guard),
                |options| {
                    let DynamicConfigRocksDb {
                        disable_auto_compactions,
                        unordered_write,
                        memtable,
                    } = &new_conf.rocksdb;

                    if let Some(disable_auto_compactions) = disable_auto_compactions {
                        options.set_disable_auto_compactions(*disable_auto_compactions);
                    }

                    if let Some(unordered_write) = unordered_write {
                        options.set_unordered_write(*unordered_write);
                    }

                    match memtable {
                        Some(RocksDbMemtable::Vector) => {
                            // Use the vector-based memtable instead of the default skiplist.
                            options.set_memtable_factory(rocksdb::MemtableFactory::Vector);

                            // Vector memtables don't support concurrent inserts, so this must be false.
                            options.set_allow_concurrent_memtable_write(false);
                        }
                        None | Some(RocksDbMemtable::Default) => {}
                    }
                },
            )
            .map_err(|()| std::io::Error::other("Error re-opening connection"))?;

        drop(kv_pool_write_guard);

        tracing::info!(
            ?new_conf.rocksdb,
            "KV store connection for {collection:?} successfully re-opened"
        );

        self.dynamic_conf_store.insert(collection, new_conf);

        Ok(())
    }
}
