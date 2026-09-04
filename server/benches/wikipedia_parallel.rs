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
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

use crate::common::huggingface::download::download_shards;
use crate::common::huggingface::load::iter_shard;
use crate::common::prelude::*;
use crate::huggingface_wikipedia::WikipediaArticle;
use crate::wikipedia_common::*;

static SHARD_PATHS: LazyLock<Vec<PathBuf>> =
    LazyLock::new(|| download_shards("wikimedia/wikipedia", "20231101.simple"));

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

    let no_progress = *NO_PROGRESS;

    let mut group = c.benchmark_group("wikipedia_parallel");

    // No need to warm up for 3 seconds (default).
    group.warm_up_time(Duration::from_secs(1));

    for config in [
        ParallelBenchmarkConfig { nthreads: 1 },
        ParallelBenchmarkConfig { nthreads: 2 },
        ParallelBenchmarkConfig { nthreads: 3 },
        ParallelBenchmarkConfig { nthreads: 4 },
        ParallelBenchmarkConfig { nthreads: 6 },
    ] {
        let articles = || articles_iter(usize::MAX);

        // Lower sample size as what we’re measuring is quite long to execute.
        group.sample_size(10);
        group.measurement_time(Duration::from_secs(30));

        let total_bytes = articles().map(|article| article.text.len() as u64).sum();
        group.throughput(Throughput::Bytes(total_bytes));

        group.bench_function(BenchmarkId::new("push", config), |b| {
            b.iter_custom(|iters| {
                let mut elapsed_total = Duration::ZERO;

                for _i in 0..iters {
                    let sonic = start_sonic_empty(|command| config.update_command(command));

                    let multiplexer = Arc::new(SonicMultiplexer::new().unwrap());

                    let articles = Arc::new(Mutex::new(articles()));

                    let (mut elapsed, ingested_count, ingested_bytes) = (0..config.nthreads)
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

                                        match black_box(channel.push_with_options("wikipedia", "default", article.id, article.text, &[&Lang("eng")])) {
                                            Ok(()) => {
                                                if !no_progress {
                                                    eprint!("{}", size_char(len));
                                                }

                                                ingested_count += 1;
                                                ingested_bytes += len as u32;
                                            }
                                            Err(err) => {
                                                panic!(
                                                    "Failed ingesting {:?} ({len}B) after {ingested_count} success(es) ({ingested_bytes}B): {err}",
                                                    article.title,
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

                    elapsed = elapsed / (config.nthreads as u32);

                    elapsed_total += elapsed;

                    println!("Ingested {ingested_count} articles ({ingested_bytes}B) in {elapsed:.3?}.");

                    {
                        let mut channel = SonicChannelControlBlocking::connect(ADDR, SONIC_PASSWORD, &multiplexer).unwrap();

                        let start = Instant::now();

                        black_box(channel.trigger_consolidate()).unwrap();

                        let elapsed = start.elapsed();
                        elapsed_total += elapsed;

                        channel.quit().unwrap();
                        drop(channel);

                        println!("Consolidated in {elapsed:.3?}.");
                    }

                    drop(sonic);
                }

                elapsed_total
            });
        });
    }

    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
