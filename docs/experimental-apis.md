---
author: Rémi Bardon <remi@remibardon.name>
created: 2026-10-02
updated: 2026-10-03
---

# Sonic’s experimental APIs

Sonic is made to remain backward-compatible as much as possible with existing
clients, thus its Sonic Channel protocol (see [`PROTOCOL.md`]) isn’t supposed
to change much over time.

[`PROTOCOL.md`]: ../PROTOCOL.md

However, after 7 years of using Sonic in production at [Crisp], we decided it
was time to introduce long-awaited features —like snippet retrieval— which
require extending the Sonic Channel protocol.

[Crisp]: https://crisp.chat "Crisp homepage"

To ensure those new features have a stable API, we decided to hide them behind
the Rust feature flag `experimental-api` until they are deemed ready.
This way, we can iterate on the design and release only after ensuring that the
API is ergonomic enough and that the implementation works at scale.

If you want to try out new features, you can build Sonic yourself with the
`experimental-api` feature enabled, but keep in mind that we might introduce
breaking changes at any time without prior notice —even in a patch release.
Semantic versioning applies to the stable release of Sonic, and does _not_
consider experimental API changes.

While marked experimental (or really anytime), we’re obviously open to feedback
about new features and their API. To share your thoughts, have a look at our
[“Experimental work” view on GitHub][exp-view]. There should generally be an
issue or a Pull Request for any experimental feature.

[exp-view]: https://github.com/valeriansaliou/sonic/issues/views/MDI0OlJlcG9zaXRvcnlTZWFyY2hTaG9ydGN1dDIzMTQz

<span id="install"></span>

## Trying out experimental Sonic features

Experimental features aren’t enabled in official Sonic builds. This way, you
cannot inadvertently use one of them wrong or have one break your Sonic index
without a warning. If you get an experimental Sonic build, you know what you
are doing.

Now that you have been warned, here is how to do it:

0. [Install Rust].
0. Get a copy of Sonic’s source code[^clone].
0. Check dependencies for known vulnerabilities.

   Every tagged Sonic version was checked for known vulnerabilities before
   being released, via [`cargo-deny`]. However, you might have a cloned a
   version which hasn’t been checked, and new vulnerabilities might have been
   discovered since our checks, so we advise you to double-check for yourself
   before installing Sonic.

   Of course, you don’t _have_ to, but a security-conscious person would.

   First, ensure you have `cargo-deny` installed:

   ```bash
   command -v cargo-deny || cargo install --locked cargo-deny
   ```

   then run:

   ```bash
   cargo deny check advisories bans sources --allow duplicate
   ```

   You should see “advisories ok, bans ok, sources ok”. If you get an error,
   read it then [contact us] if you can’t solve it by yourself (or find it
   worth mentioning).
0. Finally, install Sonic by running:

   ```bash
   cargo +stable install --locked --path server --bin sonic -F experimental-api
   ```

   Tip: You can add `--root <DIR>` to install Sonic in a specific location
   (`DIR` is where `bin/sonic` will be installed).

[Install Rust]: https://doc.rust-lang.org/book/ch01-01-installation.html "Installation - The Rust Programming Language"
[^clone]: Either by cloning `https://github.com/valeriansaliou/sonic.git` or by downloading the source code via <https://github.com/valeriansaliou/sonic/releases> or <https://github.com/valeriansaliou/sonic/archive/refs/heads/master.zip> if you don’t have `git` installed.

Note that 

[`cargo-deny`]: https://crates.io/crates/cargo-deny "“cargo-deny” on crates.io, the Rust Package Registry"

[contact us]: https://github.com/valeriansaliou/sonic/issues "valeriansaliou/sonic issues on GitHub"
