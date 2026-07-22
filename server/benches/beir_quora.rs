// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

#[path = "common/huggingface/download.rs"]
mod huggingface_download;
#[path = "common/spawn_guard.rs"]
mod spawn_guard;

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::net::Ipv6Addr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use serde::Deserialize;
use sonic_client::SonicMultiplexer;
use sonic_client::control::SonicChannelControlBlocking;
use sonic_client::ingest::SonicChannelIngestBlocking;
use sonic_client::options::{Lang, Limit};
use sonic_client::search::SonicChannelSearchBlocking;

use crate::huggingface_download::download_files;
use crate::spawn_guard::SpawnGuard;

const ADDR: (Ipv6Addr, u16) = (Ipv6Addr::LOCALHOST, 1491);
const COLLECTION: &str = "beir-quora";
const BUCKET: &str = "default";
const QUERY_LIMIT: usize = 100;
const DATASET: &str = "mteb/quora";
const SONIC_DATA_PATH: &str = concat!(env!("CARGO_TARGET_TMPDIR"), "/bench-data/beir-quora");
// BM25 baselines from Tables 2 and 9: https://arxiv.org/pdf/2104.08663
const BM25_NDCG_AT_10: f64 = 0.789;
const BM25_RECALL_AT_100: f64 = 0.973;

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

static RETAIN_WORD_OBJECTS: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("BEIR_RETAIN_WORD_OBJECTS")
        .map(|value| {
            if value == "unlimited" {
                usize::MAX
            } else {
                value.parse().unwrap_or_else(|_| {
                    panic!("BEIR_RETAIN_WORD_OBJECTS must be an integer or \"unlimited\"")
                })
            }
        })
        .unwrap_or(usize::MAX)
});

static STOPWORDS_ENABLED: LazyLock<bool> = LazyLock::new(|| {
    std::env::var("BEIR_STOPWORDS")
        .map(|value| match value.as_str() {
            "true" | "1" => true,
            "false" | "0" => false,
            _ => panic!("BEIR_STOPWORDS must be true or false"),
        })
        .unwrap_or(true)
});

#[derive(Deserialize)]
struct TextItem {
    #[serde(rename = "_id")]
    id: String,
    #[serde(default)]
    title: String,
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
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .without_time()
        .init();

    let dataset = download_dataset();
    let qrels = load_qrels(&dataset.qrels);
    let queries = load_queries(&dataset.queries, &qrels);

    ensure_index(&dataset.corpus);
    let sonic = start_sonic();
    let multiplexer = SonicMultiplexer::new().unwrap();
    let mut channel =
        SonicChannelSearchBlocking::connect(ADDR, "SecretPassword", &multiplexer).unwrap();
    channel.ping().unwrap();

    let mut metrics = Metrics::default();
    let mut latencies = Vec::with_capacity(queries.len());
    let started_at = Instant::now();

    for (index, (query_id, query_text)) in queries.iter().enumerate() {
        let query_started_at = Instant::now();
        let results = channel
            .query_with_options(
                COLLECTION,
                BUCKET,
                query_text,
                &[&benchmark_lang(), &Limit(QUERY_LIMIT)],
            )
            .unwrap_or_else(|err| panic!("Failed querying {query_id:?}: {err}"));
        latencies.push(query_started_at.elapsed());

        let relevant = qrels.get(query_id).unwrap();
        metrics.add(&results, relevant);

        if (index + 1).is_multiple_of(1000) {
            eprintln!("Evaluated {}/{} queries", index + 1, queries.len());
        }
    }

    let elapsed = started_at.elapsed();
    channel.quit().unwrap();
    drop(channel);
    drop(sonic);

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

    let sonic = start_sonic();
    let multiplexer = SonicMultiplexer::new().unwrap();
    let mut channel =
        SonicChannelIngestBlocking::connect(ADDR, "SecretPassword", &multiplexer).unwrap();
    channel.ping().unwrap();

    let started_at = Instant::now();
    let mut count = 0usize;
    for item in json_lines::<TextItem>(corpus_path) {
        let text = if item.title.is_empty() {
            item.text
        } else {
            format!("{}\n{}", item.title, item.text)
        };
        channel
            .push_with_options(COLLECTION, BUCKET, &item.id, &text, &[&benchmark_lang()])
            .unwrap_or_else(|err| panic!("Failed ingesting document {:?}: {err}", item.id));
        count += 1;

        if count.is_multiple_of(10_000) {
            eprintln!("Indexed {count} documents");
        }
    }
    channel.quit().unwrap();
    drop(channel);

    let mut control =
        SonicChannelControlBlocking::connect(ADDR, "SecretPassword", &multiplexer).unwrap();
    control.trigger_consolidate().unwrap();
    control.quit().unwrap();
    drop(control);
    drop(sonic);

    std::fs::write(&ready_path, format!("{index_config}\ncount={count}\n")).unwrap();
    eprintln!("Indexed {count} documents in {:.3?}", started_at.elapsed());
}

fn start_sonic() -> SpawnGuard {
    let data_path = Path::new(SONIC_DATA_PATH);
    eprintln!("Running BEIR Quora using {:?}", SONIC_BIN_PATH.as_path());

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
    sonic.wait_until_ready(std::net::SocketAddr::from(ADDR));
    sonic
}

fn benchmark_lang() -> Lang<'static> {
    Lang(if *STOPWORDS_ENABLED { "eng" } else { "none" })
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
        print_baseline_gap("nDCG@10", ndcg_at_10, BM25_NDCG_AT_10);
        print_baseline_gap("Recall@100", recall_at_100, BM25_RECALL_AT_100);
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
