// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Crisp IM SAS
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use clap::{Arg, ArgAction, ArgMatches, Command, value_parser};
use serde::Serialize;
use sonic_client::SonicMultiplexer;
use sonic_client::control::SonicChannelControlBlocking;
use sonic_client::ingest::{BulkDocument, BulkMode, BulkResult, SonicChannelIngestBlocking};
use sonic_client::options::{FromTimestamp, Limit, Offset, ToTimestamp};
use sonic_client::search::{Document, QueryOption, SonicChannelSearchBlocking};

#[derive(Serialize)]
struct ImportSummary {
    imported: usize,
    failed: usize,
    elapsed_ms: u128,
}

struct ImportProgress {
    last_written: usize,
    last_reported_at: Instant,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let matches = cli().get_matches();
    let addr = matches
        .get_one::<String>("addr")
        .expect("addr has a default")
        .parse::<SocketAddr>()?;
    let password = matches
        .get_one::<String>("password")
        .expect("password has a default");
    let json = matches.get_flag("json");

    match matches.subcommand() {
        Some(("import", command)) => run_import(command, addr, password, json),
        Some(("export", command)) => run_export(command, addr, password, json),
        Some(("query", command)) => run_query(command, addr, password, json),
        Some(("ping", _)) => run_ping(addr, password, json),
        Some(("consolidate", _)) => run_consolidate(addr, password, json),
        Some(("stats", command)) => run_stats(command, addr, password, json),
        _ => unreachable!("a subcommand is required"),
    }
}

fn cli() -> Command {
    Command::new("sonic-cli")
        .about("Command-line client for a running Sonic server")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .arg(
            Arg::new("addr")
                .long("addr")
                .global(true)
                .default_value("127.0.0.1:1491"),
        )
        .arg(
            Arg::new("password")
                .long("password")
                .global(true)
                .default_value("SecretPassword"),
        )
        .arg(
            Arg::new("json")
                .long("json")
                .global(true)
                .action(ArgAction::SetTrue),
        )
        .subcommand(
            Command::new("import")
                .about("Import generic Sonic NDJSON documents")
                .arg(Arg::new("collection").long("collection").required(true))
                .arg(Arg::new("file").long("file").required(true))
                .arg(
                    Arg::new("mode")
                        .long("mode")
                        .default_value("fresh")
                        .value_parser(["fresh", "upsert"]),
                )
                .arg(
                    Arg::new("batch-documents")
                        .long("batch-documents")
                        .default_value("1000")
                        .value_parser(value_parser!(usize)),
                )
                .arg(
                    Arg::new("group-window")
                        .long("group-window")
                        .help(
                            "Buffer this many documents and group them by bucket before \
                             batching, so each network batch mostly lands on a single router \
                             backend instead of being split thin across many; \
                             defaults to 10x --batch-documents",
                        )
                        .value_parser(value_parser!(usize)),
                )
                .arg(
                    Arg::new("connections")
                        .long("connections")
                        .help(
                            "Number of parallel ingest connections; a single connection only \
                             ever keeps one batch (and so one router backend) busy at a time, \
                             so raise this to actually use every backend concurrently",
                        )
                        .default_value("1")
                        .value_parser(value_parser!(usize)),
                )
                .arg(
                    Arg::new("limit")
                        .long("limit")
                        .default_value("0")
                        .value_parser(value_parser!(usize)),
                ),
        )
        .subcommand(
            Command::new("export")
                .about("Export a collection or one bucket to a local NDJSON file")
                .arg(Arg::new("collection").long("collection").required(true))
                .arg(Arg::new("bucket").long("bucket"))
                .arg(Arg::new("file").long("file").required(true))
                .arg(
                    Arg::new("batch-documents")
                        .long("batch-documents")
                        .default_value("1000")
                        .value_parser(value_parser!(usize)),
                ),
        )
        .subcommand(
            Command::new("query")
                .about("Query object identifiers or stored documents")
                .arg(Arg::new("collection").long("collection").required(true))
                .arg(Arg::new("bucket").long("bucket").required(true))
                .arg(Arg::new("terms").required(true))
                .arg(
                    Arg::new("documents")
                        .long("documents")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("limit")
                        .long("limit")
                        .default_value("10")
                        .value_parser(value_parser!(usize)),
                )
                .arg(
                    Arg::new("offset")
                        .long("offset")
                        .default_value("0")
                        .value_parser(value_parser!(usize)),
                )
                .arg(
                    Arg::new("from")
                        .long("from")
                        .value_parser(value_parser!(u64)),
                )
                .arg(Arg::new("to").long("to").value_parser(value_parser!(u64))),
        )
        .subcommand(Command::new("ping").about("Check server connectivity"))
        .subcommand(Command::new("consolidate").about("Consolidate the typo lexicon"))
        .subcommand(
            Command::new("stats")
                .about("Show collection storage statistics")
                .arg(Arg::new("collection").long("collection").required(true))
                .arg(Arg::new("deep").long("deep").action(ArgAction::SetTrue)),
        )
}

