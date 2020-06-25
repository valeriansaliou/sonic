Sonic Changelog
===============

## 1.2.4 (2020-06-25)

### Bug Fixes

* Fixed multiple deadlocks, which where not noticed in practice by running Sonic at scale, but that are still theoretically possible [[@BurtonQin](https://github.com/BurtonQin), [#213](https://github.com/valeriansaliou/sonic/pull/213), [#211](https://github.com/valeriansaliou/sonic/pull/211)].

### Changes

* Added support for Latin, which is now auto-detected from terms [[@valeriansaliou](https://github.com/valeriansaliou), [e6c5621](https://github.com/valeriansaliou/sonic/commit/e6c5621ba0fabe83b8bc060824951006b373dc3f)].
* Added Latin stopwords [[@valeriansaliou](https://github.com/valeriansaliou), [e6c5621](https://github.com/valeriansaliou/sonic/commit/e6c5621ba0fabe83b8bc060824951006b373dc3f)].
* Dependencies have been bumped to latest versions (namely: `rocksdb`, `radix`, `hashbrown`, `whatlang`) [[@valeriansaliou](https://github.com/valeriansaliou)].

### New Features

* Added a release script, with cross-compilation capabilities (currently for the `x86_64` architecture) [[@valeriansaliou](https://github.com/valeriansaliou), [961bab9](https://github.com/valeriansaliou/sonic/commit/961bab92211295e99f1f6052577fa1aeff459d0c)].

## 1.2.3 (2019-10-14)

### Changes

* RocksDB compression algorithm has been changed from LZ4 to Zstandard, for a slightly better compression ratio, and much better read/write performance; this will be used for new SST files only [[@valeriansaliou](https://github.com/valeriansaliou), [cd4cdfb](https://github.com/valeriansaliou/sonic/commit/cd4cdfb756ae9eccd43dc7e73d2c115b33297714)].
* Dependencies have been bumped to latest versions (namely: `rocksdb`) [[@valeriansaliou](https://github.com/valeriansaliou), [cd4cdfb](https://github.com/valeriansaliou/sonic/commit/cd4cdfb756ae9eccd43dc7e73d2c115b33297714)].

## 1.2.2 (2019-07-12)

### Bug Fixes

* Fixed a regression on optional configuration values not working anymore, due to an issue in the environment variable reading system introduced in `v1.2.1` [[@valeriansaliou](https://github.com/valeriansaliou), [#155](https://github.com/valeriansaliou/sonic/issues/155)].

### Changes

* Optimized some aspects of FST consolidation and pending operations management [[@valeriansaliou](https://github.com/valeriansaliou), [#156](https://github.com/valeriansaliou/sonic/issues/156)].

## 1.2.1 (2019-07-08)

### Changes

* FST graph consolidation is now able to ignore new words when the graph is over configured limits, which are set with the new `store.fst.graph.max_size` and `store.fst.graph.max_words` configuration variables [[@valeriansaliou](https://github.com/valeriansaliou), [53db9c1](https://github.com/valeriansaliou/sonic/commit/53db9c186630a6751c0a85e610cebabace1aee2b)].
* An integration testing infrastructure has been added to the Sonic automated test suite [[@vilunov](https://github.com/vilunov), [#154](https://github.com/valeriansaliou/sonic/pull/154)].
* Configuration values can now be sourced from environment variables, using the `${env.VARIABLE}` syntax in `config.cfg` [[@perzanko](https://github.com/perzanko), [#148](https://github.com/valeriansaliou/sonic/pull/148)].
* Dependencies have been bumped to latest versions (namely: `rand`, `radix` and `hashbrown`) [[@valeriansaliou](https://github.com/valeriansaliou), [c1b1f54](https://github.com/valeriansaliou/sonic/commit/c1b1f54ad836df553bec0cd14f041bb34058307c)].

## 1.2.0 (2019-05-03)

### Bug Fixes

* Fixed a rare deadlock occuring when 3 concurrent operations get executed on different threads for the same collection, in the following timely order: `PUSH` then `FLUSHB` then `PUSH` [[@valeriansaliou](https://github.com/valeriansaliou), [d96546b](https://github.com/valeriansaliou/sonic/commit/d96546bd9d8b79332df1106766377e4a4acebd50)].

### Changes

* Reworked the KV store manager to perform periodic memory flushes to disk, thus reducing startup time [[@valeriansaliou](https://github.com/valeriansaliou), [6713488](https://github.com/valeriansaliou/sonic/commit/6713488af3543bca33be6e772936f9668430ba86)].
* Stop accepting Sonic Channel commands when shutting down Sonic [[@valeriansaliou](https://github.com/valeriansaliou), [#131](https://github.com/valeriansaliou/sonic/issues/131)].

### New Features

* Introduced a server statistics `INFO` command to Sonic Channel [[@valeriansaliou](https://github.com/valeriansaliou), [#70](https://github.com/valeriansaliou/sonic/issues/70)].
* Added the ability to disable the lexer for a command with the command modifier `LANG(none)` [[@valeriansaliou](https://github.com/valeriansaliou), [#108](https://github.com/valeriansaliou/sonic/issues/108)].
* Added a backup and restore system for both KV and FST stores, which can be triggered over Sonic Channel with `TRIGGER backup` and `TRIGGER restore` [[@valeriansaliou](https://github.com/valeriansaliou), [#5](https://github.com/valeriansaliou/sonic/issues/5)].
* Added the ability to disable KV store WAL (Write-Ahead Log) with the `write_ahead_log` option, which helps limit write wear on heavily loaded SSD-backed servers [[@valeriansaliou](https://github.com/valeriansaliou), [#130](https://github.com/valeriansaliou/sonic/issues/130)].

## 1.1.9 (2019-03-29)

### Bug Fixes

* RocksDB has been bumped to `v5.18.3`, which fixes a dead-lock occuring in RocksDB at scale when a compaction task is ran under heavy disk writes (ie. disk flushes). This dead-lock was causing Sonic to stop responding to any command issued for the frozen collection. This dead-lock was due to a bug in RocksDB internals (not originating from Sonic itself) [[@baptistejamin](https://github.com/baptistejamin), [19c4a10](https://github.com/baptistejamin/sonic/commit/19c4a104a6d6aaed1dd9beb2e51d2639627825cd)].

### Changes

* Reworked the `FLUSHB` command internals, which now use the atomic `delete_range()` operation provided by RocksDB `v5.18` [[@valeriansaliou](https://github.com/valeriansaliou), [660f8b7](https://github.com/valeriansaliou/sonic/commit/660f8b714d968400fb9f88a245752dca02249bf7)].

### New Features

* Added the `LANG(<locale>)` command modifier for `QUERY` and `PUSH`, that lets a Sonic Channel client force a text locale (instead of letting the lexer system guess the text language) [[@valeriansaliou](https://github.com/valeriansaliou), [#75](https://github.com/valeriansaliou/sonic/issues/75)].
* The FST word lookup system, used by the `SUGGEST` command, now support all scripts via a restricted Unicode range forward scan [[@valeriansaliou](https://github.com/valeriansaliou), [#64](https://github.com/valeriansaliou/sonic/issues/64)].

## 1.1.8 (2019-03-27)

### Bug Fixes

* A store acquire lock has been added to prevent 2 concurrent threads from opening the same collection at the same time [[@valeriansaliou](https://github.com/valeriansaliou), [2628077](https://github.com/valeriansaliou/sonic/commit/2628077ebe7e24155975962471e7653745a0add7)].

## 1.1.7 (2019-03-27)

### Bug Fixes

* A superfluous mutex was removed from KV and FST store managers, in an attempt to solve a rare dead-lock occurring on high-traffic Sonic setups in the KV store [[@valeriansaliou](https://github.com/valeriansaliou), [60566d2](https://github.com/valeriansaliou/sonic/commit/60566d2f087fd6725dba4a60c3c5a3fef7e8399b)].

## 1.1.6 (2019-03-27)

### Changes

* Reverted changes made in `v1.1.5` regarding the open files `rlimit`, as this can be set from outside Sonic [[@valeriansaliou](https://github.com/valeriansaliou), [f6400c6](https://github.com/valeriansaliou/sonic/commit/f6400c61a9a956130ae0bdaa9a164f4955cd2a18)].
* Added Chinese Traditional stopwords [[@dsewnr](https://github.com/dsewnr), [#87](https://github.com/valeriansaliou/sonic/issues/87)].

### Bug Fixes

* Improved the way database locking is handled when calling a pool janitor; this prevents potential dead-locks under high load [[@valeriansaliou](https://github.com/valeriansaliou), [fa78372](https://github.com/valeriansaliou/sonic/commit/fa783728fd27a116b8dcf9a7180740d204b69aa4)].

## 1.1.5 (2019-03-27)

### New Features

* Added the `server.limit_open_files` configuration variable to allow configuring `rlimit` [[@valeriansaliou](https://github.com/valeriansaliou)].

## 1.1.4 (2019-03-27)

### Changes

* Added Kannada stopwords [[@dileepbapat](https://github.com/dileepbapat)].
* The Docker image is now much lighter [[@codeflows](https://github.com/codeflows)].

### New Features

* Automatically adjust `rlimit` for the process to the hard limit allowed by the system (allows opening more FSTs in parallel) [[@valeriansaliou](https://github.com/valeriansaliou)].

## 1.1.3 (2019-03-25)

### Changes

* Limit the size of words that can hit against the FST graph, as the FST gets slower for long words [[@valeriansaliou](https://github.com/valeriansaliou), [#81](https://github.com/valeriansaliou/sonic/issues/81)].

### Bug Fixes

* Rework Sonic Channel buffer management using a VecDeque (Sonic should now work better in harsh network environments) [[@valeriansaliou](https://github.com/valeriansaliou), [1c2b9c8](https://github.com/valeriansaliou/sonic/commit/1c2b9c8fcd28b033a7cb80d678c388ce78ab989d)].

## 1.1.2 (2019-03-24)

### Changes

* FST graph consolidation locking strategy has been improved even further, based on issues with the previous rework we have noticed at scale in production (now, consolidation locking is done at a lower-priority relative to actual queries and pushes to the index) [[@valeriansaliou](https://github.com/valeriansaliou), [#68](https://github.com/valeriansaliou/sonic/issues/68)].

## 1.1.1 (2019-03-24)

### Changes

* FST graph consolidation locking strategy has been reworked as to allow queries to be executed lock-free when the FST consolidate task takes a lot of time (previously, queries were being deferred due to an ongoing FST consolidate task) [[@valeriansaliou](https://github.com/valeriansaliou), [#68](https://github.com/valeriansaliou/sonic/issues/68)].
* Removed special license clause introduced in `v1.0.2`, Sonic is full `MPL 2.0` now. [[@valeriansaliou](https://github.com/valeriansaliou)]

## 1.1.0 (2019-03-21)

### Breaking Changes

* Change how buckets are stored in a KV-based collection (nest them in the same RocksDB database; this is much more efficient on setups with a large number of buckets - **`v1.1.0` is incompatible with the `v1.0.0` KV database format**) [[@valeriansaliou](https://github.com/valeriansaliou)].

### Changes

* Bump `jemallocator` to version `0.3` [[@valeriansaliou](https://github.com/valeriansaliou)].

## 1.0.2 (2019-03-20)

### Changes

* Re-license from `MPL 2.0` to `SOSSL 1.0` (Sonic has a special license clause) [[@valeriansaliou](https://github.com/valeriansaliou)].

## 1.0.1 (2019-03-19)

### Changes

* Added automated benchmarks (can be ran via `cargo bench --features benchmark`) [[@valeriansaliou](https://github.com/valeriansaliou)].
* Reduced the time to query the search index by 50% via optimizations (in multiple methods, eg. the lexer) [[@valeriansaliou](https://github.com/valeriansaliou)].

## 1.0.0 (2019-03-18)

### New Features

* Initial Sonic release [[@valeriansaliou](https://github.com/valeriansaliou)].
