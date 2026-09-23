// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

#![allow(dead_code)]

use std::cell::LazyCell;
use std::collections::HashMap;
use std::hint::black_box;
use std::ops::DerefMut as _;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Barrier, LazyLock, Mutex, Once, RwLock, mpsc};
use std::thread::{JoinHandle, Thread};
use std::time::{Duration, Instant};

use criterion::BenchmarkId;
use sonic_client::SonicMultiplexer;
use sonic_client::control::SonicChannelControlBlocking;
use sonic_client::ingest::SonicChannelIngestBlocking;
use sonic_client::search::SonicChannelSearchBlocking;

use crate::common::client_helpers::trigger_compact;
use crate::common::client_helpers::trigger_flush;
use crate::common::logging::HumanBytes;
use crate::common::logging::LOG_LEVEL;
use crate::common::random::{SplitMix64, build_dictionary};
use crate::common::{Ingestable, prelude::*};

static DICTIONARY: LazyLock<Vec<String>> = LazyLock::new(|| {
    build_dictionary(
        500,
        b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ",
        0x5EED_5EED_5EED_5EED,
    )
});

pub static NCHANNELS: LazyLock<usize> = LazyLock::new(|| {
    std::env::var("NCHANNELS").map_or_else(|_err| {
        let bytes = if cfg!(target_os = "macos") {
            Some(
                std::process::Command::new("sysctl")
                    .args(["-n", "hw.perflevel0.logicalcpu"])
                    .output()
                    .unwrap()
                    .stdout,
            )
        } else {
            None
        };

        if let Some(bytes) = bytes {
            let res = String::from_utf8(bytes).unwrap().trim_ascii_end().parse::<usize>().unwrap();
            tracing::info!("`NCHANNELS` not configured, using all available performance cores ({res}) as default.");
            res
        } else {
            // NOTE: Add support for your target OS if you run into this :)
            tracing::error!(
                "Could not find how many performance cores your CPU has; defaulting to 4 channels."
            );
            4
        }
    }, |s| s.parse().unwrap())
});

#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParallelBenchmarkConfig {
    #[serde(default)]
    pub defer_compaction: bool,

    #[serde(default)]
    pub rocksdb_unordered_write: Option<bool>,

    #[serde(default)]
    pub rocksdb_memtable: Option<String>,
}

impl ParallelBenchmarkConfig {
    pub fn from_path(bench_conf_path: &Path) -> Self {
        config::Config::builder()
            .add_source(config::File::new(
                bench_conf_path.to_str().unwrap(),
                config::FileFormat::Toml,
            ))
            .build()
            .unwrap()
            .try_deserialize::<ParallelBenchmarkConfig>()
            .unwrap()
    }
}

pub struct IngestStats {
    pub ingest_duration: Duration,
    pub compact_duration: Duration,
    pub consolidate_duration: Duration,
    pub elapsed_total: Duration,
    pub ingested_count: usize,
    pub ingested_bytes: u32,
}

