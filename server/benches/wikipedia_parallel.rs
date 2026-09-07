// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;
#[path = "common/huggingface/wikipedia.rs"]
mod huggingface_wikipedia;
mod wikipedia_common;

use std::hint::black_box;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

use crate::common::client_helpers::trigger_compact;
use crate::common::huggingface::download::download_shards;
use crate::common::huggingface::load::iter_shard;
use crate::common::logging::{CompactThousands, HumanBytes};
use crate::common::prelude::*;
use crate::huggingface_wikipedia::WikipediaArticle;
use crate::wikipedia_common::*;

static SHARD_PATHS: LazyLock<Vec<PathBuf>> =
    LazyLock::new(|| download_shards("wikimedia/wikipedia", "20231101.en", Some(4)));

fn articles_iter(limit: usize) -> impl Iterator<Item = WikipediaArticle> {
    SHARD_PATHS
        .iter()
        .flat_map(iter_shard::<WikipediaArticle>)
        // .filter(|a| a.text.as_bytes().len() > 2000)
        // .filter(|a| a.text.as_bytes().len() < 8000)
        // .filter(|a| a.text.as_bytes().len() > 20000)
        .take(limit)
}

fn criterion_benchmark(c: &mut Criterion) {
    init_logging(
        None,
        LoggingOptions {
            default_log_level: tracing::Level::WARN,
            with_target: true,
            with_file: false,
            with_line_number: false,
            with_level: true,
            with_thread_ids: false,
        },
    );

    let show_progress = *SHOW_PROGRESS;

    let mut group = c.benchmark_group("wikipedia_parallel");

    // No need to warm up for 3 seconds (default).
    group.warm_up_time(Duration::from_secs(1));

    let articles = || articles_iter(usize::MAX);

    // Lower sample size as what we’re measuring is quite long to execute.
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(30));

    let total_bytes = articles().map(|article| article.text.len() as u64).sum();
    group.throughput(Throughput::ElementsAndBytes {
        elements: articles().count() as u64,
        bytes: total_bytes,
    });

    let nchannels: usize = std::env::var("NCHANNELS").map_or(1, |s| s.parse().unwrap());
    let bench_conf: String = std::env::var("BENCH_CONF").unwrap();
    let sonic_conf: String = std::env::var("SONIC_CONF").unwrap();

    let bench_confs = bench_conf.split(",").map(|name| {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("benches/configs/bench")
            .join(name)
            .with_extension("toml");
        if !path.exists() {
            panic!("{path:?} doesn’t exist.")
        };
        (name, path)
    });
    let sonic_confs = sonic_conf
        .split(",")
        .map(|name| {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("benches/configs/sonic")
                .join(name)
                .with_extension("toml");
            if !path.exists() {
                panic!("{path:?} doesn’t exist.")
            };
            (name, path)
        })
        .collect::<Vec<_>>();

    let log_file_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("benches/results.md");
    let exists = log_file_path.exists();
    let mut log_file = std::fs::File::options()
        .create(true)
        .append(true)
        .open(&log_file_path)
        .unwrap();
    if !exists {
        writeln!(
            log_file,
            "| data size | bench conf | sonic conf | channels | ingest | compact | consolidate | thrpt | thrpt |\n\
             | ---------:| ---------- | ---------- | --------:| ------:| -------:| -----------:| -----:| -----:|",
        )
        .unwrap();
    }

    for (bench_conf_name, bench_conf_path) in bench_confs {
        for (sonic_conf_name, sonic_conf_path) in sonic_confs.iter() {
            let config = config::Config::builder()
                .add_source(config::File::new(
                    bench_conf_path.to_str().unwrap(),
                    config::FileFormat::Toml,
                ))
                .build()
                .unwrap()
                .try_deserialize::<ParallelBenchmarkConfig>()
                .unwrap();

            let bench_name =
                format!("[bench={bench_conf_name}][sonic={sonic_conf_name}][channels={nchannels}]");
            group.bench_function(BenchmarkId::new("push", &bench_name), |b| {
                b.iter_custom(|iters| {
                    let mut elapsed_total = Duration::ZERO;

                    for _i in 0..iters {
                        let sonic = start_sonic_empty(|command| command.arg("-c").arg(sonic_conf_path));

                        let multiplexer = Arc::new(SonicMultiplexer::new().unwrap());

                        const COLLECTION: &str = "wikipedia";
                        const BUCKET: &str = "default";

                        if config != ParallelBenchmarkConfig::default() {
                            tracing::info!("Setting dynamic configuration…");

                            let control = SonicChannelControlBlocking::connect(ADDR, SONIC_PASSWORD, &multiplexer).unwrap();

                            let mut args = Vec::with_capacity(3);
                            args.push(format!("rocksdb.disable_auto_compactions={}", config.defer_compaction));
                            if let Some(unordered_write) = config.rocksdb_unordered_write {
                                args.push(format!("rocksdb.unordered_write={unordered_write}"));
                            }
                            if let Some(ref memtable) = config.rocksdb_memtable {
                                args.push(format!("rocksdb.memtable={memtable}"));
                            }

                            control.config_set(COLLECTION, &args).unwrap();

                            drop(control);
                        }

                        tracing::info!("Ingesting…");

                        let articles = Arc::new(Mutex::new(articles()));

                        let (mut ingest_duration, ingested_count, ingested_bytes) = (0..nchannels)
                            .map(|i| {
                                std::thread::Builder::new().name(format!("thread-{i}")).spawn({
                                    let articles = Arc::clone(&articles);
                                    let multiplexer = Arc::clone(&multiplexer);

                                    move || {
                                        let mut channel = SonicChannelIngestBlocking::connect(
                                            ADDR,
                                            SONIC_PASSWORD,
                                            &multiplexer,
                                        ).unwrap();
                                        // println!("Opened Sonic channel");

                                        // Ensure Sonic is running fine.
                                        channel.ping().unwrap();

                                        let mut ingested_count = 0usize;
                                        let mut ingested_bytes = 0u32;

                                        /// Helper function which returns the next iterator element without keeping the lock guard alive.
                                        /// When put on a single line (e.g. in a `while` loop), the lock guard stays alive all the time,
                                        /// preventing parallelism.
                                        fn next(mutex: &Mutex<impl Iterator<Item = WikipediaArticle>>) -> Option<WikipediaArticle> {
                                            let mut lock = mutex.lock().unwrap();
                                            let next = lock.next();
                                            drop(lock);
                                            next
                                        }

                                        let start = Instant::now();
                                        while let Some(article) = next(&articles) {
                                            let len = article.text.as_bytes().len();

                                            match black_box(channel.push_with_options(COLLECTION, BUCKET, article.id, article.text, &[&Lang("eng")])) {
                                                Ok(()) => {
                                                    if show_progress {
                                                        eprint!("{}", size_char(len));
                                                    }

                                                    ingested_count += 1;
                                                    ingested_bytes += len as u32;
                                                }
                                                Err(err) => {
                                                    panic!(
                                                        "Failed ingesting {title:?} ({len:.2}) after {ingested_count} success(es) ({ingested_bytes:.2}): {err}",
                                                        title = article.title,
                                                        len = HumanBytes::from(len as u64),
                                                        ingested_bytes = HumanBytes::from(ingested_bytes),
                                                    );
                                                }
                                            };
                                        }
                                        let elapsed = start.elapsed();

                                        channel.quit().unwrap();
                                        drop(channel);

                                        (elapsed, ingested_count, ingested_bytes)
                                    }
                                }).unwrap()
                            })
                            // WARN: This `collect` is important, as it is what spawns the threads!
                            .collect::<Vec<_>>()
                            .into_iter()
                            .map(|h| h.join().expect("thread panicked"))
                            .fold((Duration::ZERO, 0, 0), |(a, b, c), (x, y, z)| {
                                (a + x, b + y, c + z)
                            });

                        ingest_duration = ingest_duration / (nchannels as u32);

                        elapsed_total += ingest_duration;

                        tracing::info!("Ingested {ingested_count} articles ({size:.2}) in {ingest_duration:.3?}.", size = HumanBytes::from(ingested_bytes));

                        let (compact_duration, consolidate_duration) = {
                            let mut channel = SonicChannelControlBlocking::connect(ADDR, SONIC_PASSWORD, &multiplexer).unwrap();

                            let compact_duration = {
                                tracing::info!("Compacting KV…");

                                let start = Instant::now();

                                black_box(trigger_compact(&channel, &[COLLECTION])).unwrap();

                                let compact_duration = start.elapsed();
                                elapsed_total += compact_duration;

                                tracing::info!("Compacted KV in {compact_duration:.3?}.");

                                compact_duration
                            };

                            let consolidate_duration = {
                                tracing::info!("Consolidating FST…");

                                let start = Instant::now();

                                black_box(channel.trigger_consolidate()).unwrap();

                                let consolidate_duration = start.elapsed();
                                elapsed_total += consolidate_duration;

                                tracing::info!("Consolidated FST in {consolidate_duration:.3?}.");

                                consolidate_duration
                            };

                            channel.quit().unwrap();
                            drop(channel);

                            (compact_duration, consolidate_duration)
                        };

                        if config != ParallelBenchmarkConfig::default() {
                            tracing::info!("Resetting dynamic configuration…");

                            let control = SonicChannelControlBlocking::connect(ADDR, SONIC_PASSWORD, &multiplexer).unwrap();

                            control.config_reset_all(COLLECTION).unwrap();

                            drop(control);
                        }

                        drop(sonic);

                        writeln!(
                            log_file,
                            "| {size:.2} | `{bench_conf_name}` | `{sonic_conf_name}` | {nchannels} | {ingest_duration:>9.3?} | {compact_duration:>9.3?} | {consolidate_duration:>9.3?} | {thrpt_bytes:>7.3}/s | {thrpt_elem:>7.3}elem/s |",
                            size = HumanBytes::from(ingested_bytes),
                            bench_conf_name = bench_conf_name.replace("-", "` `"),
                            sonic_conf_name = sonic_conf_name.replace("-", "` `"),
                            thrpt_bytes = HumanBytes::from((ingested_bytes as f32 / elapsed_total.as_secs_f32()) as u64),
                            thrpt_elem = CompactThousands::from((ingested_count as f32 / elapsed_total.as_secs_f32()) as u64)
                        ).unwrap();
                    }

                    elapsed_total
                });
            });
        }
    }

    group.finish();

    tracing::info!("Logged results in {log_file_path:?}");
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
