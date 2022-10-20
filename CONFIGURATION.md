Sonic Configuration
===================

# File: config.cfg

**All available configuration options are commented below, with allowed values:**

**[server]**

* `log_level` (type: _string_, allowed: `debug`, `info`, `warn`, `error`, default: `error`) — Verbosity of logging, set it to `error` in production

**[channel]**

* `inet` (type: _string_, allowed: IPv4 / IPv6 + port, default: `[::1]:1491`) — Host and TCP port Sonic Channel should listen on
* `tcp_timeout` (type: _integer_, allowed: seconds, default: `300`) — Timeout of idle/dead client connections to Sonic Channel
* `auth_password` (type: _string_, allowed: password values, default: none) — Authentication password required to connect to the channel (optional but recommended)

**[channel.search]**

* `query_limit_default` (type: _integer_, allowed: numbers, default: `10`) — Default search results limit for a query command (if the LIMIT command modifier is not used when issuing a QUERY command)
* `query_limit_maximum` (type: _integer_, allowed: numbers, default: `100`) — Maximum search results limit for a query command (if the LIMIT command modifier is being used when issuing a QUERY command)
* `query_alternates_try` (type: _integer_, allowed: numbers, default: `4`) — Number of alternate words that look like query word to try if there are not enough query results (if zero, no alternate will be tried; if too high there may be a noticeable performance penalty)
* `suggest_limit_default` (type: _integer_, allowed: numbers, default: `5`) — Default suggested words limit for a suggest command (if the LIMIT command modifier is not used when issuing a SUGGEST command)
* `suggest_limit_maximum` (type: _integer_, allowed: numbers, default: `20`) — Maximum suggested words limit for a suggest command (if the LIMIT command modifier is being used when issuing a SUGGEST command)
* `list_limit_default` (type: _integer_, allowed: numbers, default: `100`) — Default listed words limit for a list command (if the LIMIT command modifier is not used when issuing a LIST command)
* `list_limit_maximum` (type: _integer_, allowed: numbers, default: `500`) — Maximum listed words limit for a list command (if the LIMIT command modifier is being used when issuing a LIST command)

**[store]**

**[store.kv]**

* `path` (type: _string_, allowed: UNIX path, default: `./data/store/kv/`) — Path to the Key-Value database store
* `retain_word_objects` (type: _integer_, allowed: numbers, default: `1000`) — Maximum number of objects a given word in the index can be linked to (older objects are cleared using a sliding window)

**[store.kv.pool]**

* `inactive_after` (type: _integer_, allowed: seconds, default: `1800`) — Time after which a cached database is considered inactive and can be closed (if it is not used, ie. re-activated)

**[store.kv.database]**

* `flush_after` (type: _integer_, allowed: seconds, default: `900`) — Time after which pending database updates should be flushed from memory to disk (increase this delay if you encounter high-CPU usage issues when a flush task kicks-in; this value should be lower than `store.kv.pool.inactive_after`)
* `compress` (type: _boolean_, allowed: `true`, `false`, default: `true`) — Whether to compress database or not (uses Zstandard)
* `parallelism` (type: _integer_, allowed: numbers, default: `2`) — Limit on the number of compaction and flush threads that can run at the same time
* `max_files` (type: _integer_, allowed: numbers, no default) — Maximum number of database files kept open at the same time per-database (if any; otherwise there are no limits)
* `max_compactions` (type: _integer_, allowed: numbers, default: `1`) — Limit on the number of concurrent database compaction jobs
* `max_flushes` (type: _integer_, allowed: numbers, default: `1`) — Limit on the number of concurrent database flush jobs
* `write_buffer` (type: _integer_, allowed: numbers, default: `16384`) — Maximum size in KB of the database write buffer, after which data gets flushed to disk (ie. `16384` is `16MB`; the size should be a multiple of `1024`, eg. `128 * 1024 = 131072` for `128MB`)
* `write_ahead_log` (type: _boolean_, allowed: `true`, `false`, default: `true`) — Whether to enable Write-Ahead Log or not (it avoids losing non-flushed data in case of server crash)

**[store.fst]**

* `path` (type: _string_, allowed: UNIX path, default: `./data/store/fst/`) — Path to the Finite-State Transducer database store

**[store.fst.pool]**

* `inactive_after` (type: _integer_, allowed: seconds, default: `300`) — Time after which a cached graph is considered inactive and can be closed (if it is not used, ie. re-activated)

**[store.fst.graph]**

* `consolidate_after` (type: _integer_, allowed: seconds, default: `180`) — Time after which a graph that has pending updates should be consolidated (increase this delay if you encounter high-CPU usage issues when a consolidation task kicks-in; this value should be lower than `store.fst.pool.inactive_after`)
* `max_size` (type: _integer_, allowed: numbers, default: `2048`) — Maximum size in KB of the graph file on disk, after which further words are not inserted anymore (ie. `2048` is `2MB`; the size should be a multiple of `1024`, eg. `8 * 1024 = 8192` for `8MB`; use this limit to prevent heavy graphs to be consolidating forever; this limit is enforced in pair with `store.fst.graph.max_words`, whichever is reached first)
* `max_words` (type: _integer_, allowed: numbers, default: `250000`) — Maximum number of words that can be held at the same time in the graph, after which further words are not inserted anymore (use this limit to prevent heavy graphs to be consolidating forever; this limit is enforced in pair with `store.fst.graph.max_size`, whichever is reached first)

# Command-Line: Environment variables

You are allowed to use environment variables in the configuration file.

**You can provide them as follows:**

```toml
[channel]

auth_password = "${env.SECRET}"
```

**Then, you can run Sonic providing a defined environment variable:**

```bash
SECRET=secretphrase ./sonic -c /path/to/config.cfg
```

_Note that this can only be used with string-like values._
