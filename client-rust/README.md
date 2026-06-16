# `sonic-client`, the official Rust client for Sonic

This crate is a work in progress. API is subject to changes until it reaches
`1.0.0`. We won’t keep it in [ZeroVer](https://0ver.org/), don’t worry, but for
now some pieces still need some polishing.

## Features

- **Lock free**: No `Mutex`, `RwLock`…
- **Performant**: TCP streams are multiplexed using OS event queues
  (via [`mio`]) for optimal performance. Parsing is also minimal and aborts
  early to avoid needless computations.
- **Lighweight**: The library has only 7 dependencies, which are either
  essential or adding good performance benefits.
- **Almost inert when idle**: If no message should be sent and if no message is
  expected, `sonic-client` doesn’t execute a single line of code. No CPU time
  is wasted, it does the bare minimum.
- **Ergonomic**: All methods accept generic types so you don’t have to convert
  types or `clone` data needlessly.
- **Type-safe**: By construction, you can’t do unsound operations.
- **Command buffering**: Long commands are transparently split into smaller
  ones, depending on your Sonic server configuration. This way, you can `PUSH`
  megabytes of data without having to worry about Sonic buffer limits.
- **Production-ready**: `sonic-client` can be used at any scale. We benchmarked
  it by ingesting the English portion of Wikipedia, if that’s a good enough
  proof for you :)
- **Future-proof**: The library exposes a low-level API which you can use to
  send any command. If the Sonic Channel protocol gets updated but you can’t
  bump `sonic-client` for some reason, you’ll still have a way to do the new
  stuff.
- **Resilient**: One unexpected result or bad UTF-8 line does not prevent other
  events from being processed.
- **No async runtime assumption**: `sonic-client` can be used fully
  synchronously, but it also exposes an `async` API for use with any runtime.
- **Safe**: No `unsafe`, and `unwrap`s/`expect`s for performance in rare cases.
- **Flexible logging**: By default, `sonic-client` compiles with no log at all.
  However, you can enable `std`, `log` or `tracing` logs using feature flags.

[`mio`]: https://crates.io/crates/mio "mio on crates.io"

## Why another Rust client for Sonic?

[sonic-channel] by [@pleshevskiy] used to be the recommended Rust client, but
it has been archived on Mar 1, 2023 signifying it won’t get updated in the
future. While trying to use it in our benchmarks we noticed `PONG` is
unsupported (although `PING` is… causing a failure eery time it’s called) but
more importantly ingested text is not escaped. This means any text containing
a `"` breaks request parsing and causes ingestion to fail.

[sonic-channel]: https://github.com/pleshevskiy/sonic-channel
[@pleshevskiy]: https://github.com/pleshevskiy

[sonic_client] by [@FrontMage] was also recommended by Sonic’s README, but while
using it we noticed it logs passwords on `START` (!). In addition, the README
says the crate is under development but last commit was made on Apr 11, 2019 so
it’s safe to assume the crate is abandonned.

[sonic_client]: https://github.com/FrontMage/sonic_client
[@FrontMage]: https://github.com/FrontMage

To address those issues and as part of modernizations we decided to insource
the official Rust client. This will ensure its lifetime aligns with Sonic’s,
and tests will ensure it always works (and at scale).