pub fn ingest_parallel<T: Ingestable>(
    bench_name: &str,
    config: &ParallelBenchmarkConfig,
    sonic_conf_path: &Path,
    nchannels: usize,
    articles_iter: impl Iterator<Item = T> + Send + 'static,
    articles_count: usize,
) -> IngestStats {
    const COLLECTION: &str = "wikipedia";
    const BUCKET: &str = "default";

    let show_progress = *SHOW_PROGRESS;
    let push_use_new = *PUSH_USE_NEW;

    let mut elapsed_total = Duration::ZERO;

    let sonic = start_sonic_empty(Some(bench_name), |command| {
        command.arg("-c").arg(sonic_conf_path)
    });

    let multiplexer = Arc::new(SonicMultiplexer::new().unwrap());

    let control = LazyCell::new(|| {
        SonicChannelControlBlocking::connect(ADDR, SONIC_PASSWORD, &multiplexer).unwrap()
    });
    let ingest = LazyCell::new(|| {
        SonicChannelIngestBlocking::connect(ADDR, SONIC_PASSWORD, &multiplexer).unwrap()
    });

    {
        tracing::info!("Setting dynamic configuration…");

        let mut args: Vec<String> = Vec::with_capacity(6);

        // Temporarily disable background tasks so they don’t interfere with the batch ingestion.
        args.push("sonic.disable_janitor_tasks".to_owned());
        args.push("sonic.disable_fst_consolidate_task".to_owned());
        args.push("sonic.disable_kv_flush_task".to_owned());

        args.push(format!(
            "rocksdb.disable_auto_compactions={}",
            config.defer_compaction
        ));
        if let Some(unordered_write) = config.rocksdb_unordered_write {
            args.push(format!("rocksdb.unordered_write={unordered_write}"));
        }
        if let Some(ref memtable) = config.rocksdb_memtable {
            args.push(format!("rocksdb.memtable={memtable}"));
        }

        control.config_set(COLLECTION, &args).unwrap();
    }

    tracing::info!("Ingesting…");

    let articles = Arc::new(Mutex::new(articles_iter));

    let (mut ingest_duration, ingested_count, ingested_bytes) = (0..nchannels)
        .map(|i| {
            std::thread::Builder::new().name(format!("thread-{i}")).spawn({
                let articles = Arc::clone(&articles);
                let multiplexer = Arc::clone(&multiplexer);

                move || {
                    let mut channel = SonicChannelIngestBlocking::connect(
                        ADDR,
                        SONIC_PASSWORD,
                        &multiplexer,
                    ).unwrap();
                    // println!("Opened Sonic channel");

                    // Ensure Sonic is running fine.
                    channel.ping().unwrap();

                    let mut ingested_count = 0usize;
                    let mut ingested_bytes = 0u32;

                    /// Helper function which returns the next iterator element without keeping the lock guard alive.
                    /// When put on a single line (e.g. in a `while` loop), the lock guard stays alive all the time,
                    /// preventing parallelism.
                    fn next<T>(mutex: &Mutex<impl Iterator<Item = T>>) -> Option<T> {
                        let mut lock = mutex.lock().unwrap();
                        let next = lock.next();
                        drop(lock);
                        next
                    }

                    let mut push_options: Vec<&dyn sonic_client::ingest::PushOption> = Vec::with_capacity(2);
                    push_options.push(&Lang("eng"));
                    if push_use_new {
                        push_options.push(&New);
                    }

                    let start = Instant::now();
                    while let Some(object) = next(&articles) {
                        let len = object.data().as_bytes().len();

                        match black_box(channel.push_with_options(COLLECTION, BUCKET, object.id(), object.data(), push_options.as_slice())) {
                            Ok(()) => {
                                if show_progress {
                                    eprint!("{}", T::size_char(len));
                                }

                                ingested_count += 1;
                                ingested_bytes += len as u32;
                            }
                            Err(err) => {
                                panic!(
                                    "Failed ingesting {title:?} ({len:.2}) after {ingested_count} success(es) ({ingested_bytes:.2}): {err}",
                                    title = object.title(),
                                    len = HumanBytes::from(len as u64),
                                    ingested_bytes = HumanBytes::from(ingested_bytes),
                                );
                            }
                        };
                    }
                    let elapsed = start.elapsed();

                    channel.quit().unwrap();
                    drop(channel);

                    (elapsed, ingested_count, ingested_bytes)
                }
            }).unwrap()
        })
        // WARN: This `collect` is important, as it is what spawns the threads!
        .collect::<Vec<_>>()
        .into_iter()
        .map(|h| h.join().expect("thread panicked"))
        .fold((Duration::ZERO, 0, 0), |(a, b, c), (x, y, z)| {
            (a + x, b + y, c + z)
        });

    ingest_duration = ingest_duration / (nchannels as u32);

    elapsed_total += ingest_duration;

    tracing::info!(
        "Ingested {ingested_count} articles ({size:.2}) in {ingest_duration:.3?}.",
        size = HumanBytes::from(ingested_bytes)
    );

    let (compact_duration, consolidate_duration) = {
        let compact_duration = {
            tracing::info!("Compacting KV…");

            let start = Instant::now();

            black_box(trigger_compact(&control, &[COLLECTION])).unwrap();

            let compact_duration = start.elapsed();
            elapsed_total += compact_duration;

            tracing::info!("Compacted KV in {compact_duration:.3?}.");

            compact_duration
        };

        let consolidate_duration = {
            tracing::info!("Consolidating FST…");

            let start = Instant::now();

            black_box(control.trigger_consolidate()).unwrap();

            let consolidate_duration = start.elapsed();
            elapsed_total += consolidate_duration;

            tracing::info!("Consolidated FST in {consolidate_duration:.3?}.");

            consolidate_duration
        };

        (compact_duration, consolidate_duration)
    };

    if *config != ParallelBenchmarkConfig::default() {
        tracing::info!("Resetting dynamic configuration…");

        control.config_reset_all(COLLECTION).unwrap();
    }

    {
        tracing::info!("Ensuring documents have 1:1 matching IIDs…");

        let count = ingest.countb(COLLECTION, BUCKET).unwrap();
        if count != articles_count {
            // NOTE: Do not `panic` as some versions of Sonic had this bug and it
            //   would render benchmark comparison impossible for no good reason.
            tracing::error!(
                "Data was indexed as more IIDs than there are articles (actual: {count}, expected: {articles_count})"
            )
        }
    }

    drop(control);
    drop(ingest);
    drop(sonic);

    IngestStats {
        ingest_duration,
        compact_duration,
        consolidate_duration,
        elapsed_total,
        ingested_count,
        ingested_bytes,
    }
}

