// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::fs::File;
use std::path::Path;
use std::{fs, io};

use fst::Streamer as _;

use super::{FstStoreId, FstStorePathMode, FstStorePool};

impl FstStorePool {
    pub fn backup(&self, path: &Path) -> Result<(), io::Error> {
        tracing::debug!("backing up all fst stores to path: {path:?}");

        // Create backup directory (full path)
        fs::create_dir_all(path)?;

        // Proceed dump action (backup)
        self.dump_action(
            "backup",
            FstStorePathMode::Permanent,
            &self.fst_store_config.path,
            path,
            &Self::backup_item,
        )
    }

    pub fn restore(&self, path: &Path) -> Result<(), io::Error> {
        tracing::debug!("restoring all fst stores from path: {path:?}");

        // Proceed dump action (restore)
        self.dump_action(
            "restore",
            FstStorePathMode::Backup,
            path,
            &self.fst_store_config.path,
            &Self::restore_item,
        )
    }

    #[allow(clippy::type_complexity)]
    fn dump_action(
        &self,
        action: &str,
        path_mode: FstStorePathMode,
        read_path: &Path,
        write_path: &Path,
        fn_item: &dyn Fn(&Self, &Path, &Path, &str, &str) -> Result<(), io::Error>,
    ) -> Result<(), io::Error> {
        let fst_extension = path_mode.extension();

        // Iterate on FST collections.
        'collections: for collection_entry in fs::read_dir(read_path)? {
            let collection_entry = collection_entry?;

            // Actual collection found?
            let file_type = collection_entry.file_type()?;
            if !file_type.is_dir() {
                tracing::trace!(
                    ?file_type,
                    "Found non-directory entry in {read_path:?}, ignoring"
                );
                continue 'collections;
            }

            let file_name = collection_entry.file_name();
            let Some(collection_hash) = file_name.to_str() else {
                tracing::warn!(
                    file_name_bytes = ?file_name.as_encoded_bytes(),
                    "Found invalid entry in {read_path:?}, ignoring"
                );
                continue 'collections;
            };

            tracing::debug!("fst collection ongoing {action}: {collection_hash}");

            // Create write folder for collection.
            fs::create_dir_all(write_path.join(&collection_hash))?;

            // Iterate on FST collection buckets.
            let buckets_path = read_path.join(&collection_hash);
            'buckets: for bucket_entry in fs::read_dir(&buckets_path)? {
                let bucket_entry = bucket_entry?;

                // Actual bucket found?
                let file_type = collection_entry.file_type()?;
                if !file_type.is_file() {
                    tracing::trace!(
                        ?file_type,
                        "Found non-file entry in {buckets_path:?}, ignoring"
                    );
                    continue 'buckets;
                }

                let file_name = collection_entry.file_name();
                let Some(bucket_file_name) = file_name.to_str() else {
                    tracing::warn!(
                        file_name_bytes = ?file_name.as_encoded_bytes(),
                        "Found invalid entry in {buckets_path:?}, ignoring"
                    );
                    continue 'buckets;
                };

                let Some(bucket_hash) = bucket_file_name.strip_suffix(fst_extension) else {
                    tracing::trace!(
                        file_name_bytes = ?file_name.as_encoded_bytes(),
                        "Ignoring {bucket_file_name:?}: wrong extension (expected: {fst_extension:?})"
                    );
                    continue 'buckets;
                };

                tracing::debug!("fst bucket ongoing {action}: {collection_hash}/{bucket_hash}");

                fn_item(
                    self,
                    write_path,
                    &bucket_entry.path(),
                    collection_hash,
                    bucket_hash,
                )?;
            }
        }

        Ok(())
    }

    fn backup_item(
        &self,
        backup_path: &Path,
        _origin_path: &Path,
        collection_hash: &str,
        bucket_hash: &str,
    ) -> Result<(), io::Error> {
        use io::Write as _;

        // Acquire access lock (in blocking write mode), and reference it in context
        // Notice: this prevents store to be acquired from any context.
        let _access = self.graph_access_lock.write().unwrap();

        // Generate path to FST backup.
        let fst_backup_path = {
            let ext = FstStorePathMode::Backup.extension();
            assert!(ext.starts_with("."));
            backup_path
                .join(collection_hash)
                .join(format!("{bucket_hash}{ext}"))
        };

        let store_id = FstStoreId::try_from_hex(collection_hash, bucket_hash)?;

        tracing::debug!("fst store {store_id} backing up to path: {fst_backup_path:?}");

        // Erase any previously-existing FST backup.
        fs::remove_file(&fst_backup_path).ok();

        // Stream actual FST data to FST backup.
        let backup_fst_file = File::create(&fst_backup_path)?;
        let mut backup_fst_writer = io::BufWriter::new(backup_fst_file);

        let origin_fst = (self.open(store_id))
            .map_err(|error| io::Error::other(format!("Graph open failure: {error:?}")))?;

        let mut origin_fst_stream = origin_fst.stream();

        let mut count_words = 0;
        while let Some(word) = origin_fst_stream.next() {
            count_words += 1;

            // Write word, and append a new line.
            backup_fst_writer.write_all(word)?;
            backup_fst_writer.write_all(b"\n")?;
        }

        tracing::info!(
            "fst store {store_id} backed up to path: {fst_backup_path:?} ({count_words} words)"
        );

        Ok(())
    }

    fn restore_item(
        &self,
        _backup_path: &Path,
        origin_path: &Path,
        collection_name: &str,
        bucket_name: &str,
    ) -> Result<(), io::Error> {
        use io::BufRead as _;

        // Acquire access lock (in blocking write mode) to prevent store from
        // being acquired from any context.
        let _access = self.graph_access_lock.write().unwrap();

        let store_id = FstStoreId::try_from_hex(collection_name, bucket_name)?;

        tracing::debug!("fst store {store_id} restoring from path: {origin_path:?}");

        // Force a FST store close.
        self.close(store_id);

        // Generate path to FST.
        let fst_path = self
            .fst_store_config
            .store_path(store_id, FstStorePathMode::Permanent);

        // Remove existing FST data?
        if fst_path.exists() {
            fs::remove_file(&fst_path)?;
        }

        // Stream backup words to restored FST.
        let fst_writer = io::BufWriter::new(File::create(&fst_path)?);
        let fst_backup_reader = io::BufReader::new(File::open(&origin_path)?);

        let mut fst_builder = fst::SetBuilder::new(fst_writer).map_err(|error| {
            io::Error::other(format!("Graph restore builder failure: {error:?}"))
        })?;

        for word in fst_backup_reader.lines() {
            let word = word?;

            fst_builder.insert(word).map_err(|error| {
                io::Error::other(format!("Graph restore word insert failure: {error:?}"))
            })?;
        }

        fst_builder.finish().map_err(|error| {
            io::Error::other(format!("Graph restore finish failure: {error:?}"))
        })?;

        tracing::info!(
            "fst store {store_id} restored to path: {fst_path:?} from backup: {origin_path:?}"
        );

        Ok(())
    }
}
