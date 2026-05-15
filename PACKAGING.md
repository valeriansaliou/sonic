Packaging
=========

This file contains quick reminders and notes on how to package Sonic.

We consider here the packaging flow of Sonic for Linux.

1. **How to bump Sonic version, build Sonic, package it and release it on Crates, GitHub, Docker Hub and Packagecloud (multiple architectures):**
   1. Make sure [`CHANGELOG.md`](./CHANGELOG.md) is up to date (changes should be in the “Unreleased” section).
   1. [Install Task](https://taskfile.dev/docs/installation) then `task release -- major|minor|path`, following [Semantic Versioning](https://semver.org/).

      If you don’t want to install Task, you can run `./scripts/make_release.sh major|minor|path` directly.
   1. Wait for all release jobs to complete on the [actions](https://github.com/valeriansaliou/sonic/actions) page on GitHub.
   1. Publish a changelog and upload all the built archives on the [releases](https://github.com/valeriansaliou/sonic/releases) page on GitHub.
