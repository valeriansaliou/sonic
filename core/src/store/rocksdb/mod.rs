// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod backup;
pub(crate) mod pool;

use std::ops::Range;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use crate::config::{KvStoreConfig, RocksDbDatabaseConfig};
use crate::store::Bucket;
use crate::store::generic::GenericStore;

pub(super) trait GenericRocksDbStore: GenericStore {
    fn new(db: rocksdb::DB, config: Arc<KvStoreConfig>) -> Self;

    #[allow(
        unused_variables,
        reason = "Underscoring would affect what’s generated when implementing"
    )]
    fn configure(db_options: &mut rocksdb::Options) {}

    fn bucket_key_range(bucket: &Bucket) -> Range<Vec<u8>>;

    #[allow(
        unused_variables,
        reason = "Underscoring would affect what’s generated when implementing"
    )]
    fn on_batch_erase_bucket(&self, bucket: &Bucket) {}

    fn database(&self) -> &rocksdb::DB;

    fn config(&self) -> &KvStoreConfig;

    fn last_flushed(&self) -> &RwLock<Instant>;

    fn lock(&self) -> &RwLock<()>;

    fn write(&self, batch: rocksdb::WriteBatch) -> Result<(), rocksdb::Error> {
        // Configure this write
        let mut write_options = rocksdb::WriteOptions::default();

        write_options.set_memtable_insert_hint_per_batch(true);

        //         struct LogHandler;
        //
        //         static COUNTER: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
        //         static COUNTER_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        //
        //         impl rocksdb::WriteBatchIteratorCf for LogHandler {
        //             fn put_cf(&mut self, _cf_id: u32, key: &[u8], value: &[u8]) {
        //                 if COUNTER.load(std::sync::atomic::Ordering::Relaxed) < 3 {
        //                     println!("p: key={key:?} value={value:?}");
        //                 }
        //             }
        //
        //             fn delete_cf(&mut self, _cf_id: u32, key: &[u8]) {
        //                 if COUNTER.load(std::sync::atomic::Ordering::Relaxed) < 3 {
        //                     println!("d: key={key:?}");
        //                 }
        //             }
        //
        //             fn merge_cf(&mut self, _cf_id: u32, key: &[u8], value: &[u8]) {
        //                 if COUNTER.load(std::sync::atomic::Ordering::Relaxed) < 3 {
        //                     println!("m: key={key:?} value={value:?}");
        //                 }
        //             }
        //         }
        //
        //         let lock = COUNTER_LOCK.lock().unwrap();
        //         if COUNTER.load(std::sync::atomic::Ordering::Relaxed) < 3 {
        //             println!("=== batch ===");
        //         }
        //         batch.iterate_cf(&mut LogHandler);
        //         COUNTER.update(
        //             std::sync::atomic::Ordering::SeqCst,
        //             std::sync::atomic::Ordering::SeqCst,
        //             |ct| ct.saturating_add(1),
        //         );
        //         drop(lock);

        // WAL disabled?
        if !self.config().database.write_ahead_log {
            tracing::trace!("Ignoring WAL for {} write", Self::kind());

            write_options.disable_wal(true);
        } else {
            tracing::trace!("Using WAL for {} write", Self::kind());

            write_options.disable_wal(false);
        }

        // Commit this write
        self.database().write_opt(batch, &write_options)
    }

    fn batch_erase_bucket(&self, bucket: &Bucket) -> Result<u32, ()> {
        tracing::debug!("{} store batch erase bucket: {bucket}", Self::kind());

        // Generate start and end prefix for batch delete (in other words,
        // the minimum key value possible, and the highest key value possible).
        let key_range = Self::bucket_key_range(&bucket);

        let mut batch = rocksdb::WriteBatch::default();

        // Batch-delete keys matching range.
        // NOTE: RocksDB excludes end key, but Rust ranges are exclusive too
        //   ([as they should](https://devblog.remibardon.name/til/dijkstra-ranges/))
        //   so all keys will be deleted.
        batch.delete_range(&key_range.start, &key_range.end);

        // Commit operation to database.
        match self.write(batch) {
            Ok(()) => {
                self.on_batch_erase_bucket(bucket);
                tracing::debug!("succeeded in store batch erase bucket: {bucket}");
                Ok(1)
            }
            Err(error) => {
                tracing::error!(
                    "failed in store batch erase bucket: {bucket} with error: {error:?}"
                );
                Err(())
            }
        }
    }

    fn flush(&self) -> Result<(), rocksdb::Error> {
        // Generate flush options
        let mut flush_options = rocksdb::FlushOptions::default();

        flush_options.set_wait(true);

        // Perform flush (in blocking mode)
        self.database().flush_opt(&flush_options)
    }
}

