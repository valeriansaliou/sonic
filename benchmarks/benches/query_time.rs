// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;
mod parallelism_common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;
use std::sync::atomic::{self, AtomicU8};
use std::time::{Duration, Instant};

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use sonic_benchmarks::huggingface::NSHARDS;
use sonic_benchmarks::huggingface::download::download_shards;
use sonic_benchmarks::huggingface::load::iter_shard;
use sonic_benchmarks::huggingface::wikipedia::WikipediaArticle;
use sonic_benchmarks::prelude::*;
use sonic_benchmarks::profiling::PROFILING_MODE;

use crate::common::{BENCH_DATA_PATH, SONIC_BENCH_PASSWORD, profiling_output};
use crate::parallelism_common::*;

const COLLECTION: &str = "wikipedia";

static SHARD_PATHS: LazyLock<Vec<PathBuf>> = LazyLock::new(|| {
    download_shards(
        "wikimedia/wikipedia",
        "20231101.en",
        Some(constant!(NSHARDS, default: 16)),
    )
});

fn articles_iter() -> impl Iterator<Item = WikipediaArticle> {
    SHARD_PATHS.iter().flat_map(iter_shard::<WikipediaArticle>)
}

fn criterion_benchmark(c: &mut Criterion) {
    init_logging(
        Some(concat!(env!("CARGO_CRATE_NAME"), "=info")),
        LoggingOptions {
            default_log_level: tracing::Level::WARN,
            with_target: true,
            with_file: false,
            with_line_number: false,
            with_level: true,
            with_thread_ids: false,
        },
    );

    let is_quick = std::env::args().any(|arg| arg == "--quick");

    let mut group = c.benchmark_group("query_time");

    // No need to warm up for 3 seconds (default).
    group.warm_up_time(Duration::from_secs(1));

    let articles = || {
        articles_iter()
        // .filter(|a| a.text.as_bytes().len() > 2000)
        // .filter(|a| a.text.as_bytes().len() < 8000)
        // .filter(|a| a.text.as_bytes().len() > 20000)
        // .take(20000)
    };

    // Lower sample size as what we’re measuring is quite long to execute.
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(if is_quick { 10 } else { 30 }));

    let total_bytes = articles()
        .map(|article| article.text.len() as u64)
        .sum::<u64>();
    let articles_count = articles().count();

    let mut bench_confs = BENCH_CONFS.iter();
    let (_bench_conf_name, bench_conf_path) =
        bench_confs.next().expect("BENCH_CONFS has a default value");
    if bench_confs.next().is_some() {
        panic!(
            "{} doesn’t support using multiple bench configurations at once",
            env!("CARGO_CRATE_NAME")
        )
    }
    let config = ParallelBenchmarkConfig::from_path(bench_conf_path);

    let sonic_confs = &*SONIC_CONFS;
    let ((sonic_conf_name, sonic_conf_path), sonic_confs) = sonic_confs
        .split_first()
        .expect("SONIC_CONFS has a default value");
    if !sonic_confs.is_empty() {
        panic!(
            "{} doesn’t support using multiple Sonic configurations at once",
            env!("CARGO_CRATE_NAME")
        )
    }

    let mut ctx = RunContext::new(false);

    let data_path = Path::new(BENCH_DATA_PATH);

    if !data_path.exists() {
        std::fs::create_dir_all(&data_path).unwrap();

        ctx.start_sonic(data_path, SONIC_BENCH_PASSWORD.to_owned(), |command| {
            command
                .arg("-c")
                .arg(sonic_conf_path)
                .env("SONIC_SERVER__LOG_LEVEL", "error")
        });

        ctx.wait_until_ready();

        unsafe {
            std::env::set_var("SHOW_PROGRESS_SUMMARY", "false");
            std::env::set_var("SHOW_STATS", "false");
        }

        ingest_parallel_inner(
            &mut ctx,
            &config,
            COLLECTION,
            *NCHANNELS,
            articles(),
            articles_count,
            total_bytes,
            constant!(NBUCKETS, default: 1),
        );

        // Stop Sonic so benchmarks run on a fresh instance of Sonic with no
        // pre-filled cache or whatever, no matter if we had to ingest first.
        ctx.stop_sonic();
    }

    ctx.start_sonic(data_path, SONIC_BENCH_PASSWORD.to_owned(), |command| {
        command.arg("-c").arg(sonic_conf_path)
    });

    ctx.wait_until_ready();

    let bucket = {
        let stdout_bytes = Command::new("find")
            .args([".", "-name", "*.fst"])
            .current_dir(data_path)
            .output()
            .unwrap()
            .stdout;
        let stdout_str = std::str::from_utf8(&stdout_bytes).unwrap();
        let path = stdout_str.split('\n').next().unwrap();
        let name = path.rsplit('/').next().unwrap();
        name.split('.').next().unwrap().to_owned()
    };

    if let Some(profiling_mode) = PROFILING_MODE.as_ref() {
        ctx.start_profiling(profiling_mode, profiling_output(Some("query_time")));
    }

    let search =
        SonicChannelSearchBlocking::connect(ctx.addr, SONIC_BENCH_PASSWORD, &ctx.multiplexer)
            .unwrap();

    {
        let bench_name = format!("[sonic={sonic_conf_name}]");
        group.bench_function(BenchmarkId::new("query", &bench_name), |b| {
            b.iter_custom(|iters| {
                let mut elapsed_total = Duration::ZERO;

                for _i in 0..iters {
                    let start = Instant::now();

                    // 6723
                    let query = &articles().skip(6723).next().unwrap().text[..32];
                    let json = search
                        .send_async(
                            sonic_client::Command::new(
                                format!("QUERY {COLLECTION} {bucket} {query:?} WITH SNIPPETS")
                                    .into_boxed_str(),
                                0,
                                0,
                            ),
                            sonic_client::search::Discriminant::Pending,
                            |id| sonic_client::search::Discriminant::EventQuery(Box::from(id)),
                            |data| Ok(data.to_owned()),
                        )
                        .expect(&format!("Query failed: {query:?}"));

                    let elasped = start.elapsed();

                    static COUNTER: AtomicU8 = AtomicU8::new(0);
                    if COUNTER
                        .try_update(atomic::Ordering::SeqCst, atomic::Ordering::SeqCst, |n| {
                            n.checked_add(1)
                        })
                        .is_ok_and(|prev| prev < 5)
                    {
                        tracing::info!(?query, ?elasped, "Got snippets: {json}");
                    }

                    elapsed_total += elasped;
                }

                elapsed_total
            });
        });
    }

    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
