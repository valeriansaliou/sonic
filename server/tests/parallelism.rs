// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

mod common;

use std::collections::HashMap;
use std::ops::{DerefMut, Mul as _};
use std::sync::{Arc, Barrier, LazyLock, RwLock, mpsc};
use std::thread::Thread;
use std::time::Duration;

use sonic_client::SonicMultiplexer;
use sonic_client::control::SonicChannelControlBlocking;
use sonic_client::ingest::SonicChannelIngestBlocking;
use sonic_client::options::*;
use sonic_client::search::SonicChannelSearchBlocking;

use crate::common::client_helpers::trigger_flush;
use crate::common::logging::init_logging;
use crate::common::prelude::*;
use crate::common::random::{SplitMix64, build_dictionary};

const SLEEP: std::time::Duration = std::time::Duration::from_millis(2);

static DICTIONARY: LazyLock<Vec<String>> = LazyLock::new(|| {
    build_dictionary(
        500,
        b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ",
        0x5EED_5EED_5EED_5EED,
    )
});

fn run_test(
    iter_count: u16,
    seed: Option<u64>,
    actors: impl Fn(&TestData, &mut bus::Bus<Message>) -> Vec<Box<dyn Actor>>,
    mut validate_iter: impl FnMut(&HashMap<String, std::time::Duration>, std::time::Duration) -> bool,
) {
    init_logging("sonic_client=info,sonic_core=warn", false, false);

    let mut ctx = start_prepopulated(|command| {
        command
            .env("SONIC_SERVER__LOG_LEVEL", "WARN")
            // Disable background tasks (we’ll run them manually).
            .env("SONIC_STORE__FST__GRAPH__CONSOLIDATE_AFTER", "3600")
            .env("SONIC_STORE__FST__POOL__INACTIVE_AFTER", "3700")
            .env("SONIC_STORE__KV__DATABASE__FLUSH_AFTER", "3600")
            .env("SONIC_STORE__KV__POOL__INACTIVE_AFTER", "3700")
    });
    if let Some(manual_seed) = seed {
        *ctx.seed.deref_mut() = manual_seed;
    }
    tracing::info!("Run seed: {}", *ctx.seed);

    let mut bus = bus::Bus::<Message>::new(1);

    // Create actors.
    let actors: Vec<Box<dyn Actor>> = actors(&ctx, &mut bus);
    let node_count = actors.len();

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

    let mut timings = HashMap::with_capacity(node_count);
    let mut send = |event: Event, mut timings: Option<&mut HashMap<String, Duration>>| {
        let barrier = Arc::new(Barrier::new(node_count + 1));
        bus.broadcast(Message {
            event,
            start_barrier: Arc::clone(&barrier),
            end_tx: Arc::clone(&tick_finished_tx),
        });
        tracing::trace!("Barrier {}", std::thread::current().name().unwrap());
        barrier.wait();

        if matches!(event, Event::Finish) {
            return true;
        }

        let mut remaining = node_count;
        while remaining > 0 {
            match tick_finished_rx.recv().unwrap() {
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

                    bus.broadcast(Message {
                        event: Event::Abort,
                        start_barrier: Arc::new(Barrier::new(0)),
                        end_tx: Arc::clone(&tick_finished_tx),
                    });

                    return false;
                }
            }
        }

        true
    };

    let mut iter_failed = false;
    // NOTE: We keep running if iteration validations fail, but return a
    //   failure.
    let mut run_failed = false;
    for tick in 0..iter_count {
        eprint!("\n");
        tracing::debug!("{tick:02}: Tick");

        if !send(Event::Setup(tick), None) {
            iter_failed = true;
            break;
        };

        let tick_start = std::time::Instant::now();

        timings.clear();

        if !send(Event::Tick(tick), Some(&mut timings)) {
            iter_failed = true;
            break;
        };

        let elapsed = tick_start.elapsed();

        tracing::info!("{tick:02}: Tick took {elapsed:.3?} (total)");

        if !validate_iter(&timings, elapsed) {
            run_failed = true;
        }

        if !send(Event::TearDown(tick), None) {
            iter_failed = true;
            break;
        };

        tracing::trace!("Sleeping {SLEEP:?}…");
        std::thread::sleep(SLEEP);
    }

    if iter_failed {
        tracing::error!("Tick failed");
    } else {
        tracing::debug!("Finished");
        send(Event::Finish, None);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    if run_failed {
        panic!("Run failed (see previous logs).");
    }

    tracing::debug!("Quitting");

    assert!(!iter_failed);
}

