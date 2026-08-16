# Sonic Configuration

## Configuration sources

Sonic looks for its configuration as [TOML] in `./config.cfg`, or whatever you
passed via `--config`/`-c`.

This file is optional, as all configuration keys can be defined using
`SONIC_` environment variables (which take precedence over the static
configuration). The path separator is `__`, which means `foo.bar.baz` will be
read from `SONIC_FOO__BAR__BAZ`.

## Configuration keys reference

**All available configuration options are commented below, with allowed values:**

### Server configuration

Under `[server]`:

* `log_level` (type: _string_, allowed: `debug`, `info`, `warn`, `error`, default: `error`) — Verbosity of logging, set it to `error` in production

### Channel configuration

Under `[channel]`:

* `inet` (type: _string_, allowed: IPv4 / IPv6 + port, default: `[::1]:1491`) — Host and TCP port Sonic Channel should listen on
* `tcp_timeout` (type: _integer_, allowed: seconds, default: `300`) — Timeout of idle/dead client connections to Sonic Channel
* `auth_password` (type: _string_, allowed: password values, default: none) — Authentication password required to connect to the channel (optional but recommended)

`channel.search` has been deprecated in favor of `search`, but it’s kept as an alias for
backward compatibility reasons.

### Normalization configuration

Warning: After making changes to normalization steps, you must rebuild Sonic’s
index by re-ingesting all data. To avoid such breaking change, new features are
disabled by default (opt-in). On major releases, some steps might become
enabled by default (opt-out). Sonic won’t enable unreasonable defaults, but
override if you need consistency.

Under `[normalization]`:

