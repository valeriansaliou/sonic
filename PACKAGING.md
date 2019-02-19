Packaging
=========

This file contains quick reminders and notes on how to package Sonic.

We consider here the packaging flow of Sonic version `1.0` for Debian, for target architecture `x86_64` (the steps are alike for `i686`):

1. **How to setup Rustup Linux toolchain on MacOS:**
    1. `brew install filosottile/musl-cross/musl-cross` (see: [FiloSottile/homebrew-musl-cross](https://github.com/FiloSottile/homebrew-musl-cross))
    2. `rustup target add x86_64-unknown-linux-musl`

2. **How to bump Sonic version before a release:**
    1. Bump version in `Cargo.toml` to `1.0.0`
    2. Execute `cargo update` to bump `Cargo.lock`
    3. Bump Debian package version in `debian/rules` to `1.0`

3. **How to build Sonic for Linux on macOS:**
    1. `cargo build --target=x86_64-unknown-linux-musl --release`

4. **How to package built binary and release it on GitHub:**
    1. `mkdir sonic`
    2. `mv target/x86_64-unknown-linux-musl/release/sonic sonic/`
    3. `cp config.cfg sonic/`
    4. `tar -czvf v1.0-amd64.tar.gz sonic`
    5. `rm -r sonic/`

5. **How to trigger a Debian build from Travis CI:**
    1. Commit your changes locally
    2. `git describe --always --long` eg. gives `8aca211` (copy this)
    3. `git tag -a 1.0` insert description eg. `1.0-0-8aca211` and save
    4. `git push origin 1.0:1.0`
    5. Quickly upload the archive files as GitHub releases before the build triggers, named as eg. `v1.0-amd64.tar.gz`

6. **How to update Docker:**
    1. `docker build .`
    2. `docker tag [DOCKER_IMAGE_ID] valeriansaliou/sonic:v1.0.0` (insert the built image identifier)
    3. `docker push valeriansaliou/sonic:v1.0.0`

7. **How to update Crates:**
    1. Publish package on Crates: `cargo publish`

Notice: upon packaging `x86_64` becomes `amd64` and `i686` becomes `i386`.

Cargo configuration for custom Linux linkers (`~/.cargo/config`):

```toml
[target.x86_64-unknown-linux-musl]
linker = "x86_64-linux-musl-gcc"

[target.i686-unknown-linux-musl]
linker = "i486-linux-musl-gcc"
```