#[test]
fn test_all_in_one() {
    run_test(
        100,
        Some(17950370156880176289u64),
        |ctx, bus| {
            let write_channel1 = Arc::new(RwLock::new(None));
            let write_channel2 = Arc::new(RwLock::new(None));

            let mut split_mix = SplitMix64::new(*ctx.seed);

            vec![
                Box::new(QueryActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| tick > 1 && !tick.is_multiple_of(4)),
                    collection: "articles".to_owned(),
                    bucket: "default".to_owned(),
                    expected: (String::new(), String::new()),
                    next_expected: Arc::clone(&write_channel1),
                }),
                Box::new(QueryActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| tick > 1 && !tick.is_multiple_of(4)),
                    collection: "articles".to_owned(),
                    bucket: "default".to_owned(),
                    expected: (String::new(), String::new()),
                    next_expected: Arc::clone(&write_channel1),
                }),
                Box::new(PushActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| tick.is_multiple_of(3)),
                    seed: SplitMix64::new(split_mix.next_u64()),
                    collection: "articles".to_owned(),
                    bucket: "default".to_owned(),
                    next_write: None,
                    last_write: write_channel1,
                }),
                Box::new(QueryActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| tick > 2 && !tick.is_multiple_of(4)),
                    collection: "articles".to_owned(),
                    bucket: "default".to_owned(),
                    expected: (String::new(), String::new()),
                    next_expected: Arc::clone(&write_channel2),
                }),
                Box::new(QueryActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| tick > 2 && !tick.is_multiple_of(4)),
                    collection: "articles".to_owned(),
                    bucket: "default".to_owned(),
                    expected: (String::new(), String::new()),
                    next_expected: Arc::clone(&write_channel2),
                }),
                Box::new(PushActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| (tick + 1).is_multiple_of(3)),
                    seed: SplitMix64::new(split_mix.next_u64()),
                    collection: "articles".to_owned(),
                    bucket: "default".to_owned(),
                    next_write: None,
                    last_write: write_channel2,
                }),
                Box::new(ConsolidateActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| (tick + 1).is_multiple_of(5)),
                }),
                Box::new(FlushActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| (tick + 1).is_multiple_of(5)),
                    fail_if_not_found: false,
                }),
            ]
        },
        |_timings, _total_elapsed| {
            // TODO: Find a way to test this.
            true
        },
    );
}

#[test]
fn test_parallel_writes() {
    run_test(
        100,
        Some(17950370156880176289u64),
        |ctx, bus| {
            let mut split_mix = SplitMix64::new(*ctx.seed);
            let seed1 = SplitMix64::new(split_mix.next_u64());

            vec![
                Box::new(PushActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|_tick| true),
                    seed: seed1.clone(),
                    collection: "articles".to_owned(),
                    bucket: "default".to_owned(),
                    next_write: None,
                    last_write: Arc::new(RwLock::new(None)),
                }),
                Box::new(PushActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|_tick| true),
                    seed: seed1,
                    collection: "articles".to_owned(),
                    bucket: "default".to_owned(),
                    next_write: None,
                    last_write: Arc::new(RwLock::new(None)),
                }),
                Box::new(PushActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|_tick| true),
                    seed: SplitMix64::new(split_mix.next_u64()),
                    collection: "articles".to_owned(),
                    bucket: "default".to_owned(),
                    next_write: None,
                    last_write: Arc::new(RwLock::new(None)),
                }),
            ]
        },
        |timings, total_elapsed| {
            // Ensure total tick is `0(1)`.
            if total_elapsed >= timings.values().min().unwrap().mul(2) {
                tracing::error!("Tick is not `O(1)`.");
                return false;
            }

            true
        },
    );
}

#[test]
fn test_parallel_reads() {
    run_test(
        100,
        Some(0u64),
        |ctx, bus| {
            (0..3)
                .map(|_| {
                    Box::new(QueryActor {
                        addr: ctx.addr,
                        inbox: bus.add_rx(),
                        condition: Box::new(|_tick| true),
                        collection: "articles".to_owned(),
                        bucket: "default".to_owned(),
                        expected: (String::new(), String::new()),
                        next_expected: Arc::new(RwLock::new(Some((
                            "article:3".to_owned(),
                            "European Union".to_owned(),
                        )))),
                    }) as Box<dyn Actor>
                })
                .collect()
        },
        |timings, total_elapsed| {
            // Ensure total tick is `0(1)`.
            if total_elapsed >= timings.values().min().unwrap().mul(2) {
                tracing::error!("Tick is not `O(1)`.");
                return false;
            }

            true
        },
    );
}

