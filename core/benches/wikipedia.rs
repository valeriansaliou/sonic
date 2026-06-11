// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use sonic::{
    Executor,
    config::ConfigNormalization,
    lexer::TokenLexerBuilder,
    store::{
        fst::{StoreFSTActionConfig, StoreFSTPool},
        kv::StoreKVPool,
    },
};
use std::{hint::black_box, sync::Arc, time::Duration};
use whatlang::Lang;

use crate::common::huggingface::download_shards;
use crate::common::huggingface::iter_shard;
use crate::common::init_logging;
use crate::common::{
    config::{defaults_toml, make_config},
    huggingface::WikipediaArticle,
};

fn criterion_benchmark(c: &mut Criterion) {
    init_logging();
    let shard_paths = download_shards("wikimedia/wikipedia", "20231101.simple");

    let mut group = c.benchmark_group("wikipedia");

    // Lower sample size as what we’re measuring is quite long to execute.
    group.sample_size(10);

    // No need to warm up for 3 seconds (default).
    group.warm_up_time(Duration::from_secs(1));

    let limit: usize = 10_000;

    let articles = || {
        shard_paths
            .iter()
            .flat_map(iter_shard::<WikipediaArticle>)
            .take(limit)
    };

    let total_bytes = articles()
        .map(|article| article.text.as_bytes().len() as u64)
        .sum();
    group.throughput(Throughput::Bytes(total_bytes));

    group.bench_function("ingest", |b| {
        let config = Arc::new(make_config(&defaults_toml()));

        b.iter_batched(
            || Executor {
                app_conf: Arc::clone(&config),
                kv_pool: StoreKVPool::new(Arc::clone(&config.store.kv)),
                fst_pool: StoreFSTPool::new(
                    Arc::clone(&config.store.fst),
                    StoreFSTActionConfig {
                        prefix_matching_enabled: true,
                        fuzzy_matching_enabled: true,
                    },
                ),
            },
            |executor| {
                let mut count = 0usize;

                let normalization_config = ConfigNormalization {
                    diacritic_folding_enabled: true,
                    stemming_enabled: false,
                };

                for article in articles() {
                    executor
                        .push(
                            sonic::store::StoreItemBuilder::from_depth_3(
                                "wikipedia",
                                "default",
                                &black_box(article.id),
                            )
                            .unwrap(),
                            TokenLexerBuilder::from(
                                sonic::lexer::TokenLexerMode::NormalizeAndCleanup,
                                Some(Lang::Eng),
                                &article.text,
                                normalization_config,
                            )
                            .unwrap(),
                        )
                        .unwrap();

                    count += 1;
                }

                executor.fst_pool.consolidate(true);

                println!("Ingested {count} articles.");
            },
            criterion::BatchSize::SmallInput,
        )
    });

    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