fn run_import(
    command: &ArgMatches,
    addr: SocketAddr,
    password: &str,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let collection = command
        .get_one::<String>("collection")
        .expect("required collection")
        .to_owned();
    let file = command.get_one::<String>("file").expect("required file");
    let mode = match command
        .get_one::<String>("mode")
        .expect("default import mode")
        .as_str()
    {
        "fresh" => BulkMode::Fresh,
        "upsert" => BulkMode::Upsert,
        _ => unreachable!("mode is validated by clap"),
    };
    let batch_documents = *command
        .get_one::<usize>("batch-documents")
        .expect("default batch size");
    if batch_documents == 0 {
        return Err("--batch-documents must be greater than zero".into());
    }
    let group_window = command
        .get_one::<usize>("group-window")
        .copied()
        .unwrap_or(batch_documents.saturating_mul(10))
        .max(batch_documents);
    let connections = *command
        .get_one::<usize>("connections")
        .expect("default connections");
    if connections == 0 {
        return Err("--connections must be greater than zero".into());
    }
    let limit = *command.get_one::<usize>("limit").expect("default limit");
    let multiplexer = SonicMultiplexer::new()?;

    // Issue FLUSHC on its own short-lived connection before any worker starts writing.
    if matches!(mode, BulkMode::Fresh) {
        SonicChannelIngestBlocking::connect(addr, password, &multiplexer)?.flushc(&collection)?;
    }

    let started = Instant::now();
    let written = Arc::new(AtomicUsize::new(0));
    let rejected = Arc::new(AtomicUsize::new(0));
    let progress = Arc::new(Mutex::new(ImportProgress {
        last_written: 0,
        last_reported_at: started,
    }));

    // Each worker owns its own connection: a single connection only ever has one batch (and \
    //   so one router backend) in flight at a time, since it blocks waiting for each response \
    //   before sending the next. Spreading batches across N connections is what actually lets \
    //   N backends work concurrently instead of round-tripping one at a time.
    let (sender, receiver) = crossbeam_channel::bounded::<Vec<BulkDocument>>(connections * 4);
    let mut workers = Vec::with_capacity(connections);
    for _ in 0..connections {
        let worker_ingest = SonicChannelIngestBlocking::connect(addr, password, &multiplexer)?;
        let worker_receiver = receiver.clone();
        let worker_collection = collection.clone();
        let worker_written = Arc::clone(&written);
        let worker_rejected = Arc::clone(&rejected);
        let worker_progress = Arc::clone(&progress);
        workers.push(std::thread::spawn(move || -> std::io::Result<()> {
            for batch in worker_receiver {
                let current =
                    send_adaptive_batch(&worker_ingest, &worker_collection, mode, &batch)?;
                let previous_written = worker_written.fetch_add(current.written, Ordering::Relaxed);
                let total_written = previous_written + current.written;
                worker_rejected.fetch_add(current.rejected, Ordering::Relaxed);
                // Always report progress on stderr, even in --json mode: the final summary \
                //   still goes to stdout alone, so scripts parsing it are unaffected.
                if total_written / 10_000 != previous_written / 10_000 {
                    let mut progress = worker_progress.lock().unwrap();
                    if total_written.saturating_sub(progress.last_written) >= 10_000 {
                        let now = Instant::now();
                        let interval_written = total_written - progress.last_written;
                        let interval_elapsed = now.duration_since(progress.last_reported_at);
                        eprintln!(
                            "Imported {} documents (instant {:.0}/s, average {:.0}/s)",
                            total_written,
                            interval_written as f64 / interval_elapsed.as_secs_f64(),
                            total_written as f64 / started.elapsed().as_secs_f64()
                        );
                        progress.last_written = total_written;
                        progress.last_reported_at = now;
                    }
                }
            }
            Ok(())
        }));
    }

    let reader = open_reader(file)?;
    let limit = if limit == 0 { usize::MAX } else { limit };
    let mut window = Vec::with_capacity(group_window);
    for (index, line) in reader.lines().enumerate() {
        if index >= limit {
            break;
        }
        let line = line?;
        if !line.trim().is_empty() {
            window.push(serde_json::from_str(&line)?);
            if window.len() >= group_window {
                dispatch_grouped_window(&mut window, batch_documents, &sender)?;
            }
        }
    }
    if !window.is_empty() {
        dispatch_grouped_window(&mut window, batch_documents, &sender)?;
    }
    // Dropping the sender closes the channel once drained, which is how workers know to stop.
    drop(sender);

    for worker in workers {
        worker
            .join()
            .map_err(|_| "import worker thread panicked".to_string())??;
    }

    let summary = ImportSummary {
        imported: written.load(Ordering::Relaxed),
        failed: rejected.load(Ordering::Relaxed),
        elapsed_ms: started.elapsed().as_millis(),
    };
    if json {
        println!("{}", serde_json::to_string(&summary)?);
    } else {
        println!(
            "Imported {} documents; {} failed in {} ms",
            summary.imported, summary.failed, summary.elapsed_ms
        );
    }
    if summary.failed > 0 {
        return Err(format!("{} documents failed to import", summary.failed).into());
    }
    Ok(())
}

