// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Baptiste Jamin <baptiste@crisp.chat>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::common::huggingface::download::download_files;
use crate::common::prelude::*;
use crate::common::spawn_guard::SpawnGuard;

const COLLECTION: &str = "beir-quora";
const BUCKET: &str = "default";

const DATASET: &str = "mteb/quora";

/// BM25 baselines from Tables 2 and 9: <https://arxiv.org/pdf/2104.08663>.
mod baseline {
    pub const BM25_NDCG_AT_10: f64 = 0.789;
    pub const BM25_RECALL_AT_100: f64 = 0.973;
}

const QUERY_LIMIT: usize = 100;

static RETAIN_WORD_OBJECTS: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("BEIR_RETAIN_WORD_OBJECTS")
        .map(|value| {
            if value == "unlimited" {
                usize::MAX
            } else {
                (value.parse()).expect(
                    "env variable `BEIR_RETAIN_WORD_OBJECTS` should be an integer or \"unlimited\"",
                )
            }
        })
        .unwrap_or(usize::MAX)
});

static STOPWORDS_ENABLED: LazyLock<bool> = LazyLock::new(|| {
    std::env::var("BEIR_STOPWORDS")
        .map(|value| match value.as_str() {
            "true" | "1" => true,
            "false" | "0" => false,
            _ => panic!("env variable `BEIR_STOPWORDS` should be a boolean"),
        })
        .unwrap_or(true)
});

static BENCHMARK_LANG: LazyLock<Lang<'static>> =
    LazyLock::new(|| Lang(if *STOPWORDS_ENABLED { "eng" } else { "none" }));

#[derive(Deserialize)]
struct TextItem {
    #[serde(rename = "_id")]
    id: String,
    #[serde(default)]
    title: Option<String>,
    text: String,
}

#[derive(Default)]
struct Metrics {
    ndcg_at_10: f64,
    map_at_100: f64,
    recall_at_100: f64,
    precision_at_10: f64,
    query_count: usize,
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

    let dataset = download_dataset();
    let qrels = load_qrels(&dataset.qrels);
    let queries = load_queries(&dataset.queries, &qrels);

    ensure_index(&dataset.corpus);
    let _sonic = start_sonic();
    let multiplexer = SonicMultiplexer::new().unwrap();

    let mut metrics = Metrics::default();
    let mut latencies = Vec::with_capacity(queries.len());
    let started_at = Instant::now();

    {
        let search =
            SonicChannelSearchBlocking::connect(ADDR, SONIC_PASSWORD, &multiplexer).unwrap();

        for (index, (query_id, query_text)) in queries.iter().enumerate() {
            let query_started_at = Instant::now();
            let results = search
                .query_with_options(
                    COLLECTION,
                    BUCKET,
                    query_text,
                    &[&*BENCHMARK_LANG, &Limit(QUERY_LIMIT)],
                )
                .unwrap_or_else(|err| panic!("Failed querying {query_id:?}: {err}"));
            latencies.push(query_started_at.elapsed());

            let relevant = qrels.get(query_id).unwrap();
            metrics.add(&results, relevant);

            if (index + 1).is_multiple_of(1000) {
                eprintln!("Evaluated {:>5}/{:>5} queries", index + 1, queries.len());
            }
        }
    }

    let elapsed = started_at.elapsed();
    metrics.print(elapsed, &mut latencies);
}

struct DatasetPaths {
    corpus: PathBuf,
    queries: PathBuf,
    qrels: PathBuf,
}

fn download_dataset() -> DatasetPaths {
    let [corpus, queries, qrels] =
        download_files(DATASET, ["corpus.jsonl", "queries.jsonl", "qrels/test.tsv"]);

    DatasetPaths {
        corpus,
        queries,
        qrels,
    }
}

fn json_lines<T: for<'de> Deserialize<'de>>(path: &Path) -> impl Iterator<Item = T> + use<T> {
    BufReader::new(File::open(path).unwrap())
        .lines()
        .map(|line| serde_json::from_str(&line.unwrap()).unwrap())
}