pub fn run_bench<const N: usize>(
    bench_id: BenchmarkId,
    group: &mut criterion::BenchmarkGroup<criterion::measurement::WallTime>,
    update_command: impl FnOnce(&mut Command) -> &mut Command,
    prepopulate: impl FnOnce(&RunContext),
    seed: Option<u64>,
    actors: impl Fn(&RunContext, &mut bus::Bus<Message>) -> [Box<dyn Actor>; N],
) {
    let bench_state = Mutex::new(LazyLock::new(|| {
        let mut ctx = start_sonic_empty(None, |command| {
            update_command(command)
                .env(
                    "SONIC_SERVER__LOG_LEVEL",
                    LOG_LEVEL.unwrap_or(tracing::Level::WARN).to_string(),
                )
                // Disable background tasks (we’ll run them manually).
                .env("SONIC_STORE__FST__GRAPH__CONSOLIDATE_AFTER", "3600")
                .env("SONIC_STORE__FST__POOL__INACTIVE_AFTER", "3700")
                .env("SONIC_STORE__KV__DATABASE__FLUSH_AFTER", "3600")
                .env("SONIC_STORE__KV__POOL__INACTIVE_AFTER", "3700")
                .env("RUST_BACKTRACE", "full")
        });

        // Pre-populate Sonic.
        prepopulate(&ctx);

        if let Some(manual_seed) = seed {
            *ctx.seed.deref_mut() = manual_seed;
        }
        tracing::info!("Run seed: {}", *ctx.seed);

        let mut bus = bus::Bus::<Message>::new(1);

        // Create actors.
        let actors = actors(&ctx, &mut bus);
        let node_count = N;

        // Create synchronization promitives.
        let (tick_finished_tx, tick_finished_rx) = mpsc::sync_channel::<TickResult>(node_count);
        let tick_finished_tx = Arc::new(tick_finished_tx);

        // Spawn child threads.
        let handles = {
            let mut handles = Vec::with_capacity(node_count);
            for (n, mut actor) in actors.into_iter().enumerate() {
                let thread_name = format!("client-{n}");
                tracing::debug!("Spawning {thread_name}");

                let tick_finished_tx = Arc::clone(&tick_finished_tx);

                let join_handle = std::thread::Builder::new()
                    .name(thread_name)
                    .spawn(move || {
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            let multiplexer = SonicMultiplexer::new().unwrap();
                            actor.run(&multiplexer);
                        }));
                        tracing::trace!("catch_unwind: {result:?}");

                        if result.is_err() {
                            _ = tick_finished_tx.send(TickResult::Failure);
                        }
                    })
                    .unwrap();
                handles.push(join_handle);
            }
            handles
        };

        (
            ctx,
            BenchFnCtx {
                node_count,
                handles,
                bus,
                tick_finished_tx,
                tick_finished_rx,
            },
        )
    }));

    // NOTE: It’s important to use a constant here, to avoid initializing
    //   `bench_state` and thus spawning a Sonic instance.
    group.throughput(criterion::Throughput::Elements(N as u64));

    let warmup = Once::new();

    group.bench_with_input(bench_id, &bench_state, |b, state| {
        let mut state = state.lock().unwrap();
        let ctx = &mut (*state).deref_mut().1;
        let mut timings: HashMap<String, Duration> = HashMap::with_capacity(ctx.node_count);

        let mut routine = move |iters| {
            tracing::debug!("Running {iters} iters");
            let mut elapsed_total = Duration::ZERO;

            for tick in 0..iters {
                match run_iter(tick, ctx, &mut timings) {
                    Ok(_elapsed_iter) => {
                        let max_elapsed = timings.values().max().unwrap();
                        if *max_elapsed > Duration::from_millis(30) {
                            tracing::warn!("max_elapsed={max_elapsed:.3?}");
                        }
                        elapsed_total += *max_elapsed;
                    }
                    Err(err) => {
                        panic!("{err}");
                    }
                }
            }

            elapsed_total
        };

        // Run a warmup ouselves, before Criterion even gets a chance to do its
        // calculations, to account for read and write amplifications.
        warmup.call_once(|| {
            routine(2048);
        });

        b.iter_custom(routine);
    });

    if let Some((_, ctx)) = LazyLock::get_mut(bench_state.lock().unwrap().deref_mut()) {
        tracing::debug!("Finished");
        ctx.send(Event::Finish, None);

        for handle in ctx.handles.drain(..) {
            handle.join().unwrap();
        }
    }
    drop(bench_state);
}