// Groups a buffered window of documents by bucket before batching, so each network batch \
//   mostly maps to a single router backend instead of being fanned out thin across many \
//   (the router still splits any batch that does span backends; this just makes that split \
//   a no-op most of the time by not mixing buckets in the first place), then hands each \
//   resulting chunk off to whichever worker connection picks it up next.
fn dispatch_grouped_window(
    window: &mut Vec<BulkDocument>,
    batch_documents: usize,
    sender: &crossbeam_channel::Sender<Vec<BulkDocument>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut grouped: HashMap<String, Vec<BulkDocument>> = HashMap::new();
    for document in window.drain(..) {
        grouped
            .entry(document.bucket.clone())
            .or_default()
            .push(document);
    }
    for documents in grouped.into_values() {
        for chunk in documents.chunks(batch_documents) {
            sender.send(chunk.to_vec())?;
        }
    }
    Ok(())
}

fn send_adaptive_batch(
    ingest: &SonicChannelIngestBlocking,
    collection: &str,
    mode: BulkMode,
    documents: &[BulkDocument],
) -> std::io::Result<BulkResult> {
    match ingest.upsert_batch(collection, mode, documents) {
        Err(error) if error.kind() == std::io::ErrorKind::InvalidInput && documents.len() > 1 => {
            let middle = documents.len() / 2;
            let left = send_adaptive_batch(ingest, collection, mode, &documents[..middle])?;
            let right = send_adaptive_batch(ingest, collection, mode, &documents[middle..])?;
            Ok(BulkResult {
                written: left.written + right.written,
                rejected: left.rejected + right.rejected,
            })
        }
        result => result,
    }
}

// Streams a collection (or one bucket) to a local NDJSON file over the wire, via `DUMP`/`BUCKETS`; \
//   unlike the server-local `EXPORT` command, the file always lands on the machine running this \
//   CLI, and this works transparently through `sonic-router` since both commands are page-sized \
//   and bucket-scoped (or aggregated cluster-wide for bucket enumeration).
fn run_export(
    command: &ArgMatches,
    addr: SocketAddr,
    password: &str,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let collection = command
        .get_one::<String>("collection")
        .expect("required collection")
        .to_owned();
    let bucket = command.get_one::<String>("bucket").cloned();
    let file = command.get_one::<String>("file").expect("required file");
    let batch_documents = *command
        .get_one::<usize>("batch-documents")
        .expect("default batch size");
    if batch_documents == 0 {
        return Err("--batch-documents must be greater than zero".into());
    }
    let page_limit = u16::try_from(batch_documents).unwrap_or(u16::MAX);

    let multiplexer = SonicMultiplexer::new()?;
    let ingest = SonicChannelIngestBlocking::connect(addr, password, &multiplexer)?;

    let buckets = match bucket {
        Some(bucket) => vec![bucket],
        None => list_all_buckets(&ingest, &collection, page_limit)?,
    };

    let mut writer = ExportWriter::create(file)?;
    let mut exported = 0usize;
    for bucket in &buckets {
        let mut offset = 0u32;
        loop {
            let page = ingest.dump_bucket(&collection, bucket, page_limit, offset)?;
            let page_len = page.len();
            for document in &page {
                serde_json::to_writer(&mut writer, document)?;
                writer.write_all(b"\n")?;
            }
            exported += page_len;
            if page_len < usize::from(page_limit) {
                break;
            }
            offset += u32::from(page_limit);
        }
    }
    writer.close()?;

    if json {
        println!(
            "{}",
            serde_json::json!({"exported": exported, "buckets": buckets.len(), "file": file})
        );
    } else {
        println!(
            "Exported {exported} documents from {} bucket(s) to {file}",
            buckets.len()
        );
    }
    Ok(())
}