impl From<&RocksDbDatabaseConfig> for rocksdb::Options {
    #[rustfmt::skip]
    fn from(config: &RocksDbDatabaseConfig) -> Self {
        // NOTE: Deconstruct to avoid forgetting configuration keys.
        let RocksDbDatabaseConfig {
            flush_after: _,
            compress,
            parallelism,
            max_open_files,
            max_flushes,
            write_ahead_log: _,
            write_buffer_size,
            max_write_buffer_number,
            min_write_buffer_number,
            min_write_buffer_number_to_merge,
            block_cache_size,
            cache_index_and_filter_blocks,
            compression_type,
            wal_compression_type,
            wal_ttl_seconds,
            wal_size_limit_mb,
            wal_bytes_per_sync,
            wal_recovery_mode,
            compression_level,
            min_level_to_compress,
            level_zero_file_num_compaction_trigger,
            level_zero_slowdown_writes_trigger,
            level_zero_stop_writes_trigger,
            max_bytes_for_level_base,
            max_bytes_for_level_multiplier,
            target_file_size_base,
            max_background_jobs,
            max_subcompactions,
            stats_dump_period_sec,
            enable_blob_files,
            min_blob_size,
            blob_file_size,
            enable_blob_gc,
            blob_compression_type,
            log_level,
            db_log_dir,
            keep_log_file_num,
            max_log_file_size,
        } = config;

        // Make database options
        let mut db_options = rocksdb::Options::default();
        let mut env = rocksdb::Env::new().unwrap();

        macro_rules! if_some {
            ($(#[$($meta:meta),+])? $opts:ident.$set_fn:ident($value:expr)) => {
                if let Some(value) = $value {
                    $(#[$($meta),+])?
                    $opts.$set_fn(*value);
                }
            };
        }

        // Set static options
        db_options.create_if_missing(true);
        db_options.set_use_fsync(false);
        db_options.set_compaction_style(rocksdb::DBCompactionStyle::Level);

        // Set dynamic options
        if_some!(db_options.set_write_buffer_size(write_buffer_size.map(|n| n * 1024).as_ref()));
        if_some!(db_options.set_min_write_buffer_number(min_write_buffer_number));
        if_some!(db_options.set_min_write_buffer_number_to_merge(min_write_buffer_number_to_merge));
        if_some!(db_options.set_max_write_buffer_number(max_write_buffer_number));

        if_some!(db_options.set_max_open_files(max_open_files));

        // Configure block-based file storage.
        {
            let mut block_opts = rocksdb::BlockBasedOptions::default();

            // block_opts.set_block_size(16 * 1024); // 16 KiB instead of the 4 KiB default

            if let Some(block_cache_size) = block_cache_size {
                let cache = rocksdb::Cache::new_lru_cache((*block_cache_size as usize) * 1024 * 1024);
                block_opts.set_block_cache(&cache);
            }

            if_some!(block_opts.set_cache_index_and_filter_blocks(cache_index_and_filter_blocks));

            db_options.set_block_based_table_factory(&block_opts);
        }

        // NOTE: `compress` is a legacy shorthand for `compression_type`, it
        //   will get overriden if `compression_type` is also specified.
        if let Some(compress) = compress {
            db_options.set_compression_type(if *compress {
                rocksdb::DBCompressionType::Zstd
            } else {
                rocksdb::DBCompressionType::None
            });
        }
        if_some!(db_options.set_compression_type(compression_type));
        if let Some(compression_level) = compression_level {
            db_options.set_compression_options(
                -14,
                *compression_level,
                0,
                0,
            );
        }

        if_some!(db_options.set_wal_compression_type(wal_compression_type));
        if_some!(db_options.set_wal_ttl_seconds(wal_ttl_seconds));
        if_some!(db_options.set_wal_size_limit_mb(wal_size_limit_mb));
        if_some!(db_options.set_wal_bytes_per_sync(wal_bytes_per_sync));
        if_some!(db_options.set_wal_recovery_mode(wal_recovery_mode));

        if_some!(db_options.set_min_level_to_compress(min_level_to_compress));

        if_some!(db_options.set_level_zero_file_num_compaction_trigger(level_zero_file_num_compaction_trigger));
        if_some!(db_options.set_level_zero_slowdown_writes_trigger(level_zero_slowdown_writes_trigger));
        if_some!(db_options.set_level_zero_stop_writes_trigger(level_zero_stop_writes_trigger));

        if_some!(db_options.set_max_bytes_for_level_base(max_bytes_for_level_base));
        if_some!(db_options.set_max_bytes_for_level_multiplier(max_bytes_for_level_multiplier));
        if_some!(db_options.set_target_file_size_base(target_file_size_base));

        if_some!(db_options.set_enable_blob_files(enable_blob_files));
        if_some!(db_options.set_min_blob_size(min_blob_size));
        if_some!(db_options.set_blob_file_size(blob_file_size));
        if_some!(db_options.set_enable_blob_gc(enable_blob_gc));
        if_some!(db_options.set_blob_compression_type(blob_compression_type));

        let mut max_background_jobs = *max_background_jobs;

        if let Some(max_flushes) = max_flushes {
            if max_background_jobs.is_none() {
                max_background_jobs = Some(max_subcompactions.unwrap_or(1) as i32 + max_flushes);
            }

            #[allow(deprecated)]
            db_options.set_max_background_flushes(*max_flushes);

            // Update threads configuration otherwise RocksDB only uses 1/4 for flushes by default.
            env.set_high_priority_background_threads(*max_flushes); // HIGH pool = flushes (default)
            env.set_low_priority_background_threads(max_background_jobs.map_or(1i32, |n| (n - max_flushes).max(1i32))); // LOW pool = compactions (default)
        }

        if_some!(db_options.set_max_background_jobs(max_background_jobs.as_ref()));
        if_some!(db_options.set_max_subcompactions(max_subcompactions));

        if_some!(db_options.set_log_level(log_level));
        if_some!(db_options.set_db_log_dir(db_log_dir.as_ref().as_ref()));
        if_some!(db_options.set_keep_log_file_num(keep_log_file_num));
        if_some!(db_options.set_max_log_file_size(max_log_file_size));
        if_some!(db_options.set_stats_dump_period_sec(stats_dump_period_sec));

        if_some!(db_options.increase_parallelism(parallelism));

        db_options.set_env(&env);

        db_options
    }
}