* `unicode_normalization` (type: _string_ (optional), allowed: `"nfc"`, `"nfkc"`, default: none, recommended: `"nfkc"`) — Whether to normalize Unicode characters (see [“Unicode equivalence” on Wikipedia](https://en.wikipedia.org/wiki/Unicode_equivalence)) when ingesting and querying. It is recommended to enable Unicode normalization, but Sonic makes it opt-in (until next major release) for backward compatibility reasons. When enabled, Sonic queries will be faster and your Sonic index smaller, for better results.
* `diacritic_folding_enabled` (type: _boolean_, allowed: `true`, `false`, default: `false`) — Whether to enable [diacritic](https://en.wikipedia.org/wiki/Diacritic) folding or not (it reduces the index size and improves results)
* `stemming_enabled` (type: _boolean_, allowed: `true`, `false`, default: `false`) — Whether to enable [stemming](https://en.wikipedia.org/wiki/Stemming) or not (it avoids losing non-flushed data in case of server crash)
  * Warning: Enabling stemming greatly affects the quality of Sonic results. Enable only if you have a good reason to.

### Tokenization configuration

Warning: After making changes to tokenization steps, you must rebuild Sonic’s
index by re-ingesting all data. To avoid such breaking change, new features are
disabled by default (opt-in). On major releases, some steps might become
enabled by default (opt-out). Sonic won’t enable unreasonable defaults, but
override if you need consistency.

Under `[tokenization]`:

* `detect_special_patterns` (type: _boolean_, allowed: `true`, `false`, default: `true`) — Whether the tokenizer should detect special patterns or not
  * Sonic does fuzzy matching by default. However, some search terms are
    usually expected to match exactly, like email addresses. To support this
    use case, Sonic detects common patterns (e.g. email addresses, phone
    numbers, UUIDs, etc. and ensures they are both not split by the tokenizer
    (unless `tokenization.compat_split_special_patterns = true`) and matched
    exactly in queries.
  * For more information, see [docs/tokenizer-pattern-matching.md](./docs/tokenizer-pattern-matching.md).
  * This feature adds negligible overhead, you should probably not disable it.
* `compat_split_special_patterns` (type: _boolean_, allowed: `true`, `false`, default: `true`) — Whether the tokenizer should split special patterns or not
  * Special patterns are matched exactly when performing a query. However,
    doing so without rebuilding your Sonic index breaks queries with special
    patterns. This flag enables a compatibility feature that integrates with an
    existing inex (at the cost of potentially worse results).
  * For more information, see [docs/tokenizer-pattern-matching.md](./docs/tokenizer-pattern-matching.md).
  * You don’t need to rebuild your Sonic index if you use
    `tokenization.compat_split_special_patterns = true` (default).
  * If you can easily rebuild your Sonic index and sometimes query things like
    email addresses, phone numbers or identifiers, it is recommended that you
    disable this feature.

### Stopwords configuration

Under `[stopwords]`:

* `allow` (type: _array_, allowed: strings, default: `[]`) — Sonic has opinionated localized stopwords, but you might want to index some of them. `stopwords.allow` allows you to define a set of words which will never be considered stopwords.
  * Note that this configuration key will likely be deleted in Sonic v2 as hardcoded stopwords will likely be removed (see [issue #379 “Remove hardcoded stopwords”](https://github.com/valeriansaliou/sonic/issues/379)).
* `deny` (type: _array_, allowed: strings, default: `[]`) — If there are words you don’t want to index, here is where you can inform Sonic of it. Words in this list will be ignored when processing text.

### Search configuration

Under `[search]`:

* `query_limit_default` (type: _integer_, allowed: numbers, default: `10`) — Default search results limit for a query command (if the LIMIT command modifier is not used when issuing a QUERY command)
* `query_limit_maximum` (type: _integer_, allowed: numbers, default: `100`) — Maximum search results limit for a query command (if the LIMIT command modifier is being used when issuing a QUERY command)
* `query_alternates_try` (type: _integer_, allowed: numbers, default: `4`) — Number of alternate words that look like query word to try if there are not enough query results (if zero, no alternate will be tried; if too high there may be a noticeable performance penalty)
* `query_minimum_term_idf_default` (type: _float_, allowed: numbers in `[0;1]`, default: `0.1`) — Minimum [idf](https://en.wikipedia.org/wiki/Tf–idf#Inverse_document_frequency) of tokens taken into account in a query command. Can be used to avoid low-quality results at the end of the results list.
* `query_minimum_term_idf_minimum_object_count` (type: _integer_, allowed: numbers, default: `100`) — Minimum number of objects in a bucket for `query_minimum_term_idf_default` to be taken into account.
* `suggest_limit_default` (type: _integer_, allowed: numbers, default: `5`) — Default suggested words limit for a suggest command (if the LIMIT command modifier is not used when issuing a SUGGEST command)
* `suggest_limit_maximum` (type: _integer_, allowed: numbers, default: `20`) — Maximum suggested words limit for a suggest command (if the LIMIT command modifier is being used when issuing a SUGGEST command)
* `list_limit_default` (type: _integer_, allowed: numbers, default: `100`) — Default listed words limit for a list command (if the LIMIT command modifier is not used when issuing a LIST command)
* `list_limit_maximum` (type: _integer_, allowed: numbers, default: `500`) — Maximum listed words limit for a list command (if the LIMIT command modifier is being used when issuing a LIST command)

### KV store configuration

Under `[store.kv]`:

* `path` (type: _string_, allowed: UNIX path, default: `./data/store/kv/`) — Path to the Key-Value database store
* `retain_word_objects` (type: _integer_, allowed: numbers, default: `1000`) — Maximum number of objects a given word in the index can be linked to (older objects are cleared using a sliding window)

* `pool.inactive_after` (type: _integer_, allowed: seconds, default: `1800`) — Time after which a cached database is considered inactive and can be closed (if it is not used, ie. re-activated)

* `database.flush_after` (type: _integer_, allowed: seconds, default: `900`) — Time after which pending database updates should be flushed from memory to disk (increase this delay if you encounter high-CPU usage issues when a flush task kicks-in; this value should be lower than `store.kv.pool.inactive_after`)
* `database.parallelism` (type: _integer_, allowed: numbers, default: `2`) — Limit on the number of compaction and flush threads that can run at the same time
* `database.max_files` (type: _integer_, allowed: numbers, no default) — Maximum number of database files kept open at the same time per-database (if any; otherwise there are no limits)
* `database.write_buffer_size` (type: _integer_, allowed: numbers, default: `16384`) — Maximum size **in KiB** of the database write buffer, after which data gets flushed to disk (ie. `16384` is `16MiB`; the size should be a multiple of `1024`, eg. `128 * 1024 = 131072` for `128MiB`)
* `database.min_write_buffer_number` (type: _integer_, allowed: `≥1`, default: RocksDB default (`1`)) — Minimum number of memtables that must be written before they can be flushed together to disk (increasing this batches small flushes into fewer, larger ones, at the cost of more memory held before a flush)
* `database.min_write_buffer_number_to_merge` (type: _integer_, allowed: `≥1`, default: RocksDB default (`1`)) — Minimum number of immutable memtables that must accumulate before they are merged and flushed together (higher values reduce write amplification from small flushes, at the cost of more memory usage)
* `database.max_write_buffer_number` (type: _integer_, allowed: `≥2`, default: RocksDB default (`2`)) — Maximum number of memtables, both active and immutable, that can be held in memory at once (once this limit is reached, writes are stalled until a flush completes)
* `database.block_cache_size` (type: _integer_, allowed: `≥0`, default: RocksDB default) — Size in bytes of the LRU cache used to hold uncompressed data blocks read from disk (a larger cache improves read performance at the cost of memory usage)
* `database.cache_index_and_filter_blocks` (type: _boolean_, allowed: `true`, `false`, default: RocksDB default) — Whether index and filter blocks should be stored in the block cache rather than held in memory outside of it (enable this to keep total memory usage bounded and predictable)
* `database.compression_type` (type: _string_, allowed: `"none"`, `"zstd"`, default: `"zstd"`) — How the database should be compressed (use `"none"` for no compression)
* `database.compression_level` (type: _integer_, default: RocksDB default (`32767`)) — Compression level to use with the configured compression algorithm (higher values improve compression ratio at the cost of speed; `32767` tells RocksDB to use the algorithm's own default)
* `database.min_level_to_compress` (type: _integer_, allowed: `≥0`, default: RocksDB default) — First LSM-tree level at which compression is applied (lower levels are left uncompressed to keep recent writes fast; set to `-1` to compress every level)
* `database.write_ahead_log` (type: _boolean_, allowed: `true`, `false`, default: `true`) — Whether to enable Write-Ahead Log or not (it avoids losing non-flushed data in case of server crash)
* `database.wal_compression_type` (type: _string_, allowed: `"none"`, `"zstd"`, default: `"none"`) — How Write-Ahead Log records should be compressed (use `"none"` for no compression)
* `database.wal_ttl_seconds` (type: _integer_, allowed: `≥0`, default: RocksDB default (`0`)) — Number of seconds archived Write-Ahead Log files are retained for before being purged (`0` disables time-based purging)
* `database.wal_size_limit_mb` (type: _integer_, allowed: `≥0`, default: RocksDB default (`0`)) — Total size in MB that archived Write-Ahead Log files may occupy before the oldest ones are purged (`0` disables size-based purging)
* `database.wal_bytes_per_sync` (type: _integer_, allowed: `≥0`, default: RocksDB default (`0`)) — Number of bytes written to the Write-Ahead Log between each automatic sync to disk (`0` disables incremental syncing, relying on the OS to flush writes)
* `database.wal_recovery_mode` (type: _string_, allowed: `"tolerate_corrupted_tail_records"`, `"absolute_consistency"`, `"point_in_time"`, `"skip_any_corrupted_record"`, default: RocksDB default (`"point_in_time"`)) — How the database should handle Write-Ahead Log corruption when recovering after a crash (from strictest, refusing any inconsistency, to most lenient, skipping corrupted records to recover as much data as possible)
* `database.level_zero_file_num_compaction_trigger` (type: _integer_, default: RocksDB default (`4`)) — Number of level-0 files that triggers a compaction
* `database.level_zero_slowdown_writes_trigger` (type: _integer_, default: RocksDB default (`20`)) — Number of level-0 files at which writes start being throttled to let compaction catch up
* `database.level_zero_stop_writes_trigger` (type: _integer_, default: RocksDB default (`24`)) — Number of level-0 files at which writes are stopped entirely until compaction catches up
* `database.max_bytes_for_level_base` (type: _integer_, allowed: `≥0`, default: RocksDB default (`0x10000000` (256MiB))) — Target total size in bytes of level-1 of the LSM-tree (the target size of each subsequent level is derived from this and `max_bytes_for_level_multiplier`)
* `database.max_bytes_for_level_multiplier` (type: _float_, allowed: `>0`, default: RocksDB default (`10`)) — Factor by which the target size of each LSM-tree level grows over the previous one (eg. with the default of `10`, level-2 targets 10x the size of level-1)
* `database.target_file_size_base` (type: _u64_, allowed: `0`, default: RocksDB default (`0x4000000` (64MiB)) — Target size in bytes of an individual SST file at level-1 (file sizes at later levels scale up using the same growth factor as `max_bytes_for_level_multiplier`)
* `database.max_background_jobs` (type: _integer_, allowed: `≥2`, default: RocksDB default (`2`)) — Maximum number of background threads available for flush and compaction jobs combined
* `database.max_subcompactions` (type: _integer_, allowed: `≥1`, default: RocksDB default (`1`)) — Limit on the number of concurrent database compaction jobs
* `database.stats_dump_period_sec` (type: _integer_, allowed: `≥0`, default: RocksDB default (`600` (10 mins))) — Interval in seconds at which RocksDB writes internal statistics (throughput, compaction stats, cache hit rates, etc.) to its log file (`0` disables periodic dumping)

### FST store configuration

Under `[store.fst]`:

* `path` (type: _string_, allowed: UNIX path, default: `./data/store/fst/`) — Path to the Finite-State Transducer database store

* `pool.inactive_after` (type: _integer_, allowed: seconds, default: `300`) — Time after which a cached graph is considered inactive and can be closed (if it is not used, ie. re-activated)

* `graph.consolidate_after` (type: _integer_, allowed: seconds, default: `180`) — Time after which a graph that has pending updates should be consolidated (increase this delay if you encounter high-CPU usage issues when a consolidation task kicks-in; this value should be lower than `store.fst.pool.inactive_after`)
* `graph.max_size` (type: _integer_, allowed: numbers, default: `2048`) — Maximum size in KB of the graph file on disk, after which further words are not inserted anymore (ie. `2048` is `2MB`; the size should be a multiple of `1024`, eg. `8 * 1024 = 8192` for `8MB`; use this limit to prevent heavy graphs to be consolidating forever; this limit is enforced in pair with `store.fst.graph.max_words`, whichever is reached first)
* `graph.max_words` (type: _integer_, allowed: numbers, default: `250000`) — Maximum number of words that can be held at the same time in the graph, after which further words are not inserted anymore (use this limit to prevent heavy graphs to be consolidating forever; this limit is enforced in pair with `store.fst.graph.max_size`, whichever is reached first)

## Environment variables interpolation

Some configuration keys —namely `server.log_level`, `channel.inet`,
`channel.auth_password`, `store.kv.path` and `store.fst.path`— support
environment variable interpolation. If you set `"${env.SECRET}"` for one of
those keys, the value will be expanded from the `SECRET` environment variable.

[TOML]: https://toml.io/