pub fn run_bench_multi<const N: usize>(
    bench_name: &'static str,
    configs: &[ParallelismBenchmarkConfig],
    group: &mut criterion::BenchmarkGroup<criterion::measurement::WallTime>,
    prepopulate: impl FnOnce(&RunContext) + Copy,
    seed: Option<u64>,
    actors: impl Fn(&RunContext, &mut bus::Bus<Message>) -> [Box<dyn Actor>; N] + Copy,
) {
    for config in configs {
        run_bench(
            BenchmarkId::new(bench_name, config),
            group,
            |c| config.update_command(c),
            prepopulate,
            seed,
            actors,
        );
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ParallelismBenchmarkConfig {
    pub rocksdb_wal_enabled: Option<bool>,
    pub rocksdb_max_open_files: Option<i32>,
    pub rocksdb_write_buffer_size: Option<u32>,
    pub rocksdb_min_write_buffer_number: Option<u32>,
    pub rocksdb_level_zero_file_num_compaction_trigger: Option<i32>,
    pub rocksdb_level_zero_slowdown_writes_trigger: Option<i32>,
    pub rocksdb_level_zero_stop_writes_trigger: Option<i32>,
    pub rocksdb_target_file_size_base: Option<i32>,
}

impl ParallelismBenchmarkConfig {
    fn update_command<'c>(&self, command: &'c mut Command) -> &'c mut Command {
        let Self {
            rocksdb_wal_enabled,
            rocksdb_max_open_files,
            rocksdb_write_buffer_size,
            rocksdb_min_write_buffer_number,
            rocksdb_level_zero_file_num_compaction_trigger,
            rocksdb_level_zero_slowdown_writes_trigger,
            rocksdb_level_zero_stop_writes_trigger,
            rocksdb_target_file_size_base,
        } = self;

        if let Some(value) = rocksdb_wal_enabled {
            command.env(
                "SONIC_STORE__KV__DATABASE__WRITE_AHEAD_LOG",
                value.to_string(),
            );
        }
        if let Some(value) = rocksdb_max_open_files {
            command.env("SONIC_STORE__KV__DATABASE__MAX_FILES", value.to_string());
        }
        if let Some(value) = rocksdb_write_buffer_size {
            command.env(
                "SONIC_STORE__KV__DATABASE__WRITE_BUFFER_SIZE",
                (value / 1024).to_string(),
            );
        }
        if let Some(value) = rocksdb_min_write_buffer_number {
            command.env(
                "SONIC_STORE__KV__DATABASE__MIN_WRITE_BUFFER_NUMBER",
                value.to_string(),
            );
        }
        if let Some(value) = rocksdb_level_zero_file_num_compaction_trigger {
            command.env(
                "SONIC_STORE__KV__DATABASE__LEVEL_ZERO_FILE_NUM_COMPACTION_TRIGGER",
                value.to_string(),
            );
        }
        if let Some(value) = rocksdb_level_zero_slowdown_writes_trigger {
            command.env(
                "SONIC_STORE__KV__DATABASE__LEVEL_ZERO_SLOWDOWN_WRITES_TRIGGER",
                value.to_string(),
            );
        }
        if let Some(value) = rocksdb_level_zero_stop_writes_trigger {
            command.env(
                "SONIC_STORE__KV__DATABASE__LEVEL_ZERO_STOP_WRITES_TRIGGER",
                value.to_string(),
            );
        }
        if let Some(value) = rocksdb_target_file_size_base {
            command.env(
                "SONIC_STORE__KV__DATABASE__TARGET_FILE_SIZE_BASE",
                value.to_string(),
            );
        }

        command
    }
}

