# Packaging

This file contains quick reminders and notes on how to package Sonic.

We consider here the packaging flow of Sonic for Linux.

## Requirements

It is highly recommended to [install Task](https://taskfile.dev/docs/installation) as it simplifies all maintenance operations thanks to hand-crafted helper tools.
If you really don’t want to install it, we try our best to always provide alternatives. Check [`Taskfile.dist.yaml`](./Taskfile.dist.yaml) to see what `task` calls internally.

## Releasing Sonic core (library)

1. Make sure [`core/CHANGELOG.md`](./core/CHANGELOG.md) is up to date (changes should be in the “Unreleased” section).
   - If not up-to-date, run `task changelog:prepare -- core`[^changelog-core] then follow the instructions.
1. Run `task release:core -- major|minor|path`[^release-core], following [Semantic Versioning](https://semver.org/).

[^changelog-core]: Alternative: `./scripts/changelog_prepare.sh core`
[^release-core]: Alternative: `./scripts/make_release.sh core major|minor|path`

CD pipelines will then publish the library on [crates.io](https://crates.io).

Check the [“Actions” tab on GitHub](https://github.com/valeriansaliou/sonic/actions/workflows/release-core.yml) to see the progress.

## Releasing Sonic server (binary)

1. If you made changes to the core, release it first.
   CD pipelines will take 6–10 minutes to run, you have to wait.

   If you don’t, `cargo publish -p sonic-server` will fail because it cannot find the version of `sonic-core` the server depends on. This will abort the entire CD pipeline and force you to manually fix your mess. You don’t want that.
1. Make sure [`server/CHANGELOG.md`](./server/CHANGELOG.md) is up to date (changes should be in the “Unreleased” section).
   - If not up-to-date, run `task changelog:prepare -- server`[^changelog-server] then follow the instructions.
1. Run `task release:server -- major|minor|path`[^release-server], following [Semantic Versioning](https://semver.org/).

[^changelog-server]: Alternative: `./scripts/changelog_prepare.sh server`
[^release-server]: Alternative: `./scripts/make_release.sh server major|minor|path`

CD pipelines will then build and release Sonic (server) on [crates.io](https://crates.io), GitHub, Docker Hub and Packagecloud.

Check the [“Actions” tab on GitHub](https://github.com/valeriansaliou/sonic/actions/workflows/release-server.yml) to see the progress.
