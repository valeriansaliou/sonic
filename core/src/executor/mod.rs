// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::collections::HashMap;
use std::sync::{Arc, RwLock, RwLockReadGuard};

use crate::store::kv::KvStoreId;
use crate::store::{StoreItemPart, StoreObjectIid};
use crate::util::hash::NoopU32HasherBuilder;

#[macro_use]
mod macros;
mod types;

mod count;
mod flushb;
mod flushc;
mod flusho;
mod list;
mod pop;
mod push;
mod search;
mod suggest;

pub use types::*;

pub struct Executor {
    pub app_conf: Arc<crate::Config>,
    pub kv_pool: crate::store::kv::KvStorePool,
    pub fst_pool: crate::store::fst::FstStorePool,
    pub dynamic_conf_store: Arc<DynamicConfigStore>,

    /// When using `NEW` with `PUSH`, a new IID is automatically created.
    /// However, if the input data is larger than the allowed buffer size Sonic
    /// would end up indexing the same document across multiple IIDs
    /// (see [issue #405 “Experimental flag `NEW` is incompatible with content > `buffer_size`”](https://github.com/valeriansaliou/sonic/issues/405)).
    ///
    /// To fix it, we keep track of the last “assumed new” OID and its IID so
    /// we can reuse it on subsequent `PUSH … NEW` requests.
    // NOTE: We can’t use `StoreObjectOid` as it’d not owned.
    last_assumed_new_oid: RwLock<Option<(String, StoreObjectIid)>>,
}

impl Executor {
    pub fn new(
        app_conf: Arc<crate::Config>,
        kv_pool: crate::store::kv::KvStorePool,
        fst_pool: crate::store::fst::FstStorePool,
        dynamic_conf_store: Arc<DynamicConfigStore>,
    ) -> Self {
        Self {
            app_conf,
            kv_pool,
            fst_pool,
            dynamic_conf_store,
            last_assumed_new_oid: RwLock::new(None),
        }
    }
}

impl std::fmt::Debug for Executor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // NOTE: Deconstructing to future-proof this function.
        let Self {
            kv_pool,
            fst_pool,
            dynamic_conf_store,
            last_assumed_new_oid,
            // NOTE: We don’t care about the app configuration,
            //   we can see it elsewhere if needed.
            app_conf: _app_conf,
        } = self;

        f.debug_struct("Executor")
            .field("kv_pool", kv_pool)
            .field("fst_pool", fst_pool)
            .field("dynamic_conf_store", dynamic_conf_store)
            .field("last_assumed_new_oid", last_assumed_new_oid)
            .finish_non_exhaustive()
    }
}

/// A wrapper over a `RwLock<HashMap>` that doesn’t leak private types.
#[derive(Default)]
pub struct DynamicConfigStore(RwLock<HashMap<u32, DynamicConfig, NoopU32HasherBuilder>>);

impl DynamicConfigStore {
    pub fn insert(&self, collection: StoreItemPart, config: DynamicConfig) {
        (self.0.write().unwrap()).insert(collection.to_compact(), config);
    }

    pub fn get(&self, collection: StoreItemPart) -> Option<DynamicConfig> {
        (self.0.read().unwrap())
            .get(&collection.to_compact())
            .copied()
    }

    pub fn read<'a>(&'a self) -> DynamicConfigStoreReadGuard<'a> {
        DynamicConfigStoreReadGuard(self.0.read().unwrap())
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

pub struct DynamicConfigStoreReadGuard<'a>(
    RwLockReadGuard<'a, HashMap<u32, DynamicConfig, NoopU32HasherBuilder>>,
);

impl<'a> DynamicConfigStoreReadGuard<'a> {
    pub fn iter(&self) -> impl Iterator<Item = (&u32, &DynamicConfig)> {
        self.0.iter()
    }

    pub fn get(&self, key: &u32) -> Option<&DynamicConfig> {
        self.0.get(key)
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DynamicConfig {
    pub sonic: DynamicConfigSonic,
    pub rocksdb: DynamicConfigRocksDb,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DynamicConfigSonic {
    pub disable_janitor_tasks: Option<bool>,
    pub disable_fst_consolidate_task: Option<bool>,
    pub disable_kv_flush_task: Option<bool>,
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
        collection: StoreItemPart,
        new_conf: DynamicConfig,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let kv_store_id = KvStoreId::from_part(collection);

        tracing::debug!(
            ?new_conf.rocksdb,
            "Re-opening KV store connection for {kv_store_id:?} with new dynamic configuration overrides…"
        );

        let mut kv_pool_write_guard = self.kv_pool.write().unwrap();

        self.kv_pool
            .close(kv_store_id, Some(&mut kv_pool_write_guard));

        self.kv_pool
            .acquire(
                true,
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