impl std::fmt::Display for ParallelismBenchmarkConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "default")?;

        let Self {
            rocksdb_wal_enabled,
            rocksdb_max_open_files,
            rocksdb_write_buffer_size,
            rocksdb_min_write_buffer_number,
            rocksdb_level_zero_file_num_compaction_trigger,
            rocksdb_level_zero_slowdown_writes_trigger,
            rocksdb_level_zero_stop_writes_trigger,
            rocksdb_target_file_size_base,
        } = self;

        macro_rules! add {
            (bool: $val:ident as $name:expr) => {
                if let Some(value) = $val {
                    if *value {
                        write!(f, concat!("[+", $name, "]"))?;
                    } else {
                        write!(f, concat!("[-", $name, "]"))?;
                    }
                }
            };
            (int: $val:ident as $name:expr) => {
                if let Some(value) = $val {
                    write!(f, "[{}={}]", $name, value)?;
                }
            };
            // Shorter helper.
            ($type:tt: $val:ident) => {
                add!($type: $val as stringify!($val));
            };
        }

        add!(bool: rocksdb_wal_enabled as "wal");
        add!(int: rocksdb_max_open_files);
        add!(int: rocksdb_write_buffer_size);
        add!(int: rocksdb_min_write_buffer_number);
        add!(int: rocksdb_level_zero_file_num_compaction_trigger);
        add!(int: rocksdb_level_zero_slowdown_writes_trigger);
        add!(int: rocksdb_level_zero_stop_writes_trigger);
        add!(int: rocksdb_target_file_size_base);

        Ok(())
    }
}

pub struct BenchFnCtx {
    pub node_count: usize,
    pub handles: Vec<JoinHandle<()>>,
    pub bus: bus::Bus<Message>,
    pub tick_finished_tx: Arc<mpsc::SyncSender<TickResult>>,
    pub tick_finished_rx: mpsc::Receiver<TickResult>,
}

impl BenchFnCtx {
    pub fn send(
        &mut self,
        event: Event,
        mut timings: Option<&mut HashMap<String, Duration>>,
    ) -> bool {
        let barrier = Arc::new(Barrier::new(self.node_count + 1));
        self.bus.broadcast(Message {
            event,
            start_barrier: Arc::clone(&barrier),
            end_tx: Arc::clone(&self.tick_finished_tx),
        });
        tracing::trace!("Barrier {}", std::thread::current().name().unwrap());
        barrier.wait();

        if matches!(event, Event::Finish) {
            return true;
        }

        let mut remaining = self.node_count;
        while remaining > 0 {
            match self.tick_finished_rx.recv().unwrap() {
                TickResult::Success(thread, elapsed_opt) => {
                    remaining -= 1;

                    if let Some(timings) = timings.as_mut() {
                        // NOTE: `elapsed_opt == None` if tick is skipped.
                        if let Some(elapsed) = elapsed_opt {
                            timings.insert(thread.name().unwrap().to_owned(), elapsed);
                        }
                    }
                }
                TickResult::Failure => {
                    tracing::trace!("Failure");

                    self.bus.broadcast(Message {
                        event: Event::Abort,
                        start_barrier: Arc::new(Barrier::new(0)),
                        end_tx: Arc::clone(&self.tick_finished_tx),
                    });

                    return false;
                }
            }
        }

        true
    }
}

