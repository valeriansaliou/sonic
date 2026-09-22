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

use std::path::PathBuf;
use std::sync::LazyLock;

use crate::common::huggingface::NSHARDS;
use crate::common::huggingface::download::download_shards;
use crate::common::huggingface::load::iter_shard;
use crate::common::prelude::*;
use crate::huggingface_wikipedia::WikipediaArticle;
use crate::parallelism_common::*;

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
                nchannels,
                articles(),
                articles_count,
            );
        }
    }
}
