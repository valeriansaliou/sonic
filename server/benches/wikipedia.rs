// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;

use std::collections::HashMap;
use std::hint::black_box;
use std::net::Ipv6Addr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{LazyLock, RwLock};
use std::time::{Duration, Instant};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use sonic_client::SonicMultiplexer;
use sonic_client::control::SonicChannelControlBlocking;
use sonic_client::ingest::SonicChannelIngestBlocking;
use sonic_client::options::*;
use sonic_client::search::SonicChannelSearchBlocking;

use crate::common::huggingface::download::download_shards;
use crate::common::huggingface::load::iter_shard;
use crate::common::huggingface::wikipedia::WikipediaArticle;
use crate::common::spawn_guard::SpawnGuard;

const ADDR: (Ipv6Addr, u16) = (Ipv6Addr::LOCALHOST, 1491);

// NOTE: We initialize `SONIC_BIN_PATH` lazily to avoid logging the
//   “Environment variable "SONIC_BIN" not found” warning on
//   `--load-baseline`.
static SONIC_BIN_PATH: LazyLock<PathBuf> = LazyLock::new(|| {
    std::env::var("SONIC_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let path = Path::new(env!("CARGO_TARGET_TMPDIR"))
                .parent()
                .unwrap()
                .join("release/sonic");
            eprintln!("Environment variable \"SONIC_BIN\" not found, using local build");
            path
        })
});

static SHARD_PATHS: LazyLock<Vec<PathBuf>> =
    LazyLock::new(|| download_shards("wikimedia/wikipedia", "20231101.simple"));

fn articles_iter(limit: usize) -> impl Iterator<Item = WikipediaArticle> {
    SHARD_PATHS
        .iter()
        .flat_map(iter_shard::<WikipediaArticle>)
        .filter(|a| a.text.as_bytes().len() > 2000)
        // .filter(|a| a.text.as_bytes().len() < 8000)
        // .filter(|a| a.text.as_bytes().len() > 20000)
        .take(limit)
}

fn criterion_benchmark(c: &mut Criterion) {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .with_target(true)
        .with_file(false)
        .with_line_number(false)
        .without_time()
        .with_level(true)
        .with_writer(tracing_subscriber::fmt::TestWriter::new)
        .init();

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
        let articles = || articles_iter(1000);

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

                    let multiplexer = SonicMultiplexer::new().unwrap();

                    {
                        let mut channel = SonicChannelIngestBlocking::connect(
                            ADDR,
                            "SecretPassword",
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
                                    eprint!("{}", size_char(len));

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
                        let mut channel = SonicChannelControlBlocking::connect(ADDR, "SecretPassword", &multiplexer).unwrap();

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
                    let sonic = start_sonic_empty(|command| command
                        .env("SONIC_STORE__FST__GRAPH__CONSOLIDATE_AFTER", "3600")
                        .env("SONIC_STORE__FST__POOL__INACTIVE_AFTER", "3700")
                        .env("SONIC_STORE__KV__DATABASE__FLUSH_AFTER", "3600")
                        .env("SONIC_STORE__KV__POOL__INACTIVE_AFTER", "3700"));

                    let multiplexer = SonicMultiplexer::new().unwrap();

                    {
                        let mut channel = SonicChannelIngestBlocking::connect(
                            ADDR,
                            "SecretPassword",
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
                                    eprint!("{}", size_char(len));

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
                        let mut channel = SonicChannelControlBlocking::connect(ADDR, "SecretPassword", &multiplexer).unwrap();

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

                let sonic = start_sonic_prepopulated(
                    &multiplexer,
                    ConfigNormalization {
                        diacritic_folding_enabled: Some(true),
                    },
                    |command| command.env("SONIC_SEARCH__QUERY_LIMIT_DEFAULT", limit.to_string()),
                );

                b.iter_custom(|iters| {
                    let mut elapsed_total = Duration::ZERO;
                    for _i in 0..iters {
                        let mut channel =
                            SonicChannelSearchBlocking::connect(ADDR, "SecretPassword", &multiplexer)
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

                drop(sonic);
            },
        );
    }

    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct PushBenchmarkConfig {
    diacritic_folding_enabled: Option<bool>,
}

impl PushBenchmarkConfig {
    fn update_command<'c>(&self, command: &'c mut Command) -> &'c mut Command {
        if let Some(diacritic_folding_enabled) = self.diacritic_folding_enabled {
            command.env(
                "SONIC_NORMALIZATION__DIACRITIC_FOLDING_ENABLED",
                diacritic_folding_enabled.to_string(),
            );
        }

        command
    }
}

impl std::fmt::Display for PushBenchmarkConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "default")?;

        let Self {
            diacritic_folding_enabled,
        } = self;

        if let Some(diacritic_folding_enabled) = diacritic_folding_enabled {
            if *diacritic_folding_enabled {
                write!(f, "[+diacritic_folding]")?;
            } else {
                write!(f, "[-diacritic_folding]")?;
            }
        }

        Ok(())
    }
}

