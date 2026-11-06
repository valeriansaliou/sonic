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

use crate::Context as _;
use crate::store::fst::FstStore;

use super::{FstStoreId, FstStorePathMode, FstStorePool};

impl FstStorePool {
    #[tracing::instrument(
        level = "debug",
        skip_all, fields(?path),
    )]
    pub fn backup(&self, path: &Path) -> Result<(), crate::Error> {
        tracing::debug!("backing up all fst stores to path: {path:?}");

        // Create backup directory (full path)
        fs::create_dir_all(path)?;

        // NOTE: The FST store directory gets created when the first FST is
        //   consolidated, so there is nothing to back up until then.
        if !self.fst_store_config.path.exists() {
            return Ok(());
        }

        // Proceed dump action (backup)
        self.dump_action(
            |collection_hash| tracing::debug_span!("beckup_collection", ?collection_hash),
            FstStorePathMode::Permanent,
            &self.fst_store_config.path,
            path,
            &Self::backup_bucket,
        )
    }

    #[tracing::instrument(
        level = "debug",
        skip_all, fields(?path),
    )]
    pub fn restore(&self, path: &Path) -> Result<(), crate::Error> {
        tracing::debug!("restoring all fst stores from path: {path:?}");

        // Proceed dump action (restore)
        self.dump_action(
            |collection_hash| tracing::debug_span!("restore_collection", ?collection_hash),
            FstStorePathMode::Backup,
            path,
            &self.fst_store_config.path,
            &Self::restore_bucket,
        )
    }

    #[allow(clippy::type_complexity)]
    fn dump_action(
        &self,
        per_collection_span: impl Fn(&str) -> tracing::Span,
        path_mode: FstStorePathMode,
        read_path: &Path,
        write_path: &Path,
        fn_item: &dyn Fn(&Self, &Path, &Path, &str, &str) -> Result<(), crate::Error>,
    ) -> Result<(), crate::Error> {
        let fst_extension = path_mode.extension();

        // Iterate on FST collections.
        'collections: for collection_entry in fs::read_dir(read_path)? {
            let collection_entry = collection_entry?;

            // Actual collection found?
            let file_type = collection_entry.file_type()?;
            if !file_type.is_dir() {
                tracing::trace!(
                    ?file_type,
                    file_path = ?collection_entry.path(),
                    "Found non-directory entry, ignoring."
                );
                continue 'collections;
            }

            let file_name = collection_entry.file_name();
            let Some(collection_hash) = file_name.to_str() else {
                tracing::warn!(
                    file_name_bytes = ?file_name.as_encoded_bytes(),
                    file_path = ?collection_entry.path(),
                    "Found invalid directory entry, ignoring."
                );
                continue 'collections;
            };

            let span = per_collection_span(collection_hash);
            let _span = span.enter();

            // Create write folder for collection.
            fs::create_dir_all(write_path.join(&collection_hash))?;

            // Iterate on FST collection buckets.
            let buckets_path = read_path.join(&collection_hash);
            'buckets: for bucket_entry in fs::read_dir(&buckets_path)? {
                let bucket_entry = bucket_entry?;

                // Actual bucket found?
                let file_type = bucket_entry.file_type()?;
                if !file_type.is_file() {
                    tracing::trace!(
                        ?file_type,
                        file_path = ?bucket_entry.path(),
                        "Found non-file entry, ignoring."
                    );
                    continue 'buckets;
                }

                let file_name = bucket_entry.file_name();
                let Some(bucket_file_name) = file_name.to_str() else {
                    tracing::warn!(
                        file_name_bytes = ?file_name.as_encoded_bytes(),
                        file_path = ?bucket_entry.path(),
                        "Found invalid directory entry, ignoring."
                    );
                    continue 'buckets;
                };

                let Some(bucket_name) = bucket_file_name.strip_suffix(fst_extension) else {
                    tracing::trace!(
                        file_name_bytes = ?file_name.as_encoded_bytes(),
                        "Ignoring {bucket_file_name:?}: wrong extension (expected: {fst_extension:?})"
                    );
                    continue 'buckets;
                };

                fn_item(
                    self,
                    write_path,
                    &bucket_entry.path(),
                    collection_hash,
                    bucket_name,
                )?;
            }
        }

        Ok(())
    }

    #[tracing::instrument(
        level = "debug",
        skip_all, fields(?bucket_name),
    )]
    fn backup_bucket(
        &self,
        backup_dir: &Path,
        _origin_path: &Path,
        collection_hash: &str,
        bucket_name: &str,
    ) -> Result<(), crate::Error> {
        use io::Write as _;

        // Acquire access lock (in blocking write mode), and reference it in context
        // Notice: this prevents store to be acquired from any context.
        let _access = self.graph_access_lock.write().unwrap();

        // Generate path to FST backup.
        let backup_path = {
            let ext = FstStorePathMode::Backup.extension();
            assert!(ext.starts_with("."));
            backup_dir
                .join(collection_hash)
                .join(format!("{bucket_name}{ext}"))
        };

        let store_id = FstStoreId::try_from_hex(collection_hash, bucket_name)?;

        tracing::trace!(?backup_path, "Backing up FST store…");

        // Erase any previously-existing FST backup.
        fs::remove_file(&backup_path).ok();

        // Stream actual FST data to FST backup.
        let backup_fst_file = File::create(&backup_path)?;
        let mut backup_fst_writer = io::BufWriter::new(backup_fst_file);

        let origin_fst =
            FstStore::open(&store_id, &self.fst_store_config).context("Graph open failure")?;

        let mut origin_fst_stream = origin_fst.stream();

        let mut word_count = 0;
        while let Some(word) = origin_fst_stream.next() {
            word_count += 1;

            // Write word, and append a new line.
            backup_fst_writer.write_all(word)?;
            backup_fst_writer.write_all(b"\n")?;
        }

        tracing::debug!(?backup_path, word_count, "FST store backed up");

        Ok(())
    }

    #[tracing::instrument(
        level = "debug",
        skip_all, fields(?bucket_name),
    )]
    fn restore_bucket(
        &self,
        _destination_dir: &Path,
        backup_path: &Path,
        collection_name: &str,
        bucket_name: &str,
    ) -> Result<(), crate::Error> {
        use io::BufRead as _;

        // Acquire access lock (in blocking write mode) to prevent store from
        // being acquired from any context.
        let _access = self.graph_access_lock.write().unwrap();

        let store_id = FstStoreId::try_from_hex(collection_name, bucket_name)?;

        tracing::trace!(?backup_path, "Restoring FST store…");

        // Force a FST store close.
        self.close(&store_id);

        // Generate path to FST.
        let fst_path = self
            .fst_store_config
            .store_path(&store_id, FstStorePathMode::Permanent);

        // Remove existing FST data?
        if fst_path.exists() {
            fs::remove_file(&fst_path)?;
        }

        // Stream backup words to restored FST.
        let fst_writer = io::BufWriter::new(File::create(&fst_path)?);
        let fst_backup_reader = io::BufReader::new(File::open(&backup_path)?);

        let mut fst_builder =
            fst::SetBuilder::new(fst_writer).context("Graph restore builder failure")?;

        for word in fst_backup_reader.lines() {
            let word = word?;

            (fst_builder.insert(word)).context("Graph restore word insert failure")?;
        }

        (fst_builder.finish()).context("Graph restore finish failure")?;

        tracing::debug!(?backup_path, ?fst_path, "FST store restored");

        Ok(())
    }
}
