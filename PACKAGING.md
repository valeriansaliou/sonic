# Packaging

This file contains quick reminders and notes on how to package Sonic.

We consider here the packaging flow of Sonic for Linux.

## Releasing Sonic core (library)

1. Make sure [`core/CHANGELOG.md`](./core/CHANGELOG.md) is up to date (changes should be in the “Unreleased” section).
1. [Install Task](https://taskfile.dev/docs/installation) then `task release:core -- major|minor|path`, following [Semantic Versioning](https://semver.org/).

   If you don’t want to install Task, you can run `./scripts/make_release.sh core major|minor|path` directly.

CD will then publish the library on [crates.io](https://crates.io).

Check the [“Actions” tab on GitHub](https://github.com/valeriansaliou/sonic/actions) to see the progress.

## Releasing Sonic server (binary)

1. If you made changes to the core, release it first.
1. Make sure [`server/CHANGELOG.md`](./server/CHANGELOG.md) is up to date (changes should be in the “Unreleased” section).
1. [Install Task](https://taskfile.dev/docs/installation) then `task release:server -- major|minor|path`, following [Semantic Versioning](https://semver.org/).

   If you don’t want to install Task, you can run `./scripts/make_release.sh server major|minor|path` directly.

CD will then build and release Sonic (server) on [crates.io](https://crates.io), GitHub, Docker Hub and Packagecloud.

Check the [“Actions” tab on GitHub](https://github.com/valeriansaliou/sonic/actions) to see the progress.
