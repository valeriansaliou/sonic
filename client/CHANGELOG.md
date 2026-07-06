# Sonic Rust client Changelog

<!-- markdownlint-disable no-duplicate-heading -->

## [Unreleased]

<!-- WARN: Do not move the next line and add changelog entries **under** it.
       It’s used by `task release:*` when updating the changelog. -->
[Unreleased]: https://github.com/valeriansaliou/sonic/compare/client-v0.3.0...HEAD

## [0.3.0] (2026-07-06)

[0.3.0]: https://github.com/valeriansaliou/sonic/compare/client-v0.2.0...client-v0.3.0

### Changes

* feat(client)!: Return the count on `SonicChannelIngest::pop` (in `7ab16d1`)
* feat(client)!: Return the count on `SonicChannelIngest::flush*` (in `3bb154b`)

### Bug Fixes

* fix(client): Fix `SonicChannelIngest::pop` (in `a2bd72e`)
* fix(client): Fix `SonicChannelIngest::flush*` (in `a34b430`)
* fix(core): Perform implicit `AND` when `QUERY`ing an ID (in `fe3d2cc`)

## [0.2.0] (2026-07-01)

[0.2.0]: https://github.com/valeriansaliou/sonic/compare/core-v0.1.0...client-v0.2.0

### New Features

* Initial release \[[@RemiBardon](https://github.com/RemiBardon)\].