fn load_qrels(path: &Path) -> HashMap<String, HashMap<String, u32>> {
    let mut lines = BufReader::new(File::open(path).unwrap()).lines();
    assert_eq!(lines.next().unwrap().unwrap(), "query-id\tcorpus-id\tscore");

    let mut qrels = HashMap::<String, HashMap<String, u32>>::new();
    for line in lines {
        let line = line.unwrap();
        let mut columns = line.split('\t');
        let query_id = columns.next().unwrap();
        let corpus_id = columns.next().unwrap();
        let score = columns.next().unwrap().parse().unwrap();
        assert!(columns.next().is_none(), "Invalid qrels row: {line:?}");

        qrels
            .entry(query_id.to_owned())
            .or_default()
            .insert(corpus_id.to_owned(), score);
    }

    qrels
}

fn load_queries(
    path: &Path,
    qrels: &HashMap<String, HashMap<String, u32>>,
) -> Vec<(String, String)> {
    let mut queries: Vec<_> = json_lines::<TextItem>(path)
        .filter(|query| qrels.contains_key(&query.id))
        .map(|query| (query.id, query.text))
        .collect();
    queries.sort_unstable_by(|left, right| left.0.cmp(&right.0));

    assert_eq!(
        queries.len(),
        qrels.len(),
        "Some test qrels have no matching query"
    );
    queries
}

fn ensure_index(corpus_path: &Path) {
    let data_path = Path::new(SONIC_DATA_PATH);
    let ready_path = data_path.join("READY");
    let force_reindex = std::env::var_os("BEIR_REINDEX").is_some();
    let index_config = format!(
        "retain_word_objects={};stopwords={}",
        *RETAIN_WORD_OBJECTS, *STOPWORDS_ENABLED
    );
    let index_is_ready = std::fs::read_to_string(&ready_path)
        .is_ok_and(|marker| marker.lines().any(|line| line == index_config));

    if index_is_ready && !force_reindex {
        eprintln!("Reusing the existing Quora index at {data_path:?}");
        return;
    }
    if data_path.exists() {
        std::fs::remove_dir_all(data_path).unwrap();
    }
    std::fs::create_dir_all(data_path).unwrap();

    let _sonic = start_sonic();
    let multiplexer = SonicMultiplexer::new().unwrap();

    let started_at = Instant::now();
    let mut total_count = 0usize;
    let mut total_bytes = 0u64;

    {
        let ingest =
            SonicChannelIngestBlocking::connect(ADDR, SONIC_PASSWORD, &multiplexer).unwrap();

        const CHUNK_SIZE: usize = 10_000;
        let mut chunk_start = Instant::now();
        let mut chunk_bytes = 0u64;

        for TextItem { id, title, text } in json_lines::<TextItem>(corpus_path) {
            let text = if let Some(title) = title {
                format!("{title}\n{text}")
            } else {
                text
            };

            ingest
                .push_with_options(COLLECTION, BUCKET, &id, &text, &[&*BENCHMARK_LANG])
                .expect(&format!("Failed ingesting document {id:?}"));

            total_count += 1;
            total_bytes += text.len() as u64;
            chunk_bytes += text.len() as u64;

            if total_count.is_multiple_of(CHUNK_SIZE) {
                let chunk_elapsed = chunk_start.elapsed();
                eprintln!(
                    "Indexed {CHUNK_SIZE} documents ({chunk_bytes}B) in {chunk_elapsed:>9.3?} ({chunk_thrpt:>5}kB/s).\
                \tTotal: {total_count:>6} ({total_bytes:>8}B)",
                    chunk_thrpt = (chunk_bytes as u128 / chunk_start.elapsed().as_millis())
                );
                chunk_start = Instant::now();
                chunk_bytes = 0;
            }
        }
    }

    {
        let control =
            SonicChannelControlBlocking::connect(ADDR, SONIC_PASSWORD, &multiplexer).unwrap();
        control.trigger_consolidate().unwrap();
    }

    std::fs::write(
        &ready_path,
        format!("{index_config}\ncount={total_count}\n"),
    )
    .unwrap();

    let total_time = started_at.elapsed();
    eprintln!(
        "Indexed {total_count} documents ({total_bytes}B) in {total_time:.3?} ({thrpt}kB/s)",
        thrpt = (total_bytes / started_at.elapsed().as_secs()) as f32 / 1000.
    );
}