pub fn run_iter<'timings>(
    tick: u64,
    ctx: &mut BenchFnCtx,
    timings: &'timings mut HashMap<String, Duration>,
) -> Result<Duration, &'static str> {
    // eprint!("\n");
    tracing::debug!("{tick:02}: Tick");

    if !ctx.send(Event::Setup(tick), None) {
        return Err("Setup failed");
    };

    let tick_start = std::time::Instant::now();

    timings.clear();

    if !ctx.send(Event::Tick(tick), Some(timings)) {
        return Err("Tick failed");
    };

    let elapsed = tick_start.elapsed();

    tracing::info!("{tick:02}: Tick took {elapsed:.3?} (total)");

    if !ctx.send(Event::TearDown(tick), None) {
        return Err("Teardown failed");
    };

    Ok(elapsed)
}

// MARK: Actors

#[derive(Debug, Clone)]
pub struct Message {
    pub event: Event,
    pub start_barrier: Arc<Barrier>,
    pub end_tx: Arc<mpsc::SyncSender<TickResult>>,
}

#[derive(Debug, Clone, Copy)]
pub enum Event {
    // Sent before a tick. Can be used to generate data.
    Setup(u64),
    Tick(u64),
    // Sent after a tick. Can be used to retrieve data.
    TearDown(u64),
    Finish,
    Abort,
}

pub enum TickResult {
    Success(Thread, Option<Duration>),
    Failure,
}

type Inbox = bus::BusReader<Message>;
type RunCondition = Box<dyn Fn(u64) -> bool + Send + 'static>;

pub trait Actor: Send {
    fn run(&mut self, multiplexer: &SonicMultiplexer);
}

fn run_loop<T: std::fmt::Debug>(
    label: &'static str,
    inbox: &mut Inbox,
    condition: &RunCondition,
    mut state: T,
    setup: impl Fn(&mut T),
    run_one: impl Fn(&mut T),
    tear_down: impl Fn(&mut T),
) {
    assert!(label.chars().next().unwrap().is_uppercase());

    let thread_name = std::thread::current().name().unwrap().to_owned();

    loop {
        let Message {
            event,
            start_barrier,
            end_tx,
        } = match inbox.recv() {
            Ok(message) => message,
            Err(error) => {
                tracing::debug!(?error, "Breaking: RecvError");
                break;
            }
        };
        tracing::trace!("{thread_name}: {event:?}");

        tracing::trace!("{thread_name}: Barrier");
        start_barrier.wait();

        let mut elapsed_opt: Option<Duration> = None;
        match event {
            Event::Finish => {
                tracing::debug!("Breaking: Finish");
                break;
            }
            Event::Abort => {
                tracing::debug!("Breaking: Abort");
                break;
            }
            Event::Setup(tick) if condition(tick) => setup(&mut state),
            Event::Tick(tick) if condition(tick) => {
                tracing::debug!(?state, "{tick:02}:{thread_name}: Start {label}");

                let start = std::time::Instant::now();

                run_one(&mut state);

                let elapsed = start.elapsed();

                tracing::info!("{tick:02}:{thread_name}: {label} took {elapsed:.3?}");

                elapsed_opt = Some(start.elapsed());
            }
            Event::TearDown(tick) if condition(tick) => tear_down(&mut state),
            _ => {}
        }

        _ = end_tx.send(TickResult::Success(std::thread::current(), elapsed_opt));
    }
}

/// Pushes data.
pub struct PushActor {
    pub addr: std::net::SocketAddr,
    pub inbox: Inbox,
    pub condition: RunCondition,
    pub seed: SplitMix64,
    pub collection: String,
    pub bucket: String,
    pub next_write: Option<(String, String)>,
    pub last_write: Arc<RwLock<Option<(String, String)>>>,
}

