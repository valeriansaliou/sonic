Sonic Contributing Guidelines
=============================

# How to get started with contributing

- First of all, fork and clone this repo
- Install Rust and Cargo (to build and test Sonic)
- Install NPM (for integration tests)

## Build Sonic

From the repository root, run:

```sh
cargo build
```

## Start Sonic

From the repository root, run:

```sh
cargo run
```

## Run unit tests

From the repository root, run:

```sh
cargo test
```

## Run integration tests

From the directory: `<repository root>/tests/integration/scripts/`, run:

```sh
./run.sh
```
