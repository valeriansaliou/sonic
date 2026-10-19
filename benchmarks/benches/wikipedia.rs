// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;

use std::hint::black_box;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use sonic_benchmarks::huggingface::download::download_shards;
use sonic_benchmarks::huggingface::load::iter_shard;
use sonic_benchmarks::huggingface::wikipedia::WikipediaArticle;
use sonic_benchmarks::huggingface::wikipedia_common::*;
use sonic_benchmarks::prelude::*;

use crate::common::{BENCH_DATA_PATH, SONIC_BENCH_PASSWORD};

static SHARD_PATHS: LazyLock<Vec<PathBuf>> =
    LazyLock::new(|| download_shards("wikimedia/wikipedia", "20231101.simple", None));

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

    let mut group = c.benchmark_group("wikipedia");

    // No need to warm up for 3 seconds (default).
    group.warm_up_time(Duration::from_secs(1));

    for config in [
        PushBenchmarkConfig::default(),
        PushBenchmarkConfig {
            diacritic_folding_enabled: Some(false),
        },
        PushBenchmarkConfig {
            diacritic_folding_enabled: Some(true),
        },
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
                    let mut ctx = RunContext::new(true);
                    ctx.start_sonic(&new_empty_data_path(BENCH_DATA_PATH, ctx.id), SONIC_BENCH_PASSWORD.to_owned(), |command| config.update_command(command));
                    ctx.wait_until_ready();

                    let multiplexer = SonicMultiplexer::new().unwrap();

                    {
                        let mut channel = SonicChannelIngestBlocking::connect(
                            ctx.addr,
                            SONIC_BENCH_PASSWORD,
                            &multiplexer,
                        ).unwrap();
                        // println!("Opened Sonic channel");

                        // Ensure Sonic is running fine.
                        channel.ping().unwrap();

                        let mut ingested_count = 0usize;
                        let mut ingested_bytes = 0u32;

                        let start = Instant::now();
                        for article in articles() {
                            let len = article.text.as_bytes().len();

                            match black_box(channel.push_with_options("wikipedia", "default", article.id, article.text, &[&Lang("eng")])) {
                                Ok(()) => {
                                    if show_progress {
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
                        elapsed_total += elapsed;

                        channel.quit().unwrap();
                        drop(channel);

                        println!("Ingested {ingested_count} articles ({ingested_bytes}B) in {elapsed:.3?}.");
                    }

                    {
                        let mut channel = SonicChannelControlBlocking::connect(ctx.addr, SONIC_BENCH_PASSWORD, &multiplexer).unwrap();

                        let start = Instant::now();

                        black_box(channel.trigger_consolidate()).unwrap();

                        let elapsed = start.elapsed();
                        elapsed_total += elapsed;

                        channel.quit().unwrap();
                        drop(channel);

                        println!("Consolidated in {elapsed:.3?}.");
                    }

                    drop(ctx);
                }

                elapsed_total
            });
        });
    }

    for count in [10, 100, 1000] {
        let articles = || articles_iter(count);

        // Lower sample size as what we’re measuring is quite stable.
        group.sample_size(10);
        group.measurement_time(Duration::from_secs(15));
        group.throughput(Throughput::Elements(count as u64));

        group.bench_function(BenchmarkId::new("consolidate", format!("count-{count}")), |b| {
            b.iter_custom(|iters| {
                let mut elapsed_total = Duration::ZERO;

                for _i in 0..iters {
                    let mut ctx = RunContext::new(true);
                    ctx.start_sonic(&new_empty_data_path(BENCH_DATA_PATH, ctx.id), SONIC_BENCH_PASSWORD.to_owned(), |command| command
                        .env("SONIC_STORE__FST__GRAPH__CONSOLIDATE_AFTER", "3600")
                        .env("SONIC_STORE__FST__POOL__INACTIVE_AFTER", "3700")
                        .env("SONIC_STORE__KV__DATABASE__FLUSH_AFTER", "3600")
                        .env("SONIC_STORE__KV__POOL__INACTIVE_AFTER", "3700"));
                    ctx.wait_until_ready();

                    let multiplexer = SonicMultiplexer::new().unwrap();

                    {
                        let mut channel = SonicChannelIngestBlocking::connect(
                            ctx.addr, SONIC_BENCH_PASSWORD,
                            &multiplexer,
                        ).unwrap();
                        // println!("Opened Sonic channel");

                        // Ensure Sonic is running fine.
                        channel.ping().unwrap();

                        let mut ingested_count = 0usize;
                        let mut ingested_bytes = 0u32;

                        let start = Instant::now();
                        for article in articles() {
                            let len = article.text.as_bytes().len();

                            match black_box(channel.push_with_options("wikipedia", "default", article.id, article.text, &[&Lang("eng")])) {
                                Ok(()) => {
                                    if show_progress {
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

                        println!("Ingested {ingested_count} articles ({ingested_bytes}B) in {elapsed:.3?}.");
                    }

                    {
                        let mut channel = SonicChannelControlBlocking::connect(ctx.addr, SONIC_BENCH_PASSWORD, &multiplexer).unwrap();

                        let start = Instant::now();

                        black_box(channel.trigger_consolidate()).unwrap();

                        let elapsed = start.elapsed();
                        elapsed_total += elapsed;

                        channel.quit().unwrap();
                        drop(channel);

                        println!("Consolidated in {elapsed:.3?}.");
                    }

                    drop(ctx);
                }

                elapsed_total
            });
        });
    }

    let queries = [
        "photography art",
        "basque country",
        "autonomous communities",
        "horses sheep goats",
        "theoretical astronomy",
        "archaeological fieldwork",
    ];

    // Lower sample size as what we’re measuring is quite stable.
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(10));
    group.throughput(Throughput::Elements(queries.len() as u64));

    for limit in [10, 100] {
        group.bench_with_input(
            BenchmarkId::new("query", format!("limit-{limit}")),
            &limit,
            |b, &limit| {
                let multiplexer = SonicMultiplexer::new().unwrap();

                let mut ctx = RunContext::new(true);
                let data_path = new_empty_data_path(BENCH_DATA_PATH, ctx.id);
                start_sonic_prepopulated(
                    &mut ctx,
                    &data_path,
                    SONIC_BENCH_PASSWORD.to_owned(),
                    &multiplexer,
                    NormalizationConfig {
                        diacritic_folding_enabled: Some(true),
                    },
                    |command| command.env("SONIC_SEARCH__QUERY_LIMIT_DEFAULT", limit.to_string()),
                    || articles_iter(10000),
                );
                ctx.wait_until_ready();

                b.iter_custom(|iters| {
                    let mut elapsed_total = Duration::ZERO;
                    for _i in 0..iters {
                        let mut channel =
                            SonicChannelSearchBlocking::connect(ctx.addr, SONIC_BENCH_PASSWORD, &multiplexer)
                                .unwrap();
                        // println!("Opened Sonic channel");

                        let mut query_count = 0usize;

                        let start = Instant::now();
                        eprint!("Result counts: ");
                        for query in queries {
                            match black_box(channel.query_with_options(
                                "wikipedia",
                                "default",
                                query,
                                &[&Lang("eng")],
                            )) {
                                Ok(res) => {
                                    assert!(!res.is_empty());
                                    eprint!("{query:?}: {}, ", res.len());
                                    // eprintln!("\nQuery: {query}");
                                    // eprintln!("Result count: {}", res.len());
                                    // eprintln!("Result IDs: {res:?}");
                                    // eprintln!("Results: {:?}", res.into_iter().map(|id| search_articles().find(|a| a.id.as_str().eq(id.as_ref())).unwrap().title).collect::<Vec<_>>());

                                    query_count += 1;
                                }
                                Err(err) => {
                                    panic!(
                                        "Failed querying {query:?} after {query_count} success(es): {err}",
                                    );
                                }
                            };
                        }
                        eprint!("\n");
                        let elapsed = start.elapsed();
                        elapsed_total += elapsed;

                        channel.quit().unwrap();
                        drop(channel);

                        println!("Ran {query_count} queries in {elapsed:.3?}.");
                    }

                    elapsed_total
                });

                drop(ctx);
            },
        );
    }

    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