impl Actor for PushActor {
    fn run(&mut self, multiplexer: &SonicMultiplexer) {
        let ingest =
            SonicChannelIngestBlocking::connect(self.addr, SONIC_PASSWORD, &multiplexer).unwrap();

        run_loop(
            "Push",
            &mut self.inbox,
            &self.condition,
            (&mut self.seed, &mut self.next_write, &mut self.last_write),
            |(seed, next_write, _last_write)| {
                let len = 1000 + seed.next_range(200);
                let id = seed.next_u64();
                **next_write = Some((
                    format!("article:{id}"),
                    random::pick_words(len, &DICTIONARY, id).join(" "),
                ));
            },
            |(_seed, next_write, _last_write)| {
                let (object, text) = next_write.as_ref().unwrap();

                tracing::trace!("PUSH {object:?} \"{}…\"", &text[..=32]);

                ingest
                    .push_with_options(
                        &self.collection,
                        &self.bucket,
                        object,
                        text,
                        &[&Lang("none")],
                    )
                    .unwrap();
            },
            |(_seed, text, last_write)| {
                let mut new_write = std::mem::take(*text);

                // Truncate text to first 4 words as it will be used as a query.
                // NOTE: We’re not truncating mid-word as the FST graph isn’t
                //   always consolidated at the time of query.
                (new_write.as_mut().unwrap().1)
                    .splitn(5, ' ')
                    .take(4)
                    .join(" ");

                *last_write.write().unwrap() = new_write;
            },
        );

        // drop(ingest);
    }
}

/// Runs a query.
pub struct QueryActor {
    pub addr: std::net::SocketAddr,
    pub inbox: Inbox,
    pub condition: RunCondition,
    pub collection: String,
    pub bucket: String,
    pub expected: (String, String),
    pub next_expected: Arc<RwLock<Option<(String, String)>>>,
}

impl Actor for QueryActor {
    fn run(&mut self, multiplexer: &SonicMultiplexer) {
        let search =
            SonicChannelSearchBlocking::connect(self.addr, SONIC_PASSWORD, &multiplexer).unwrap();

        run_loop(
            "Query",
            &mut self.inbox,
            &self.condition,
            (&mut self.expected, &mut self.next_expected),
            |(expected, next)| {
                **expected = next.read().unwrap().clone().unwrap();
            },
            |(expected, _next)| {
                tracing::trace!("QUERY {:?}", &expected.1);

                let response = search
                    .query_with_options(
                        &self.collection,
                        &self.bucket,
                        &expected.1,
                        &[&Lang("none")],
                    )
                    .unwrap();
                let expected_oid = Box::from(expected.0.as_str());

                assert!(
                    response.contains(&expected_oid),
                    "{response:?} should contain {expected_oid} (query: {query:?})",
                    query = expected.1.as_str()
                );
            },
            |_| {},
        );

        // drop(search);
    }
}

/// Flushes the KV store.
pub struct FlushActor {
    pub addr: std::net::SocketAddr,
    pub inbox: Inbox,
    pub condition: RunCondition,
    /// Whether the test should fail if Sonic was compiled without support for
    /// the command.
    pub fail_if_not_found: bool,
}

impl Actor for FlushActor {
    fn run(&mut self, multiplexer: &SonicMultiplexer) {
        let control =
            SonicChannelControlBlocking::connect(self.addr, SONIC_PASSWORD, &multiplexer).unwrap();

        run_loop(
            "Flush",
            &mut self.inbox,
            &self.condition,
            (),
            |_| {},
            |_| {
                trigger_flush(&control)
                    .or_else(|err| {
                        if err.to_string().contains("not_found") {
                            if !self.fail_if_not_found {
                                Ok(())
                            } else {
                                Err("Sonic was compiled without support for `TRIGGER flush`"
                                    .to_owned())
                            }
                        } else {
                            Err(err.to_string())
                        }
                    })
                    .unwrap();
            },
            |_| {},
        );

        // drop(control);
    }
}

/// Consolidates the FST store.
pub struct ConsolidateActor {
    pub addr: std::net::SocketAddr,
    pub inbox: Inbox,
    pub condition: RunCondition,
}

impl Actor for ConsolidateActor {
    fn run(&mut self, multiplexer: &SonicMultiplexer) {
        let control =
            SonicChannelControlBlocking::connect(self.addr, SONIC_PASSWORD, &multiplexer).unwrap();

        run_loop(
            "Consolidate",
            &mut self.inbox,
            &self.condition,
            (),
            |_| {},
            |_| {
                control.trigger_consolidate().unwrap();
            },
            |_| {},
        );

        // drop(control);
    }
}
