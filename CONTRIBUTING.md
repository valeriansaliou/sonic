Sonic Contributing Guide
========================

# Get Started

- First of all, fork and clone this repo;
- Install Rust and Cargo (to build and test Sonic);
- Install NPM (for integration tests);

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

# Report Issues & Request Features

**If you encounter an issue with Sonic, or would like to request a feature to be implemented, please do [open an issue](https://github.com/valeriansaliou/sonic/issues/new).**

Note that before opening an issue, you should always search for other similar issues as to avoid opening a duplicate issue. This makes the life of the project maintainer much easier.

When writing your issue title and command, make sure to be as precise as possible, giving away the maximum amount of details (even if you have a feeling some details are useless, they might make debugging or understanding easier for us).

# Submit Your Code

**If you would like to contribute directly by writing code, you should fork this repository and edit it right away from your GitHub namespace.**

Once you are done with your work, always ensure to format your Rust code according to guidelines, via the [rustfmt](https://github.com/rust-lang/rustfmt) utility: `rustfmt src/*.rs`

When this is done, you may open a Pull Request (PR), then explain your changes and their purpose precisely. We will finally accept or comment on your Pull Request, if we need more changes done on your code.
