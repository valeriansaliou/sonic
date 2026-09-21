# Sonic core Changelog

<!-- markdownlint-disable no-duplicate-heading -->

## [Unreleased]

<!-- WARN: Do not move the next line and add changelog entries **under** it.
       It’s used by `task release:*` when updating the changelog. -->
[Unreleased]: https://github.com/valeriansaliou/sonic/compare/core-v0.4.0...HEAD

## [0.4.0] (2026-09-21)

[0.4.0]: https://github.com/valeriansaliou/sonic/compare/core-v0.3.0...core-v0.4.0

### Changes

* feat!: Rework the tokenizer to make its API more flexible (in `c264253`)
* feat!: Make `countb` return the number of objects in the bucket, instead of term count (in `5bc6082`)

Dependency updates:

* Remove dependency to `byteorder` (in `b79c329`)
* Remove dependency to `radix` (in `2fd3ecf`)

### Bug Fixes

* Fix `u32_max` merge operator really being `u32_min`… (in `ec6a9b9`)
* Fix `get_new_iid` not initializing from database (in `a60b3af`)
* Fix document count after a `FLUSHO` (in `060a899`)

## [0.3.0] (2026-09-15)

[0.3.0]: https://github.com/valeriansaliou/sonic/compare/core-v0.2.1...core-v0.3.0

This version introduces substantial performance improvements to `PUSH` requests.

### Changes

Performance improvements:

* Reduce locks contention caused by `IIDIncr` (in `f7de118`)
* Use a single `rocksdb::WriteBatch` per Sonic request (in `606cb31`, `981d1bc`)
* Use merge operators for `TermToIIDs` and `IIDToTerms` (in `b5921a1`, `6bd24cb`)
* Perform fewer merge operations by writing less often (in `3207a35`)
* Avoid `get_meta_to_value` read in `auto_increment_iid` (in `79fa14f`)
* And various other performance improvements

Dependency updates:

* Bump `rocksdb` from `0.24` to `0.25` (in `80c3827`)

### New Features

* Add `allocator-jemalloc` feature flag to enable `rocksdb/jemalloc`
* Add experimental `NEW` flag to `PUSH` (in `f94cba0`)
* `impl Debug for sonic::Config` (in `6464cbf`)

### Bug Fixes

* Fix `max_flushes` (in `36de4c5`)

## [0.2.1] (2026-08-16)

[0.2.1]: https://github.com/valeriansaliou/sonic/compare/core-v0.2.0...core-v0.2.1

### Bug Fixes

* Fix configuration deserialization (in `2f678ae`)
  * The bug shouldn’t affect any `sonic-core` user, but this release is necessary for `sonic-server`.

## [0.2.0] (2026-08-16)

[0.2.0]: https://github.com/valeriansaliou/sonic/compare/core-v0.1.3...core-v0.2.0

### Changes

Improvements to search results:

* Implement BM25 lite (idf only) (in `0ac3e7b`)
* Minimum term idf, to avoid low-quality results at the end of the results list (in `58cd0e4`)

### New Features

* Add support for Unicode normalization (in `678d5e1`)
* Add support for custom stopwords (in `764536e`)
* Add more RocksDB configuration keys (in `8eb3045`)

### Bug Fixes

* Make `QUERY` insensitive to Unicode normal form (in `0fd1050`)
* Make stopwords insensitive to Unicode Normal Form (in `3f9782b`)
* Remove numbers from stopwords (in `4e11f75`)

## [0.1.3] (2026-07-09)

[0.1.3]: https://github.com/valeriansaliou/sonic/compare/core-v0.1.2...core-v0.1.3

### Changes

Dependency updates:

* Bump `jieba-rs` from `0.9` to `0.10` (in `5d4a432`)
* Disable unnecessary `rocksdb` features (in `421d13d`)
* Disable default features for all dependencies (in `cd7f328`)

### New Features

* feat: Improve the tokenizer to avoid splitting special tokens (in `f290006`)
* feat: Make tokenizer pattern matching opt-in (in `ddd6848`)
* feat: Make tokenizer pattern matching non-breaking (in `e744fd0`)

## [0.1.2] (2026-07-07)

[0.1.2]: https://github.com/valeriansaliou/sonic/compare/core-v0.1.1...core-v0.1.2

### Changes

* feat(core): Consider more strings to be IDs (in `58bfe3c`)

## [0.1.1] (2026-07-06)

[0.1.1]: https://github.com/valeriansaliou/sonic/compare/core-v0.1.0...core-v0.1.1

### Bug Fixes

* fix(core): Perform implicit `AND` when `QUERY`ing an ID (in `fe3d2cc`)

## [0.1.0] (2026-06-28)

[0.1.0]: https://github.com/valeriansaliou/sonic/compare/core-v0.0.1...core-v0.1.0

This release was focused on making improvements to search results
(see [Milestone #20 “v1.7.x - Better search results (non-breaking)”][milestone-20]).
We benchmarked the changes, and concluded the performance impact of all those
changes is negligible. If you notice something now being noticeably slower,
please tell us as it might be a bug!

[milestone-20]: https://github.com/valeriansaliou/sonic/milestone/20

### Changes

* Implement proper case folding (in `f67f964`)
* Rework `QUERY` results ranking algorithm (see [Pull Request #355 “No implicit `AND`”](https://github.com/valeriansaliou/sonic/pull/355))
* Logging improvements
* More tests

### New Features

* Allow disabling loose matching at the library level (in `17e196d`)
* Add support for diacritics-insensitive search (in `3d38caa`, `2379b0a`)
* Add support for stemming (in `db83731`)

### Bug Fixes

* Fix max typo correction in `QUERY` (in `097a752`)
* config: Fix non-string parsing from env (in `b17daad`)

## [0.0.1] (2026-06-03)

[0.0.1]: https://github.com/valeriansaliou/sonic/compare/v1.5.1...core-v0.0.1

### New Features

* Initial Sonic core release [[@RemiBardon](https://github.com/RemiBardon)].