#[test]
fn test_parallel_read_writes() {
    run_test(
        100,
        Some(17950370156880176289u64),
        |ctx, bus| {
            let write_channel1 = Arc::new(RwLock::new(None));
            let write_channel2 = Arc::new(RwLock::new(None));

            let mut split_mix = SplitMix64::new(*ctx.seed);

            vec![
                Box::new(QueryActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| tick > 1),
                    collection: "articles".to_owned(),
                    bucket: "default".to_owned(),
                    expected: (String::new(), String::new()),
                    next_expected: Arc::clone(&write_channel1),
                }),
                Box::new(QueryActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| tick > 1),
                    collection: "articles".to_owned(),
                    bucket: "default".to_owned(),
                    expected: (String::new(), String::new()),
                    next_expected: Arc::clone(&write_channel1),
                }),
                Box::new(PushActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| tick.is_multiple_of(2)),
                    seed: SplitMix64::new(split_mix.next_u64()),
                    collection: "articles".to_owned(),
                    bucket: "default".to_owned(),
                    next_write: None,
                    last_write: write_channel1,
                }),
                Box::new(QueryActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| tick > 1),
                    collection: "articles".to_owned(),
                    bucket: "default".to_owned(),
                    expected: (String::new(), String::new()),
                    next_expected: Arc::clone(&write_channel2),
                }),
                Box::new(QueryActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| tick > 1),
                    collection: "articles".to_owned(),
                    bucket: "default".to_owned(),
                    expected: (String::new(), String::new()),
                    next_expected: Arc::clone(&write_channel2),
                }),
                Box::new(PushActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| tick.is_multiple_of(2)),
                    seed: SplitMix64::new(split_mix.next_u64()),
                    collection: "articles".to_owned(),
                    bucket: "default".to_owned(),
                    next_write: None,
                    last_write: write_channel2,
                }),
                Box::new(ConsolidateActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| (tick + 1).is_multiple_of(2)),
                }),
            ]
        },
        |_timings, _total_elapsed| {
            // TODO: Find a way to test this.
            true
        },
    );
}

#[test]
fn test_flush_impact() {
    run_test(
        100,
        Some(17950370156880176289u64),
        |ctx, bus| {
            let mut split_mix = SplitMix64::new(*ctx.seed);

            vec![
                Box::new(PushActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|_tick| true),
                    seed: SplitMix64::new(split_mix.next_u64()),
                    collection: "articles".to_owned(),
                    bucket: "default".to_owned(),
                    next_write: None,
                    last_write: Arc::new(RwLock::new(None)),
                }),
                Box::new(FlushActor {
                    addr: ctx.addr,
                    inbox: bus.add_rx(),
                    condition: Box::new(|tick| tick > 0 && tick.is_multiple_of(4)),
                    fail_if_not_found: true,
                }),
            ]
        },
        |_timings, _total_elapsed| {
            // We can’t really test this without more info about timings inside
            // of Sonic.
            true
        },
    );
}

// MARK: Actors

#[derive(Debug, Clone)]
struct Message {
    event: Event,
    start_barrier: Arc<Barrier>,
    end_tx: Arc<mpsc::SyncSender<TickResult>>,
}

#[derive(Debug, Clone, Copy)]
enum Event {
    // Sent before a tick. Can be used to generate data.
    Setup(u16),
    Tick(u16),
    // Sent after a tick. Can be used to retrieve data.
    TearDown(u16),
    Finish,
    Abort,
}

enum TickResult {
    Success(Thread, Option<std::time::Duration>),
    Failure,
}

type Inbox = bus::BusReader<Message>;
type RunCondition = Box<dyn Fn(u16) -> bool + Send + 'static>;

trait Actor: Send {
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
            Err(_error) => break,
        };
        tracing::trace!("{thread_name}: {event:?}");

        tracing::trace!("{thread_name}: Barrier");
        start_barrier.wait();

        let mut elapsed_opt: Option<Duration> = None;
        match event {
            Event::Finish => break,
            Event::Abort => break,
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
struct PushActor {
    addr: std::net::SocketAddr,
    inbox: Inbox,
    condition: RunCondition,
    seed: SplitMix64,
    collection: String,
    bucket: String,
    next_write: Option<(String, String)>,
    last_write: Arc<RwLock<Option<(String, String)>>>,
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

                // Truncate text to 32 chars as it will be used as a query.
                // SAFETY: We know it’s ASCII because of `ALPHABET`.
                new_write.as_mut().unwrap().1.truncate(32);

                *last_write.write().unwrap() = new_write;
            },
        );

        // drop(ingest);
    }
}

/// Runs a query.
struct QueryActor {
    addr: std::net::SocketAddr,
    inbox: Inbox,
    condition: RunCondition,
    collection: String,
    bucket: String,
    expected: (String, String),
    next_expected: Arc<RwLock<Option<(String, String)>>>,
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
struct FlushActor {
    addr: std::net::SocketAddr,
    inbox: Inbox,
    condition: RunCondition,
    /// Whether the test should fail if Sonic was compiled without support for
    /// the command.
    fail_if_not_found: bool,
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
struct ConsolidateActor {
    addr: std::net::SocketAddr,
    inbox: Inbox,
    condition: RunCondition,
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
