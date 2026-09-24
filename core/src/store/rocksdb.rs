// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::config::RocksDbDatabaseConfig;

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

        // db_options.set_block_cache_size();
        // db_options.set_cache_index_and_filter_blocks();

        if let Some(block_cache_size) = block_cache_size {
            let cache = rocksdb::Cache::new_lru_cache((*block_cache_size as usize) * 1024 * 1024);
            let mut block_opts = rocksdb::BlockBasedOptions::default();
            block_opts.set_block_cache(&cache);
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

        let mut max_background_jobs = *max_background_jobs;

        if let Some(max_flushes) = max_flushes {
            if max_background_jobs.is_none() {
                max_background_jobs = Some(max_subcompactions.unwrap_or(1) as i32 + max_flushes);
            }

            #[allow(deprecated)]
            db_options.set_max_background_flushes(*max_flushes);

            // Update threads configuration otherwise RocksDB only uses 1/4 for flushes by default.
            env.set_high_priority_background_threads(*max_flushes); // HIGH pool = flushes (default)
            env.set_low_priority_background_threads(max_subcompactions.unwrap_or(1) as i32 - max_flushes); // LOW pool = compactions (default)
        }

        if_some!(db_options.set_max_background_jobs(max_background_jobs.as_ref()));
        if_some!(db_options.set_max_subcompactions(max_subcompactions));

        if_some!(db_options.set_stats_dump_period_sec(stats_dump_period_sec));

        if_some!(db_options.increase_parallelism(parallelism));

        db_options.set_env(&env);

        db_options
    }
}