// Pages through `BUCKETS` until a page comes back shorter than requested.
fn list_all_buckets(
    ingest: &SonicChannelIngestBlocking,
    collection: &str,
    page_limit: u16,
) -> std::io::Result<Vec<String>> {
    let mut buckets = Vec::new();
    let mut offset = 0u32;
    loop {
        let page = ingest.list_buckets(collection, page_limit, offset)?;
        let page_len = page.len();
        buckets.extend(page);
        if page_len < usize::from(page_limit) {
            break;
        }
        offset += u32::from(page_limit);
    }
    Ok(buckets)
}

// Writes NDJSON to a local file, transparently Zstd-compressing it when the path ends in `.zst`; \
//   the mirror image of `open_reader`, used by `import`.
enum ExportWriter {
    Plain(BufWriter<File>),
    Compressed(zstd::stream::write::Encoder<'static, BufWriter<File>>),
}

impl ExportWriter {
    fn create(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let file = File::create(path)?;
        if path.ends_with(".zst") {
            Ok(Self::Compressed(zstd::stream::write::Encoder::new(
                BufWriter::new(file),
                3,
            )?))
        } else {
            Ok(Self::Plain(BufWriter::new(file)))
        }
    }

    fn close(self) -> std::io::Result<()> {
        match self {
            Self::Plain(mut writer) => writer.flush(),
            Self::Compressed(encoder) => encoder.finish()?.flush(),
        }
    }
}

impl Write for ExportWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(writer) => writer.write(buf),
            Self::Compressed(writer) => writer.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Plain(writer) => writer.flush(),
            Self::Compressed(writer) => writer.flush(),
        }
    }
}

fn run_query(
    command: &ArgMatches,
    addr: SocketAddr,
    password: &str,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let collection = command
        .get_one::<String>("collection")
        .expect("required collection");
    let bucket = command
        .get_one::<String>("bucket")
        .expect("required bucket");
    let terms = command.get_one::<String>("terms").expect("required terms");
    let limit = *command.get_one::<usize>("limit").expect("default limit");
    let offset = *command.get_one::<usize>("offset").expect("default offset");
    let mut options: Vec<Box<dyn QueryOption>> =
        vec![Box::new(Limit(limit)), Box::new(Offset(offset))];
    if let Some(from) = command.get_one::<u64>("from") {
        options.push(Box::new(FromTimestamp(*from)));
    }
    if let Some(to) = command.get_one::<u64>("to") {
        options.push(Box::new(ToTimestamp(*to)));
    }
    let options = options
        .iter()
        .map(|option| option.as_ref())
        .collect::<Vec<_>>();
    let multiplexer = SonicMultiplexer::new()?;
    let search = SonicChannelSearchBlocking::connect(addr, password, &multiplexer)?;

    if command.get_flag("documents") {
        let documents = search.query_documents(collection, bucket, terms, &options)?;
        print_documents(&documents, json)?;
    } else {
        let identifiers = search.query_with_options(collection, bucket, terms, &options)?;
        if json {
            println!("{}", serde_json::to_string(&identifiers)?);
        } else {
            for identifier in identifiers {
                println!("{identifier}");
            }
        }
    }
    Ok(())
}

fn print_documents(documents: &[Document], json: bool) -> Result<(), Box<dyn std::error::Error>> {
    if json {
        println!("{}", serde_json::to_string(documents)?);
    } else {
        for document in documents {
            println!(
                "{}\t{}\t{}",
                document.oid,
                document.timestamp_ms,
                document.text.replace('\n', " ")
            );
        }
    }
    Ok(())
}

