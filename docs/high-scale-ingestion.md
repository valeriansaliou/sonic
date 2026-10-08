---
author: Rémi Bardon <remi@remibardon.name>
created: 2026-10-01
updated: 2026-10-03
---

# High-scale ingestion in Sonic

This document lists some available techniques to ingest data in Sonic at a high
scale (TB).

A lot of configuration discussed here isn’t enabled by default because such a
scale isn’t a regular use of Sonic, but rather an occasional operation one
might want to do to re-index data (e.g. when migrating to Sonic v2).

Finally, note that 

## Prerequisites

A lot of useful features discussed here are still experimental
(see [“Sonic’s experimental APIs”][x-api]).

To test them, you’ll have to build Sonic yourself.
To do so, follow our instructions under “Trying out experimental Sonic features”
in [“Sonic’s experimental APIs”][x-api].

[x-api]: ./experimental-apis.md "“Sonic’s experimental APIs” in Sonic docs"

## TL;DR

(Each point is detailed later in this guide, this is a summary.)

0. Adjust your configuration

   ```toml
   [store.kv.database]
   # Merge medium-sized memtables into medium L0 SSTs.
   write_buffer_size = 16_384 # 16MiB
   
   # Allow RocksDB to use more threads for background jobs.
   parallelism = # Number of performance cores available to Sonic (server-side).
   max_background_jobs = # parallelism + 1 (max_flushes)
   max_subcompactions = # parallelism
   
   write_ahead_log = false
   
   max_open_files = 20 # Or anything < `ulimit -n`.
   
   [store.object.database]
   # Merge medium-sized memtables into large L0 SSTs.
   write_buffer_size = 65_536 # 64MiB
   
   # Allow RocksDB to use more threads for background jobs.
   parallelism = # Number of performance cores available to Sonic (server-side).
   max_background_jobs = # parallelism + 1 (max_flushes)
   max_subcompactions = # parallelism
   
   write_ahead_log = false
   
   max_open_files = 20 # Or anything < `ulimit -n`.
   ```
0. Prepare for bulk ingestion:

   ```txt
   CONFIG <collection> SET rocksdb.disable_auto_compactions rocksdb.unordered_write
   CONFIG <collection> SET sonic.disable_all_task
   ```
0. Ingest data without wasting compute.

   * Use experimental flags with `PUSH`

     ```txt
     PUSH <collection> <bucket> <object> "<data_chunk_0>" LANG(<lang>) INCOMPLETE CAPACITY(<len>)
     PUSH <collection> <bucket> <object> "<data_chunk_1>" LANG(<lang>) INCOMPLETE
     PUSH <collection> <bucket> <object> "<data_chunk_2>" LANG(<lang>) INCOMPLETE
     ...
     PUSH <collection> <bucket> <object> "<data_chunk_n>" LANG(<lang>) NEW
     ```

     Tip: Out of simplicity, you can pass `CAPACITY` and `NEW` to all commands,
     they will be ignored when unnecessary. The only thing you need is to _not_
     pass `INCOMPLETE` to the last command for a given object.

   * Run this on ~15 Sonic channels in parallel if you have a good processor,
     but find your sweet spot.
   * Ingest buckets in lexicographically sorted order. It’s not a tremendous
     improvement, but we might make improvements to Sonic in the future to make
     this case blazing fast so do it preemptively if you can.
   * Only use `NEW` if you’re ingesting new objects.
0. Prepare for reads:

   ```txt
   TRIGGER flush
   TRIGGER compact
   TRIGGER consolidate
   ```

   Tip: You can run `TRIGGER consolidate` in parallel, it’s unrelated and
   would save you some time at the end.
* Reset Sonic’s dynamic configuration to go back to normal operation:

  ```txt
  CONFIG <collection> RESET
  ```
* (Optional) Restart Sonic with your usual production configuration.

## Detailed explanations

_This section is a work in progress, I’ll finish it later_

### Ingest with multiple Sonic channels in parallel

Until RocksDB becomes the bottleneck, ingestion throughput scales linearly with
the number of Sonic channels used to ingest data in parallel. Using plenty of
them is your biggest leverage that comes merely for free —up to a certain point.

On a MacBook Pro (M4 Pro, 2024), I(@RemiBardon) found that using 15 channels
was a sweet spot, but Sonic is bottlenecked by RocksDB flushes, which happen on
a single thread, so **it really depends on the speed of one core**.

I’d say **start with 10 or 15** if you have a good machine, and make a few tests
to find _your_ sweet spot.

### Enable unordered writes in RocksDB

Ordered writes affect snapshots, which Sonic doesn’t use (and even less so
during ingestion), so it’s not important to us. In fact, it slows RocksDB down
by a factor of 3 in some benchmarks —where we see a **190% throughput increase**.
Enable unordered writes at all costs if you want to ingest fast.

```txt
CONFIG <collection> SET rocksdb.unordered_write
```

### Disable RocksDB automatic compactions

By default, RocksDB compacts automatically when there are too many SSTs in
level zero. It’s not much at the beginning, but when compactions start to
cascade write amplification gets bigger and bigger, causing throughput to
plummet. If you’re ingesting a lot of data, you definitely want to disable
automatic compactions and compact a single time at the end.

```txt
CONFIG <collection> SET rocksdb.disable_auto_compactions
```

### Disable Sonic background tasks

Sonic runs a few tasks in the background, to ensure your index is in the best
condition to serve requests. It’s useless during a bulk ingestion and even gets
in our way, so you should disable it:

