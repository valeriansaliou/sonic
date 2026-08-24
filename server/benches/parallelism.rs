// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;
mod parallelism_common;

use std::sync::{Arc, RwLock};

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

use crate::common::random::SplitMix64;
use crate::common::{no_prepopulate, prelude::*, prepopulate_gdpr};
use crate::parallelism_common::{
    Actor, ConsolidateActor, FlushActor, ParallelismBenchmarkConfig, PushActor, QueryActor,
    run_bench, run_bench_multi,
};

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);

fn criterion_benchmark(c: &mut Criterion) {
    init_logging(
        Some("sonic_client=info,sonic_core=warn,parallelism::parallelism_common=warn"),
        LoggingOptions {
            default_log_level: tracing::Level::WARN,
            with_target: true,
            with_file: false,
            with_line_number: false,
            with_level: true,
            with_thread_ids: true,
        },
    );

    let mut group = c.benchmark_group("parallelism");

    // group.warm_up_time(Duration::from_secs(10));
    // group.measurement_time(Duration::from_secs(10));
    // group.sampling_mode(SamplingMode::Flat);
    // group.sample_size(10000);

    const PARALLEL_READERS: usize = 50;
    run_bench(
        BenchmarkId::new("parallel_reads", PARALLEL_READERS),
        &mut group,
        |c| c,
        prepopulate_gdpr,
        Some(17950370156880176289u64),
        |ctx, bus| {
            std::array::from_fn::<_, PARALLEL_READERS, _>(|_| {
                Box::new(QueryActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|_tick| true),
                    collection: "articles".to_owned(),
                    bucket: "default".to_owned(),
                    expected: (String::new(), String::new()),
                    next_expected: Arc::new(RwLock::new(Some((
                        "article:3".to_owned(),
                        "European Union".to_owned(),
                    )))),
                }) as Box<dyn Actor>
            })
        },
    );

    // Warm up longer so we get write amplification.
    // NOTE: This affects all future benchmarks.
    // group.warm_up_time(Duration::from_secs(30));
    // NOTE: Make `--quick` wait longer before switching to measurements.
    group.significance_level(0.001);
    // group.measurement_time(Duration::from_secs(5));
    // group.sampling_mode(SamplingMode::Flat);
    // group.sample_size(100);

    const PARALLEL_WRITERS: usize = 5;
    run_bench_multi(
        "parallel_writes",
        &[
            ParallelismBenchmarkConfig::default(),
            ParallelismBenchmarkConfig {
                // Skip the write-ahead log. Faster writes, but data loss in
                // case of crash.
                rocksdb_wal_enabled: Some(false),
                ..Default::default()
            },
            ParallelismBenchmarkConfig {
                // Skip the write-ahead log. Faster writes, but data loss in
                // case of crash.
                rocksdb_wal_enabled: Some(false),
                // Unlimlited max open SST files. Faster reads, and potentially
                // faster compactions (no reopen cost).
                rocksdb_max_open_files: Some(-1),
                ..Default::default()
            },
            ParallelismBenchmarkConfig {
                // Skip the write-ahead log. Faster writes, but data loss in
                // case of crash.
                rocksdb_wal_enabled: Some(false),
                // Larger memtables before flushing to SST. Fewer flushes
                // and compactions but more RAM use and longer flush time.
                rocksdb_write_buffer_size: Some(32 * 1024 * 1024),
                ..Default::default()
            },
            ParallelismBenchmarkConfig {
                // Skip the write-ahead log. Faster writes, but data loss in
                // case of crash.
                rocksdb_wal_enabled: Some(false),
                // Larger memtables before flushing to SST. Fewer flushes
                // and compactions but more RAM use and longer flush time.
                rocksdb_write_buffer_size: Some(64 * 1024 * 1024),
                ..Default::default()
            },
            ParallelismBenchmarkConfig {
                // Skip the write-ahead log. Faster writes, but data loss in
                // case of crash.
                rocksdb_wal_enabled: Some(false),
                // Larger memtables before flushing to SST. Fewer flushes
                // and compactions but more RAM use and longer flush time.
                rocksdb_write_buffer_size: Some(256 * 1024 * 1024),
                ..Default::default()
            },
            ParallelismBenchmarkConfig {
                // Skip the write-ahead log. Faster writes, but data loss in
                // case of crash.
                rocksdb_wal_enabled: Some(false),
                // Unlimlited max open SST files. Faster reads, and potentially
                // faster compactions (no reopen cost).
                rocksdb_max_open_files: Some(-1),
                // Larger memtables before flushing to SST. Fewer flushes
                // and compactions but more RAM use and longer flush time.
                rocksdb_write_buffer_size: Some(256 * 1024 * 1024),
                // Fewer, larger L0 files.
                rocksdb_min_write_buffer_number: Some(4),
                // No L0→L1 compaction based on file count.
                rocksdb_level_zero_file_num_compaction_trigger: Some(-1),
                // No write throttling, no matter how many L0 files exist.
                rocksdb_level_zero_slowdown_writes_trigger: Some(-1),
                // No write blocking, no matter how many L0 files exist.
                rocksdb_level_zero_stop_writes_trigger: Some(i32::MAX),
                ..Default::default()
            },
        ],
        &mut group,
        no_prepopulate,
        Some(17950370156880176289u64),
        |ctx, bus| {
            let mut split_mix = SplitMix64::new(*ctx.seed);

            // Create parallel writers.
            let mut actors = std::array::from_fn::<_, PARALLEL_WRITERS, _>(|_| PushActor {
                addr: ctx.addr,
                inbox: bus.add_rx(),
                condition: Box::new(|_tick| true),
                seed: SplitMix64::new(split_mix.next_u64()),
                collection: "articles".to_owned(),
                bucket: "default".to_owned(),
                next_write: None,
                last_write: Arc::new(RwLock::new(None)),
            });

            // Make some actors conflict (push the same thing).
            actors[1].seed = actors[0].seed.clone();

            actors.map(|f| Box::new(f) as Box<dyn Actor>)
        },
    );

    run_bench(
        BenchmarkId::new("parallel_read_writes", "p2-c1-q4"),
        &mut group,
        |c| c,
        no_prepopulate,
        Some(17950370156880176289u64),
        |ctx, bus| {
            let write_channel1 = Arc::new(RwLock::new(None));
            let write_channel2 = Arc::new(RwLock::new(None));

            let mut split_mix = SplitMix64::new(*ctx.seed);

            [
                Box::new(QueryActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| tick > 1),
                    collection: "articles".to_owned(),
                    bucket: "default".to_owned(),
                    expected: (String::new(), String::new()),
                    next_expected: Arc::clone(&write_channel1),
                }) as Box<dyn Actor>,
                Box::new(QueryActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| tick > 1),
                    collection: "articles".to_owned(),
                    bucket: "default".to_owned(),
                    expected: (String::new(), String::new()),
                    next_expected: Arc::clone(&write_channel1),
                }),
                Box::new(PushActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| tick.is_multiple_of(2)),
                    seed: SplitMix64::new(split_mix.next_u64()),
                    collection: "articles".to_owned(),
                    bucket: "default".to_owned(),
                    next_write: None,
                    last_write: write_channel1,
                }),
                Box::new(QueryActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| tick > 1),
                    collection: "articles".to_owned(),
                    bucket: "default".to_owned(),
                    expected: (String::new(), String::new()),
                    next_expected: Arc::clone(&write_channel2),
                }),
                Box::new(QueryActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| tick > 1),
                    collection: "articles".to_owned(),
                    bucket: "default".to_owned(),
                    expected: (String::new(), String::new()),
                    next_expected: Arc::clone(&write_channel2),
                }),
                Box::new(PushActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| tick.is_multiple_of(2)),
                    seed: SplitMix64::new(split_mix.next_u64()),
                    collection: "articles".to_owned(),
                    bucket: "default".to_owned(),
                    next_write: None,
                    last_write: write_channel2,
                }),
                Box::new(ConsolidateActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| (tick - 1).is_multiple_of(100)),
                }),
            ]
        },
    );

    run_bench(
        BenchmarkId::new("all_in_one", "p2-c1-f1-q4"),
        &mut group,
        |c| c,
        no_prepopulate,
        Some(17950370156880176289u64),
        |ctx, bus| {
            let write_channel1 = Arc::new(RwLock::new(None));
            let write_channel2 = Arc::new(RwLock::new(None));

            let mut split_mix = SplitMix64::new(*ctx.seed);

            [
                Box::new(QueryActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| tick > 1 && !tick.is_multiple_of(4)),
                    collection: "articles".to_owned(),
                    bucket: "default".to_owned(),
                    expected: (String::new(), String::new()),
                    next_expected: Arc::clone(&write_channel1),
                }) as Box<dyn Actor>,
                Box::new(QueryActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| tick > 1 && !tick.is_multiple_of(4)),
                    collection: "articles".to_owned(),
                    bucket: "default".to_owned(),
                    expected: (String::new(), String::new()),
                    next_expected: Arc::clone(&write_channel1),
                }),
                Box::new(PushActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| tick.is_multiple_of(3)),
                    seed: SplitMix64::new(split_mix.next_u64()),
                    collection: "articles".to_owned(),
                    bucket: "default".to_owned(),
                    next_write: None,
                    last_write: write_channel1,
                }),
                Box::new(QueryActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| tick > 2 && !tick.is_multiple_of(4)),
                    collection: "articles".to_owned(),
                    bucket: "default".to_owned(),
                    expected: (String::new(), String::new()),
                    next_expected: Arc::clone(&write_channel2),
                }),
                Box::new(QueryActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| tick > 2 && !tick.is_multiple_of(4)),
                    collection: "articles".to_owned(),
                    bucket: "default".to_owned(),
                    expected: (String::new(), String::new()),
                    next_expected: Arc::clone(&write_channel2),
                }),
                Box::new(PushActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| (tick + 1).is_multiple_of(3)),
                    seed: SplitMix64::new(split_mix.next_u64()),
                    collection: "articles".to_owned(),
                    bucket: "default".to_owned(),
                    next_write: None,
                    last_write: write_channel2,
                }),
                Box::new(ConsolidateActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| (tick + 1).is_multiple_of(5)),
                }),
                Box::new(FlushActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| (tick + 1).is_multiple_of(5)),
                    fail_if_not_found: false,
                }),
            ]
        },
    );

    #[cfg(feature = "experimental-api")]
    run_bench(
        BenchmarkId::new("test_flush_impact", "p1-f1"),
        &mut group,
        |c| c,
        no_prepopulate,
        Some(17950370156880176289u64),
        |ctx, bus| {
            let mut split_mix = SplitMix64::new(*ctx.seed);

            [
                Box::new(PushActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|_tick| true),
                    seed: SplitMix64::new(split_mix.next_u64()),
                    collection: "articles".to_owned(),
                    bucket: "default".to_owned(),
                    next_write: None,
                    last_write: Arc::new(RwLock::new(None)),
                }) as Box<dyn Actor>,
                Box::new(FlushActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| tick > 0 && tick.is_multiple_of(4)),
                    fail_if_not_found: true,
                }),
            ]
        },
    );
}
