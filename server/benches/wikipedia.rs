// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;

use std::time::Instant;
use std::{hint::black_box, process::Command, time::Duration};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use sonic_channel::*;

use crate::common::SpawnGuard;
use crate::common::huggingface::WikipediaArticle;
use crate::common::huggingface::download_shards;
use crate::common::huggingface::iter_shard;

fn criterion_benchmark(c: &mut Criterion) {
    let sonic_bin_path = concat!(env!("CARGO_TARGET_TMPDIR"), "/../release/sonic");
    // let sonic_config_path = concat!(env!("CARGO_MANIFEST_DIR"), "/benches/config.cfg");

    let shard_paths = download_shards("wikimedia/wikipedia", "20231101.simple");

    let mut group = c.benchmark_group("wikipedia");

    // Lower sample size as what we’re measuring is quite long to execute.
    group.sample_size(10);

    // No need to warm up for 3 seconds (default).
    group.warm_up_time(Duration::from_secs(1));

    let limit: usize = 1;

    let articles = || {
        shard_paths
            .iter()
            .flat_map(iter_shard::<WikipediaArticle>)
            .filter(|a| a.text.as_bytes().len() < 8000)
            .take(limit)
    };

    let total_bytes = articles()
        .map(|article| article.text.as_bytes().len() as u64)
        .sum();
    group.throughput(Throughput::Bytes(total_bytes));

    group.bench_function("ingest", |b| {
        b.iter_custom(|iters| {
            let mut elapsed_total = Duration::ZERO;

            for _i in 0..iters {
                let sonic =
                    Command::new(sonic_bin_path)
                        // .args(["-c", sonic_config_path])
                        .env("SONIC_SERVER__LOG_LEVEL", "TRACE")
                        .env("SONIC_NORMALIZATION__DIACRITIC_FOLDING_ENABLED", "true")
                        .spawn()
                        .unwrap();

                // Auto-kill Sonic.
                let mut sonic = SpawnGuard(sonic);

                std::thread::sleep(Duration::from_millis(500));
                if let Some(status) = sonic.try_wait().unwrap() {
                    panic!("Sonic exited with {status}")
                };
                // println!("Started Sonic");

                let channel = IngestChannel::start(
                    "localhost:1491",
                    "SecretPassword",
                ).unwrap();
                // println!("Opened Sonic channel");

                // Ensure Sonic is running fine.
                // () = channel.ping().unwrap(); // `sonic-channel` doesn’t support PONG…

                let mut ingested_count = 0usize;
                let mut ingested_bytes = 0u32;

                let start = Instant::now();
                for article in articles() {
                    let len = article.text.as_bytes().len();

                    let dest = Dest::col_buc("wikipedia", "default").obj(article.id);
                    match black_box(channel.push(PushRequest { dest, text: article.text, lang: Some(sonic_channel::Lang::Eng) })) {
                        Ok(()) => {
                            print!(".");

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

                // {
                //     let mut channel = ControlChan::new("[::1]", 1491, "SecretPassword").unwrap();
                //     channel.connect().unwrap();

                //     let start = Instant::now();

                //     let receiver = channel.trigger(Some("consolidate")).unwrap();
                //     receiver.recv().unwrap();

                //     let elapsed = start.elapsed();
                //     elapsed_total += elapsed;

                //     channel.quit().unwrap();
                //     drop(channel);

                //     println!("Consolidated in {elapsed:.3?}.");
                // }

                // TODO: Consolidate?

                drop(sonic);
            }

            elapsed_total
        });
    });

    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
