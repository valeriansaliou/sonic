// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{LazyLock, RwLock};
use std::time::Instant;

use super::wikipedia::WikipediaArticle;
use crate::prelude::*;

impl Ingestable for WikipediaArticle {
    fn id(&self) -> &str {
        &self.id
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn data(&self) -> &str {
        &self.text
    }

    fn size_char(size: usize) -> char {
        size_char(size)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PushBenchmarkConfig {
    pub diacritic_folding_enabled: Option<bool>,
}

impl PushBenchmarkConfig {
    pub fn update_command<'c>(&self, command: &'c mut Command) -> &'c mut Command {
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

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct NormalizationConfig {
    pub diacritic_folding_enabled: Option<bool>,
}

pub fn start_sonic_prepopulated<Articles: Iterator<Item = WikipediaArticle>>(
    ctx: &mut RunContext,
    data_path: &Path,
    password: String,
    multiplexer: &SonicMultiplexer,
    normalization_config: NormalizationConfig,
    update_command: impl for<'a> FnOnce(&'a mut Command) -> &'a mut Command,
    articles: fn() -> Articles,
) {
    fn apply_normalization(
        normalization_config: NormalizationConfig,
        command: &mut Command,
    ) -> &mut Command {
        let NormalizationConfig {
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

    static PATHS: LazyLock<RwLock<HashMap<NormalizationConfig, PathBuf>>> =
        LazyLock::new(|| RwLock::new(HashMap::with_capacity(1)));
    let show_progress = *SHOW_PROGRESS;

    let mut paths = PATHS.write().unwrap();
    let path = paths
        .entry(normalization_config)
        .or_insert_with(|| {
            let data_path = data_path.join("prepopulated");

            ctx.start_sonic(&data_path, String::clone(&password), |command| {
                apply_normalization(normalization_config, command)
            });

            // Ingest data.
            {
                // PUSH
                {
                    let mut channel = SonicChannelIngestBlocking::connect(
                        ctx.addr,
                        &ctx.password,
                        multiplexer,
                    ).unwrap();
                    // println!("Opened Sonic channel");

                    // Ensure Sonic is running fine.
                    channel.ping().unwrap();

                    let mut ingested_count = 0usize;
                    let mut ingested_bytes = 0u32;

                    let mut max_size = 0;

                    let start = Instant::now();
                    for article in articles() {
                        let len = article.text.len();
                        // eprintln!("\n================================");
                        // eprintln!("{}", &article.title);
                        // eprintln!("{}", &article.text[..(1000.min(len))]);
                        max_size = max_size.max(len);

                        match channel.push_with_options("wikipedia", "default", article.id, article.text, &[&Lang("eng")]) {
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

                    println!("Ingested {ingested_count} articles ({ingested_bytes}B) in {elapsed:.3?} (max size: {max_size}).");
                }

                // TRIGGER consolidate
                {
                    let mut channel = SonicChannelControlBlocking::connect(ctx.addr, &ctx.password, multiplexer).unwrap();

                    let start = Instant::now();

                    channel.trigger_consolidate().unwrap();

                    let elapsed = start.elapsed();

                    channel.quit().unwrap();
                    drop(channel);

                    println!("Consolidated in {elapsed:.3?}.");
                }
            }

            drop(ctx.sonic.take().unwrap());

            data_path
        })
        .as_path();

    ctx.start_sonic(path, password, |command| {
        update_command(apply_normalization(normalization_config, command))
    });
}

pub fn size_char(len: usize) -> char {
    // NOTE: Largest article in the first 10000 ones is 73759B.
    let max_size: usize = 65536;
    let step: usize = max_size / 8;

    let chars = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    chars[len.min(max_size - 1).div_euclid(step)]
}
