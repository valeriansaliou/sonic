# Sonic core Changelog

<!-- markdownlint-disable no-duplicate-heading -->

## [Unreleased]

<!-- WARN: Do not move the next line and add changelog entries **under** it.
       It’s used by `task release:*` when updating the changelog. -->
[Unreleased]: https://github.com/valeriansaliou/sonic/compare/core-v0.1.2...HEAD

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
