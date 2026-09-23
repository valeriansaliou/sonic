// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;
#[path = "common/huggingface/wikipedia.rs"]
mod huggingface_wikipedia;
mod parallelism_common;
mod wikipedia_common;

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

use crate::common::huggingface::NSHARDS;
use crate::common::huggingface::download::download_shards;
use crate::common::huggingface::load::iter_shard;
use crate::common::logging::{CompactThousands, HumanBytes};
use crate::common::prelude::*;
use crate::huggingface_wikipedia::WikipediaArticle;
use crate::parallelism_common::*;

static SHARD_PATHS: LazyLock<Vec<PathBuf>> =
    LazyLock::new(|| download_shards("wikimedia/wikipedia", "20231101.en", Some(*NSHARDS)));

fn articles_iter() -> impl Iterator<Item = WikipediaArticle> {
    SHARD_PATHS.iter().flat_map(iter_shard::<WikipediaArticle>)
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

    let mut group = c.benchmark_group("wikipedia_parallel");

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
    group.measurement_time(Duration::from_secs(30));

    let total_bytes = articles()
        .map(|article| article.text.len() as u64)
        .sum::<u64>();
    let articles_count = articles().count();
    group.throughput(Throughput::ElementsAndBytes {
        elements: articles_count as u64,
        bytes: total_bytes,
    });

    let nchannels = *NCHANNELS;
    let bench_confs = BENCH_CONFS.iter();
    let sonic_confs = &*SONIC_CONFS;

    let log_file_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("results.md");
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
            let config = ParallelBenchmarkConfig::from_path(bench_conf_path);

            let bench_name =
                format!("[bench={bench_conf_name}][sonic={sonic_conf_name}][channels={nchannels}]");
            group.bench_function(BenchmarkId::new("push", &bench_name), |b| {
                b.iter_custom(|iters| {
                    let mut elapsed_total = Duration::ZERO;

                    for _i in 0..iters {
                        let stats = ingest_parallel(&bench_name, &config, sonic_conf_path, nchannels, articles(), articles_count);

                        elapsed_total += stats.elapsed_total;

                        writeln!(
                            log_file,
                            "| {size:.2} | `{bench_conf_name}` | `{sonic_conf_name}` | {nchannels} | {ingest_duration:>9.3?} | {compact_duration:>9.3?} | {consolidate_duration:>9.3?} | {thrpt_bytes:>7.3}/s | {thrpt_elem:>7.3}elem/s |",
                            size = HumanBytes::from(stats.ingested_bytes),
                            bench_conf_name = bench_conf_name.replace("-", "` `"),
                            sonic_conf_name = sonic_conf_name.replace("-", "` `"),
                            ingest_duration = stats.ingest_duration,
                            compact_duration = stats.compact_duration,
                            consolidate_duration = stats.consolidate_duration,
                            thrpt_bytes = HumanBytes::from((stats.ingested_bytes as f32 / stats.elapsed_total.as_secs_f32()) as u64),
                            thrpt_elem = CompactThousands::from((stats.ingested_count as f32 / stats.elapsed_total.as_secs_f32()) as u64)
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
