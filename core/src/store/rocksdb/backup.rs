// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::fs;
use std::path::Path;
use std::sync::Arc;

use rocksdb::backup::{
    BackupEngine as DBBackupEngine, BackupEngineOptions as DBBackupEngineOptions,
    RestoreOptions as DBRestoreOptions,
};

use crate::Context as _;
use crate::store::kv::{GenericKvStorePool, KvStoreId};

use super::GenericRocksDbStore;

#[allow(private_bounds)]
impl<Store: GenericRocksDbStore> GenericKvStorePool<Store> {
    #[tracing::instrument(
        level = "debug",
        skip_all, fields(?path),
    )]
    pub fn backup(&self, path: &Path) -> Result<(), crate::Error> {
        tracing::debug!(
            "backing up all {kind} stores to path: {path:?}",
            kind = Store::kind()
        );

        // Create backup directory (full path)
        fs::create_dir_all(path)?;

        // NOTE: The KV store directory gets created when the first KV store
        //   is opened, so there is nothing to back up until then.
        if !self.kv_store_config.path.exists() {
            return Ok(());
        }

        // Proceed dump action (backup)
        self.dump_action(&self.kv_store_config.path, path, &Self::backup_collection)
    }

    #[tracing::instrument(
        level = "debug",
        skip_all, fields(?path),
    )]
    pub fn restore(&self, path: &Path) -> Result<(), crate::Error> {
        tracing::debug!(
            "restoring all {kind} stores from path: {path:?}",
            kind = Store::kind()
        );

        // Proceed dump action (restore)
        self.dump_action(path, &self.kv_store_config.path, &Self::restore_collection)
    }

    #[allow(clippy::type_complexity)]
    fn dump_action(
        &self,
        read_path: &Path,
        write_path: &Path,
        fn_item: &dyn Fn(&Self, &Path, &Path, &str) -> Result<(), crate::Error>,
    ) -> Result<(), crate::Error> {
        // Iterate on KV collections.
        for entry in fs::read_dir(read_path)? {
            let Ok(collection) = entry else {
                continue;
            };

            // Actual collection found?
            if !collection.file_type().is_ok_and(|f| f.is_dir()) {
                continue;
            }

            if let Some(collection_hash) = collection.file_name().to_str() {
                fn_item(self, write_path, &collection.path(), collection_hash)?;
            }
        }

        Ok(())
    }

    #[tracing::instrument(
        level = "debug",
        skip_all, fields(?collection_hash),
    )]
    fn backup_collection(
        &self,
        backup_dir: &Path,
        _origin_path: &Path,
        collection_hash: &str,
    ) -> Result<(), crate::Error> {
        let kind = Store::kind();

        // Acquire access lock (in blocking write mode), and reference it in context
        // Notice: this prevents store to be acquired from any context
        let _access = self.store_access_lock.write().unwrap();

        // Generate path to KV backup
        let kv_backup_path = backup_dir.join(collection_hash);

        let store_id = KvStoreId::try_from_hex(collection_hash)?;

        tracing::trace!(?backup_dir, "Backing up {kind} store…");

        // Erase any previously-existing KV backup
        if kv_backup_path.exists() {
            fs::remove_dir_all(&kv_backup_path)?;
        }

        // Create backup folder for collection
        fs::create_dir_all(backup_dir.join(collection_hash))?;

        // Initialize KV database backup engine
        let kv_backup_options = DBBackupEngineOptions::new(&kv_backup_path)
            .context("backup engine options acquire failure")?;
        let kv_backup_environment =
            rocksdb::Env::new().context("backup engine environment acquire failure")?;

        let mut kv_backup_engine = DBBackupEngine::open(&kv_backup_options, &kv_backup_environment)
            .context("backup engine failure")?;

        // Proceed actual KV database backup
        // NOTE: RocksDB refuses to open a database which is already open in
        //   this process (its `LOCK` file is held), so back up the pooled
        //   store if there is one. Flush it first, as its memtables may hold
        //   writes not persisted yet (e.g. if the write-ahead log is disabled).
        let pooled_store = self.read().unwrap().get(&store_id).map(Arc::clone);

        match pooled_store {
            Some(store) => kv_backup_engine.create_new_backup_flush(store.database(), true),
            None => {
                let dyn_confs = self.dynamic_configs.read().unwrap();
                let dyn_conf_opt = dyn_confs.get(&store_id).map(Arc::as_ref);

                let origin_kv = Store::open(&store_id, &self.kv_store_config, dyn_conf_opt)
                    .context("database open failure")?;

                drop(dyn_confs);

                kv_backup_engine.create_new_backup(&origin_kv)
            }
        }
        .context("database backup failure")?;

        tracing::debug!(?backup_dir, "{kind} store backed up");

        Ok(())
    }

    #[tracing::instrument(
        level = "debug",
        skip_all, fields(?collection_hash),
    )]
    fn restore_collection(
        &self,
        _destination_dir: &Path,
        backup_path: &Path,
        collection_hash: &str,
    ) -> Result<(), crate::Error> {
        let kind = Store::kind();

        // Acquire access lock (in blocking write mode), and reference it in context
        // Notice: this prevents store to be acquired from any context
        let _access = self.store_access_lock.write().unwrap();

        let store_id = KvStoreId::try_from_hex(collection_hash)?;

        tracing::trace!(?backup_path, "Restoring {kind} store…");

        // Force a KV store close
        self.close(store_id, None);

        // Generate path to KV
        let kv_path = self.kv_store_config.store_path(&store_id);

        // Remove existing KV database data?
        if kv_path.exists() {
            fs::remove_dir_all(&kv_path)?;
        }

        // Create KV folder for collection
        fs::create_dir_all(&kv_path)?;

        // Initialize KV database backup engine
        let kv_backup_options = DBBackupEngineOptions::new(&backup_path)
            .context("backup engine options acquire failure")?;
        let kv_backup_environment =
            rocksdb::Env::new().context("backup engine environment acquire failure")?;

        let mut kv_backup_engine = DBBackupEngine::open(&kv_backup_options, &kv_backup_environment)
            .context("backup engine failure")?;

        kv_backup_engine
            .restore_from_latest_backup(&kv_path, &kv_path, &DBRestoreOptions::default())
            .context("database restore failure")?;

        tracing::debug!(?backup_path, ?kv_path, "{kind} store restored");

        Ok(())
    }
}
