// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::path::Path;
use std::sync::Arc;
use std::{fs, io};

use rocksdb::backup::{
    BackupEngine as DBBackupEngine, BackupEngineOptions as DBBackupEngineOptions,
    RestoreOptions as DBRestoreOptions,
};

use crate::store::kv::{GenericKvStorePool, KvStoreId};

use super::GenericRocksDbStore;

#[allow(private_bounds)]
impl<Store: GenericRocksDbStore> GenericKvStorePool<Store> {
    pub fn backup(&self, path: &Path) -> Result<(), io::Error> {
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
        self.dump_action(
            "backup",
            &self.kv_store_config.path,
            path,
            &Self::backup_item,
        )
    }

    pub fn restore(&self, path: &Path) -> Result<(), io::Error> {
        tracing::debug!(
            "restoring all {kind} stores from path: {path:?}",
            kind = Store::kind()
        );

        // Proceed dump action (restore)
        self.dump_action(
            "restore",
            path,
            &self.kv_store_config.path,
            &Self::restore_item,
        )
    }

    #[allow(clippy::type_complexity)]
    fn dump_action(
        &self,
        action: &str,
        read_path: &Path,
        write_path: &Path,
        fn_item: &dyn Fn(&Self, &Path, &Path, &str) -> Result<(), io::Error>,
    ) -> Result<(), io::Error> {
        let kind = Store::kind();

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
                tracing::debug!("{kind} collection ongoing {action}: {collection_hash}");

                fn_item(self, write_path, &collection.path(), collection_hash)?;
            }
        }

        Ok(())
    }

    fn backup_item(
        &self,
        backup_path: &Path,
        _origin_path: &Path,
        collection_hash: &str,
    ) -> Result<(), io::Error> {
        let kind = Store::kind();

        // Acquire access lock (in blocking write mode), and reference it in context
        // Notice: this prevents store to be acquired from any context
        let _access = self.store_access_lock.write().unwrap();

        // Generate path to KV backup
        let kv_backup_path = backup_path.join(collection_hash);

        let store_id = KvStoreId::try_from_hex(collection_hash)?;

        tracing::debug!("{kind} store {store_id} backing up to path: {kv_backup_path:?}");

        // Erase any previously-existing KV backup
        if kv_backup_path.exists() {
            fs::remove_dir_all(&kv_backup_path)?;
        }

        // Create backup folder for collection
        fs::create_dir_all(backup_path.join(collection_hash))?;

        // Initialize KV database backup engine
        let kv_backup_options = DBBackupEngineOptions::new(&kv_backup_path)
            .map_err(|_| io::Error::other("backup engine options acquire failure"))?;
        let kv_backup_environment = rocksdb::Env::new()
            .map_err(|_| io::Error::other("backup engine environment acquire failure"))?;

        let mut kv_backup_engine = DBBackupEngine::open(&kv_backup_options, &kv_backup_environment)
            .map_err(|_| io::Error::other("backup engine failure"))?;

        // Proceed actual KV database backup
        // NOTE: RocksDB refuses to open a database which is already open in
        //   this process (its `LOCK` file is held), so back up the pooled
        //   store if there is one. Flush it first, as its memtables may hold
        //   writes not persisted yet (e.g. if the write-ahead log is disabled).
        let pooled_store = self.read().unwrap().get(&store_id).map(Arc::clone);

        match pooled_store {
            Some(store) => kv_backup_engine.create_new_backup_flush(store.database(), true),
            None => {
                let origin_kv = self
                    .open(&store_id, |_| {})
                    .map_err(|_| io::Error::other("database open failure"))?;

                kv_backup_engine.create_new_backup(&origin_kv)
            }
        }
        .map_err(|_| io::Error::other("database backup failure"))?;

        tracing::info!("{kind} store {store_id} backed up to path: {kv_backup_path:?}");

        Ok(())
    }

    fn restore_item(
        &self,
        _backup_path: &Path,
        origin_path: &Path,
        collection_hash: &str,
    ) -> Result<(), io::Error> {
        let kind = Store::kind();

        // Acquire access lock (in blocking write mode), and reference it in context
        // Notice: this prevents store to be acquired from any context
        let _access = self.store_access_lock.write().unwrap();

        let store_id = KvStoreId::try_from_hex(collection_hash)?;

        tracing::debug!("{kind} store {store_id} restoring from path: {origin_path:?}");

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
        let kv_backup_options = DBBackupEngineOptions::new(&origin_path)
            .map_err(|_| io::Error::other("backup engine options acquire failure"))?;
        let kv_backup_environment = rocksdb::Env::new()
            .map_err(|_| io::Error::other("backup engine environment acquire failure"))?;

        let mut kv_backup_engine = DBBackupEngine::open(&kv_backup_options, &kv_backup_environment)
            .map_err(|_| io::Error::other("backup engine failure"))?;

        kv_backup_engine
            .restore_from_latest_backup(&kv_path, &kv_path, &DBRestoreOptions::default())
            .map_err(|_| io::Error::other("database restore failure"))?;

        tracing::info!(
            "{kind} store {store_id} restored to path: {kv_path:?} from backup: {origin_path:?}"
        );

        Ok(())
    }
}