const SONIC_DATA_PATH: &str = concat!(env!("CARGO_TARGET_TMPDIR"), "/bench-data");

fn new_sonic_data_path() -> PathBuf {
    let path = Path::new(SONIC_DATA_PATH).join("empty");

    if path.exists() {
        std::fs::remove_dir_all(&path).unwrap();
    }

    path
}

fn start_sonic_empty(update_command: impl FnOnce(&mut Command) -> &mut Command) -> SpawnGuard {
    let data_path = new_sonic_data_path();

    start_sonic(&data_path, update_command)
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConfigNormalization {
    pub diacritic_folding_enabled: Option<bool>,
}

fn start_sonic_prepopulated(
    multiplexer: &SonicMultiplexer,
    normalization_config: ConfigNormalization,
    update_command: impl for<'a> FnOnce(&'a mut Command) -> &'a mut Command,
) -> SpawnGuard {
    fn apply_normalization(
        normalization_config: ConfigNormalization,
        command: &mut Command,
    ) -> &mut Command {
        let ConfigNormalization {
            diacritic_folding_enabled,
        } = normalization_config;

        if let Some(diacritic_folding_enabled) = diacritic_folding_enabled {
            command.env(
                "SONIC_NORMALIZATION__DIACRITIC_FOLDING_ENABLED",
                diacritic_folding_enabled.to_string(),
            );
        }

        command
    }

    static PATHS: LazyLock<RwLock<HashMap<ConfigNormalization, PathBuf>>> =
        LazyLock::new(|| RwLock::new(HashMap::with_capacity(1)));

    // Articles used in read-only benchmarks.
    let search_articles = || articles_iter(10000);

    let mut paths = PATHS.write().unwrap();
    let path = paths
        .entry(normalization_config)
        .or_insert_with(|| {
            let data_path = Path::new(SONIC_DATA_PATH).join("prepopulated");

            let sonic = start_sonic(&data_path, |command| {
                apply_normalization(normalization_config, command)
            });

            // Ingest data.
            {
                // PUSH
                {
                    let mut channel = SonicChannelIngestBlocking::connect(
                        ADDR,
                        "SecretPassword",
                        &multiplexer,
                    ).unwrap();
                    // println!("Opened Sonic channel");

                    // Ensure Sonic is running fine.
                    channel.ping().unwrap();

                    let mut ingested_count = 0usize;
                    let mut ingested_bytes = 0u32;

                    let mut max_size = 0;

                    let start = Instant::now();
                    for article in search_articles() {
                        let len = article.text.as_bytes().len();
                        // eprintln!("\n================================");
                        // eprintln!("{}", &article.title);
                        // eprintln!("{}", &article.text[..(1000.min(article.text.len()))]);
                        max_size = max_size.max(article.text.len());

                        match black_box(channel.push_with_options("wikipedia", "default", article.id, article.text, &[&Lang("eng")])) {
                            Ok(()) => {
                                eprint!("{}", size_char(len));

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

                    println!("Ingested {ingested_count} articles ({ingested_bytes}B) in {elapsed:.3?} (max size: {max_size}).");
                }

                // TRIGGER consolidate
                {
                    let mut channel = SonicChannelControlBlocking::connect(ADDR, "SecretPassword", &multiplexer).unwrap();

                    let start = Instant::now();

                    black_box(channel.trigger_consolidate()).unwrap();

                    let elapsed = start.elapsed();

                    channel.quit().unwrap();
                    drop(channel);

                    println!("Consolidated in {elapsed:.3?}.");
                }
            }

            drop(sonic);

            data_path
        })
        .as_path();

    start_sonic(path, |command| {
        update_command(apply_normalization(normalization_config, command))
    })
}

#[must_use]
fn start_sonic(
    data_path: &Path,
    update_command: impl FnOnce(&mut Command) -> &mut Command,
) -> SpawnGuard {
    // let sonic_config_path = concat!(env!("CARGO_MANIFEST_DIR"), "/benches/config.cfg");

    eprintln!("Benchmarking using {:?}", SONIC_BIN_PATH.as_path());
    let sonic = update_command(
        Command::new(SONIC_BIN_PATH.as_path())
            // .args(["-c", sonic_config_path])
            .env("SONIC_SERVER__LOG_LEVEL", "WARN"),
    )
    .env("SONIC_STORE__KV__PATH", data_path.join("kv"))
    .env("SONIC_STORE__FST__PATH", data_path.join("fst"))
    .spawn()
    .unwrap();

    // Auto-kill Sonic.
    let mut sonic = SpawnGuard(sonic);
    sonic.wait_until_ready(std::net::SocketAddr::from(ADDR));
    // println!("Started Sonic");

    sonic
}

fn size_char(len: usize) -> char {
    // NOTE: Largest article in the first 10000 ones is 73759B.
    let max_size: usize = 65536;
    let step: usize = max_size / 8;

    let chars = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    chars[len.min(max_size - 1).div_euclid(step)]
}