```txt
CONFIG <collection> SET sonic.disable_all_task
```

### `PUSH` with `NEW`

If you know an object doesn’t exist in the index (e.g. when re-indexing from
scratch), use the `NEW` flag with `PUSH`. It will cause Sonic to not perform a
single read while indexing a bucket[^one-read].

[^one-read]: Except for the very first object, to initialize the IIDIncr cache.

Although it’s highly dependant on configuration, some benchmarks show a **66%
throughput increase** with this single change so you should definitely not pass
on it unless you really can’t use it.

```txt
PUSH <collection> <bucket> <object> "<data_chunk_0>" NEW
PUSH <collection> <bucket> <object> "<data_chunk_1>" NEW
PUSH <collection> <bucket> <object> "<data_chunk_2>" NEW
...
PUSH <collection> <bucket> <object> "<data_chunk_n>" NEW
```

### `PUSH` with `INCOMPLETE`

If an object is too long to fit in a single command (because of Sonic’s buffer
size), you have to split it. However, splitting an object into $n$ chunks causes
Sonic to make $n$ times as many writes to stores.

To keep writes linearly proportional to the number of ingested objects, you can
add `INCOMPLETE` to all commands except the last one (per object):

```txt
PUSH <collection> <bucket> <object> "<data_chunk_0>" INCOMPLETE
PUSH <collection> <bucket> <object> "<data_chunk_1>" INCOMPLETE
PUSH <collection> <bucket> <object> "<data_chunk_2>" INCOMPLETE
...
PUSH <collection> <bucket> <object> "<data_chunk_n>"
```

Tip: When using `INCOMPLETE`, you can skip `NEW` and set it only on the final
`PUSH`. Out of simplicity, you can still set it every time; Sonic will ignore it.

### Combine `INCOMPLETE` with `CAPACITY`

When using `INCOMPLETE`, Sonic keeps some data in memory between `PUSH`
commands. Because it doesn’t know how much memory to allocate upfront, it’s
forced to re-allocate every time you push a new chunk. To fix that, pass
`CAPACITY(<len>)` to the first `INCOMPLETE` chunk, with `<len>` being the total
size (in bytes) of the object’s value.

### Set `LANG` if you know it

In v2, Sonic doesn’t care as much about languages. It’s only used for stemming
(optional) and switching to specific tokenizers (e.g. Chinese, Japanese). Just
in case, if you know it, it’s still a good practice to inform Sonic of the
language so it doesn’t have to guess:

```txt
PUSH <collection> <bucket> <object> "<data_chunk_0>" LANG(<lang>)
PUSH <collection> <bucket> <object> "<data_chunk_1>" LANG(<lang>)
PUSH <collection> <bucket> <object> "<data_chunk_2>" LANG(<lang>)
...
PUSH <collection> <bucket> <object> "<data_chunk_n>" LANG(<lang>)
```

### Increase RocksDB’s parallelism

At scale, RocksDB is Sonic’s bottleneck. To raise the bar, you should increase
RocksDB’s parallelism so it distributes work across multiple threads.

Under **both** `[store.kv.database]` **and** `[store.object.database]`, set:

```toml
# Allow RocksDB to use more threads for background jobs.
parallelism = # Number of performance cores available to Sonic (server-side).
max_background_jobs = # parallelism + 2 (max_flushes)
max_subcompactions = # parallelism
max_flushes = 2 # 1 for KV store, 1 for Object store
```

As stated in the comments, you should set `parallelism` to the number of
performance cores available to Sonic (server-side). RocksDB will still use a
single thread per connection (so 2 in total, for a single collection) during
ingestion, but **it will speed up the final compaction** (which will be
parallelized).

Note: We know it’s annoying to add this in two sections, we’ll improve that
someday.

### Disable RocksDB’s Write-Ahead Log

Under **both** `[store.kv.database]` **and** `[store.object.database]`, set:

```toml
write_ahead_log = false
```

### Increase RocksDB’s buffer sizes

Filesystem I/O takes time. To write less often during a bulk ingestion, we can
increase RocksDB’s buffer sizes. We found this to be a good fit:

```toml
[store.kv.database]
# Merge medium-sized memtables into medium L0 SSTs.
write_buffer_size = 16_384 # 16MiB
min_write_buffer_number_to_merge = 4 # L0 SST ⪅ 64MiB
max_write_buffer_number = 64 # Do not stop writes while a flush is in progress (max 1GiB RAM usage).

[store.object.database]
# Merge medium-sized memtables into large L0 SSTs.
write_buffer_size = 65_536 # 64MiB
min_write_buffer_number_to_merge = 4 # L0 SST ⪅ 256MiB
max_write_buffer_number = 16 # Do not stop writes while a flush is in progress (max 1GiB RAM usage).
```

### Ingest buckets in lexicographically sorted order

It’s not a tremendous improvement, but we might make improvements to Sonic in
the future to make this case blazing fast so do it preemptively **if you can**.

### Make sure `max_open_files` is correct

During normal operation, it’s easy to not notice `max_open_files` is too high
(because RocksDB automatically closes open files). However, when ingesting a
lot of data very fast RocksDB keeps a lot of files open in cache and it becomes
very easy to reach `ulimit -n`.

Since we don’t read during a bulk ingestion, you can set `max_open_files = 20`
under **both** `[store.kv.database]` **and** `[store.object.database]`.

You can set it to anything lower than `ulimit -n`, but using `20` won’t slow
you down so don’t bother finding a good value.
