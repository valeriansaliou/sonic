Sonic Packaging
===============

This file contains quick reminders and notes on how to package Sonic.

We consider here the packaging flow of Sonic version `1.0.0`, for target architecture `x86_64` and distribution `debian9`:

1. **How to bump Sonic version before a release:**
    1. Bump version in `Cargo.toml` to `1.0.0`
    2. Execute `cargo update` to bump `Cargo.lock`

2. **How to build Sonic for Linux on Debian:**
    1. `apt-get install -y git build-essential clang`
    2. `curl https://sh.rustup.rs -sSf | sh` (install the `nightly` toolchain)
    3. `git clone https://github.com/valeriansaliou/sonic.git`
    4. `cd sonic/`
    5. `cargo build --release`

3. **How to package built binary and release it on GitHub:**
    1. `mkdir sonic`
    2. `mv target/release/sonic sonic/`
    3. `strip sonic/sonic`
    4. `cp -r config.cfg sonic/`
    5. `tar -czvf v1.0.0-x86_64-debian9.tar.gz sonic`
    6. `rm -r sonic/`
    7. Publish the archive on the [releases](https://github.com/valeriansaliou/sonic/releases) page on GitHub

4. **How to update Docker:**
    1. `docker build .`
    2. `docker tag [DOCKER_IMAGE_ID] valeriansaliou/sonic:v1.0.0` (insert the built image identifier)
    3. `docker push valeriansaliou/sonic:v1.0.0`

5. **How to update Crates:**
    1. Publish package on Crates: `cargo publish`
