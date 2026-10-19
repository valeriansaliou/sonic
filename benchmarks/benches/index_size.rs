// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;
mod parallelism_common;

use std::path::PathBuf;
use std::sync::LazyLock;

use sonic_benchmarks::huggingface::NSHARDS;
use sonic_benchmarks::huggingface::download::download_shards;
use sonic_benchmarks::huggingface::load::iter_shard;
use sonic_benchmarks::huggingface::wikipedia::WikipediaArticle;
use sonic_benchmarks::prelude::*;
use sonic_benchmarks::util::logging::{LoggingOptions, init_logging};

use crate::parallelism_common::*;

const COLLECTION: &str = "wikipedia";

static SHARD_PATHS: LazyLock<Vec<PathBuf>> =
    LazyLock::new(|| download_shards("wikimedia/wikipedia", "20231101.en", Some(*NSHARDS)));

fn articles_iter() -> impl Iterator<Item = WikipediaArticle> {
    SHARD_PATHS.iter().flat_map(iter_shard::<WikipediaArticle>)
}

fn main() {
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

    let articles = || {
        articles_iter()
        // .filter(|a| a.text.as_bytes().len() > 2000)
        // .filter(|a| a.text.as_bytes().len() < 8000)
        // .filter(|a| a.text.as_bytes().len() > 20000)
        // .take(20000)
    };

    let total_bytes = articles()
        .map(|article| article.text.len() as u64)
        .sum::<u64>();
    let articles_count = articles().count();

    let nchannels = *NCHANNELS;
    let bench_confs = BENCH_CONFS.iter();
    let sonic_confs = &*SONIC_CONFS;

    unsafe { std::env::set_var("SHOW_STORE_SIZE", "true") };

    for (bench_conf_name, bench_conf_path) in bench_confs {
        for (sonic_conf_name, sonic_conf_path) in sonic_confs.iter() {
            let config = ParallelBenchmarkConfig::from_path(bench_conf_path);

            let bench_name =
                format!("[bench={bench_conf_name}][sonic={sonic_conf_name}][channels={nchannels}]");

            ingest_parallel(
                &bench_name,
                &config,
                sonic_conf_path,
                COLLECTION,
                nchannels,
                articles(),
                articles_count,
                total_bytes,
            );
        }
    }
}