fn start_sonic() -> SpawnGuard {
    let data_path = Path::new(SONIC_DATA_PATH);
    eprintln!("Running BEIR Quora using {SONIC_BIN_PATH:?}");

    let child = Command::new(SONIC_BIN_PATH.as_path())
        .env("SONIC_SERVER__LOG_LEVEL", "WARN")
        .env("SONIC_SEARCH__QUERY_LIMIT_DEFAULT", QUERY_LIMIT.to_string())
        .env("SONIC_SEARCH__QUERY_LIMIT_MAXIMUM", QUERY_LIMIT.to_string())
        .env("SONIC_STORE__KV__PATH", data_path.join("kv"))
        .env(
            "SONIC_STORE__KV__RETAIN_WORD_OBJECTS",
            RETAIN_WORD_OBJECTS.to_string(),
        )
        .env("SONIC_STORE__FST__PATH", data_path.join("fst"))
        .spawn()
        .unwrap();

    let mut sonic = SpawnGuard(child);
    sonic.wait_until_ready(ADDR);
    sonic
}

impl Metrics {
    fn add(&mut self, results: &[Box<str>], relevant: &HashMap<String, u32>) {
        let relevant_count = relevant.values().filter(|&&score| score > 0).count();
        assert!(relevant_count > 0);

        let mut seen = HashSet::with_capacity(results.len());
        let mut hits = 0usize;
        let mut precision_hits = 0usize;
        let mut average_precision = 0.0;
        let mut dcg = 0.0;

        for (index, result) in results.iter().take(QUERY_LIMIT).enumerate() {
            if !seen.insert(result.as_ref()) {
                continue;
            }

            let score = relevant.get(result.as_ref()).copied().unwrap_or(0);
            if score > 0 {
                hits += 1;
                average_precision += hits as f64 / (index + 1) as f64;
                if index < 10 {
                    precision_hits += 1;
                }
            }
            if index < 10 {
                dcg += gain(score, index);
            }
        }

        let mut ideal_scores: Vec<_> = relevant.values().copied().collect();
        ideal_scores.sort_unstable_by(|left, right| right.cmp(left));
        let ideal_dcg: f64 = ideal_scores
            .into_iter()
            .take(10)
            .enumerate()
            .map(|(index, score)| gain(score, index))
            .sum();

        self.ndcg_at_10 += dcg / ideal_dcg;
        self.map_at_100 += average_precision / relevant_count.min(QUERY_LIMIT) as f64;
        self.recall_at_100 += hits as f64 / relevant_count as f64;
        self.precision_at_10 += precision_hits as f64 / 10.0;
        self.query_count += 1;
    }

    fn print(&self, elapsed: Duration, latencies: &mut [Duration]) {
        latencies.sort_unstable();
        let count = self.query_count as f64;
        let ndcg_at_10 = self.ndcg_at_10 / count;
        let recall_at_100 = self.recall_at_100 / count;

        println!("\nBEIR Quora test results ({} queries)", self.query_count);
        println!("nDCG@10:      {ndcg_at_10:.5}");
        println!("MAP@100:      {:.5}", self.map_at_100 / count);
        println!("Recall@100:   {recall_at_100:.5}");
        println!("Precision@10: {:.5}", self.precision_at_10 / count);
        println!(
            "Throughput:   {:.1} queries/s",
            count / elapsed.as_secs_f64()
        );
        println!(
            "Latency:      p50 {:.2} ms, p95 {:.2} ms, p99 {:.2} ms",
            percentile(latencies, 50).as_secs_f64() * 1000.0,
            percentile(latencies, 95).as_secs_f64() * 1000.0,
            percentile(latencies, 99).as_secs_f64() * 1000.0,
        );
        println!("\nGap from the BEIR BM25 baseline");
        print_baseline_gap("nDCG@10", ndcg_at_10, baseline::BM25_NDCG_AT_10);
        print_baseline_gap("Recall@100", recall_at_100, baseline::BM25_RECALL_AT_100);
    }
}

fn gain(score: u32, index: usize) -> f64 {
    (2_f64.powi(score as i32) - 1.0) / (index as f64 + 2.0).log2()
}

fn percentile(values: &[Duration], percentile: usize) -> Duration {
    let index = (values.len() - 1) * percentile / 100;
    values[index]
}

fn print_baseline_gap(metric: &str, score: f64, baseline: f64) {
    let relative_gap = (score / baseline - 1.0) * 100.0;
    println!("{metric}: {score:.5} vs {baseline:.3} ({relative_gap:.1}% relative)");
}