fn run_ping(
    addr: SocketAddr,
    password: &str,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let multiplexer = SonicMultiplexer::new()?;
    let search = SonicChannelSearchBlocking::connect(addr, password, &multiplexer)?;
    search.ping()?;
    println!("{}", if json { r#"{"ok":true}"# } else { "PONG" });
    Ok(())
}

fn run_consolidate(
    addr: SocketAddr,
    password: &str,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let multiplexer = SonicMultiplexer::new()?;
    let control = SonicChannelControlBlocking::connect(addr, password, &multiplexer)?;
    control.trigger_consolidate()?;
    println!("{}", if json { r#"{"ok":true}"# } else { "OK" });
    Ok(())
}

fn run_stats(
    command: &ArgMatches,
    addr: SocketAddr,
    password: &str,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let collection = command
        .get_one::<String>("collection")
        .expect("required collection");
    let deep = command.get_flag("deep");
    let multiplexer = SonicMultiplexer::new()?;
    let control = SonicChannelControlBlocking::connect(addr, password, &multiplexer)?;
    let stats = control.stats(collection, deep)?;
    if json {
        println!("{}", serde_json::to_string(&stats)?);
        return Ok(());
    }
    println!(
        "Collection: {}  schema: v{}",
        stats.collection, stats.schema_version
    );
    println!(
        "Index CF:     SST {}  live {}  memtable {}  keys ~{}",
        human_bytes(stats.index.sst_bytes),
        human_bytes(stats.index.live_data_bytes),
        human_bytes(stats.index.memtable_bytes),
        stats.index.estimated_keys
    );
    println!(
        "Postings CF:  SST {}  live {}  memtable {}  keys ~{}",
        human_bytes(stats.postings.sst_bytes),
        human_bytes(stats.postings.live_data_bytes),
        human_bytes(stats.postings.memtable_bytes),
        stats.postings.estimated_keys
    );
    println!(
        "Documents CF: SST {}  live {}  memtable {}  keys ~{}",
        human_bytes(stats.documents.sst_bytes),
        human_bytes(stats.documents.live_data_bytes),
        human_bytes(stats.documents.memtable_bytes),
        stats.documents.estimated_keys
    );
    if let Some(logical) = stats.logical {
        println!(
            "Documents: {}  encoded {}  text {}  metadata {}",
            logical.document_count,
            human_bytes(logical.document_encoded_bytes),
            human_bytes(logical.document_text_bytes),
            human_bytes(logical.document_metadata_bytes)
        );
        println!(
            "Term postings: {} fragments ({} sparse, {} dense), {}, {} associations",
            logical.term_postings.fragments,
            logical.term_postings.sparse_fragments,
            logical.term_postings.dense_fragments,
            human_bytes(logical.term_postings.encoded_bytes),
            logical.term_postings.associations
        );
        println!(
            "Time postings: {} fragments ({} sparse, {} dense), {}, {} associations",
            logical.time_postings.fragments,
            logical.time_postings.sparse_fragments,
            logical.time_postings.dense_fragments,
            human_bytes(logical.time_postings.encoded_bytes),
            logical.time_postings.associations
        );
        println!("Index families:");
        for family in logical
            .families
            .into_iter()
            .filter(|family| family.keys > 0)
        {
            println!(
                "  {:<20} keys {:>10}  key {}  values {}",
                family.name,
                family.keys,
                human_bytes(family.key_bytes),
                human_bytes(family.value_bytes)
            );
        }
    }
    Ok(())
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn open_reader(path: &str) -> Result<Box<dyn BufRead>, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    if path.ends_with(".zst") {
        Ok(Box::new(BufReader::new(zstd::stream::read::Decoder::new(
            file,
        )?)))
    } else {
        Ok(Box::new(BufReader::new(file)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_parses_query_options() {
        let matches = cli()
            .try_get_matches_from([
                "sonic-cli",
                "--json",
                "query",
                "--collection",
                "movies",
                "--bucket",
                "default",
                "--documents",
                "--from",
                "1000",
                "--to",
                "2000",
                "star wars",
            ])
            .unwrap();
        assert!(matches.get_flag("json"));
        let (_, query) = matches.subcommand().unwrap();
        assert!(query.get_flag("documents"));
        assert_eq!(query.get_one::<u64>("from"), Some(&1000));
        assert_eq!(query.get_one::<u64>("to"), Some(&2000));
    }

    #[test]
    fn it_requires_import_collection_and_file() {
        assert!(cli().try_get_matches_from(["sonic-cli", "import"]).is_err());
    }

    #[test]
    fn it_parses_optional_connections() {
        let matches = cli()
            .try_get_matches_from([
                "sonic-cli",
                "import",
                "--collection",
                "movies",
                "--file",
                "movies.ndjson",
                "--connections",
                "5",
            ])
            .unwrap();
        let (_, import) = matches.subcommand().unwrap();
        assert_eq!(import.get_one::<usize>("connections"), Some(&5));
    }

    #[test]
    fn it_defaults_to_a_single_connection() {
        let matches = cli()
            .try_get_matches_from([
                "sonic-cli",
                "import",
                "--collection",
                "movies",
                "--file",
                "movies.ndjson",
            ])
            .unwrap();
        let (_, import) = matches.subcommand().unwrap();
        assert_eq!(import.get_one::<usize>("connections"), Some(&1));
    }

    #[test]
    fn it_parses_optional_group_window() {
        let matches = cli()
            .try_get_matches_from([
                "sonic-cli",
                "import",
                "--collection",
                "movies",
                "--file",
                "movies.ndjson",
                "--group-window",
                "5000",
            ])
            .unwrap();
        let (_, import) = matches.subcommand().unwrap();
        assert_eq!(import.get_one::<usize>("group-window"), Some(&5000));
    }

    #[test]
    fn it_groups_documents_by_bucket_before_chunking() {
        let mut window = vec![
            BulkDocument {
                bucket: "a".to_owned(),
                document: Document {
                    oid: "o:1".to_owned(),
                    timestamp_ms: 0,
                    text: "one".to_owned(),
                    metadata: serde_json::json!({}),
                },
            },
            BulkDocument {
                bucket: "b".to_owned(),
                document: Document {
                    oid: "o:2".to_owned(),
                    timestamp_ms: 0,
                    text: "two".to_owned(),
                    metadata: serde_json::json!({}),
                },
            },
            BulkDocument {
                bucket: "a".to_owned(),
                document: Document {
                    oid: "o:3".to_owned(),
                    timestamp_ms: 0,
                    text: "three".to_owned(),
                    metadata: serde_json::json!({}),
                },
            },
        ];
        let mut grouped: HashMap<String, Vec<BulkDocument>> = HashMap::new();
        for document in window.drain(..) {
            grouped
                .entry(document.bucket.clone())
                .or_default()
                .push(document);
        }
        assert_eq!(grouped.len(), 2);
        assert_eq!(grouped["a"].len(), 2);
        assert_eq!(grouped["b"].len(), 1);
    }

    #[test]
    fn it_parses_optional_export_bucket() {
        let matches = cli()
            .try_get_matches_from([
                "sonic-cli",
                "export",
                "--collection",
                "movies",
                "--file",
                "movies.ndjson.zst",
            ])
            .unwrap();
        assert!(
            matches
                .subcommand()
                .unwrap()
                .1
                .get_one::<String>("bucket")
                .is_none()
        );
    }

    #[test]
    fn it_deserializes_records_with_independent_buckets() {
        let first: BulkDocument = serde_json::from_str(
            r#"{"bucket":"current","oid":"movie:1","timestamp_ms":0,"text":"One","metadata":{}}"#,
        )
        .unwrap();
        let second: BulkDocument = serde_json::from_str(
            r#"{"bucket":"archive","oid":"movie:2","timestamp_ms":0,"text":"Two","metadata":{}}"#,
        )
        .unwrap();
        assert_eq!(first.bucket, "current");
        assert_eq!(second.bucket, "archive");
    }

    #[test]
    fn it_serializes_stable_json_summaries() {
        let summary = ImportSummary {
            imported: 2,
            failed: 0,
            elapsed_ms: 10,
        };
        assert_eq!(
            serde_json::to_string(&summary).unwrap(),
            r#"{"imported":2,"failed":0,"elapsed_ms":10}"#
        );
    }

    #[test]
    fn it_parses_deep_stats() {
        let matches = cli()
            .try_get_matches_from(["sonic-cli", "stats", "--collection", "movies", "--deep"])
            .unwrap();
        let (_, stats) = matches.subcommand().unwrap();
        assert!(stats.get_flag("deep"));
    }
}
