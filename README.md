Sonic
=====

[![Build Status](https://travis-ci.org/valeriansaliou/sonic.svg?branch=master)](https://travis-ci.org/valeriansaliou/sonic) [![Dependency Status](https://deps.rs/repo/github/valeriansaliou/sonic/status.svg)](https://deps.rs/repo/github/valeriansaliou/sonic) [![Buy Me A Coffee](https://img.shields.io/badge/buy%20me%20a%20coffee-donate-yellow.svg)](https://www.buymeacoffee.com/valeriansaliou)

**Sonic is a fast, lightweight and schema-less search backend. It ingests search texts and identifier tuples, that can then be queried against.**

Sonic can be used as a simple alternative to super-heavy and full-featured search backends such as Elasticsearch in some use-cases. It is capable of normalizing natural language search queries, auto-completing a search query and providing most relevant results for a query.

**🇫🇷 Crafted in Nantes, France.**

TODO: link to journal (below)

**:newspaper: The Sonic project was initially announced in [a post on my personal journal](https://journal.valeriansaliou.name/TODO/).**

TODO: sonic hedgehog image (from personal drawing) (below)

![Sonic](https://valeriansaliou.github.io/sonic/images/sonic.png)

## Who uses it?

<table>
<tr>
<td align="center"><a href="https://crisp.chat/"><img src="https://valeriansaliou.github.io/sonic/images/crisp.png" height="64" /></a></td>
</tr>
<tr>
<td align="center">Crisp</td>
</tr>
</table>

_👋 You use Sonic and you want to be listed there? [Contact me](https://valeriansaliou.name/)._

## Features

* **Search terms are stored in collections, organized in buckets**; you may use a single bucket, or a bucket per user on your platform if you need to search in separate graphs.
* **Search results return object identifiers**, that can be resolved from an external database if you need to enrich the search results. This makes Sonic a simple graph storage system, that points to identifier results. Sonic doesn't store any textual data in its graphs.
* **Networked channel interface (Sonic Channel)**, that let you search your stored graphs or manage data ingestion (push in the index, pop from the index, flush a collection, flush a bucket, etc.). The Sonic Channel protocol was designed to be lightweight on resources and simple to integrate with (the protocol is specified in the sections below).
* **Easy-to-use libraries**, that let you connect to Sonic Channel from your apps.

## How to use it?

### Installation

Sonic is built in Rust. To install it, either download a version from the [Sonic releases](https://github.com/valeriansaliou/sonic/releases) page, use `cargo install` or pull the source code from `master`.

**Install from source:**

If you pulled the source code from Git, you can build it using `cargo`:

```bash
cargo build --release
```

You can find the built binaries in the `./target/release` directory.

**Install from Cargo:**

You can install Sonic directly with `cargo install`:

```bash
cargo install sonic-server
```

Ensure that your `$PATH` is properly configured to source the Crates binaries, and then run Sonic using the `sonic` command.

**Install from packages:**

Debian & Ubuntu packages are also available. Refer to the _[How to install it on Debian & Ubuntu?](#how-to-install-it-on-debian--ubuntu)_ section.

**Install from Docker Hub:**

You might find it convenient to run Sonic via Docker. You can find the pre-built Sonic image on Docker Hub as [valeriansaliou/sonic](https://hub.docker.com/r/valeriansaliou/sonic/).

First, pull the `valeriansaliou/sonic` image:

```bash
docker pull valeriansaliou/sonic:v1.0.0
```

Then, seed it a configuration file and run it (replace `/path/to/your/sonic/config.cfg` with the path to your configuration file):

```bash
docker run -p 1491:1491 -v /path/to/your/sonic/config.cfg:/etc/sonic.cfg valeriansaliou/sonic:v1.0.0
```

In the configuration file, ensure that:

* `channel.inet` is set to `0.0.0.0:1491` (this lets Sonic Channel be reached from outside the container)

Sonic Channel will be reachable from `tcp://localhost:1491`.

### Configuration

Use the sample [config.cfg](https://github.com/valeriansaliou/sonic/blob/master/config.cfg) configuration file and adjust it to your own environment.

**Available configuration options are commented below, with allowed values:**

**[server]**

* `log_level` (type: _string_, allowed: `debug`, `info`, `warn`, `error`, default: `error`) — Verbosity of logging, set it to `error` in production

**[channel]**

* `inet` (type: _string_, allowed: IPv4 / IPv6 + port, default: `[::1]:1491`) — Host and TCP port Sonic Channel should listen on
* `tcp_timeout` (type: _integer_, allowed: seconds, default: `300`) — Timeout of idle/dead client connections to Sonic Channel

**[[channel.search]]**

* `query_limit_default` (type: _integer_, allowed: numbers, default: `10`) — Default search results limit for a query command (if the LIMIT command modifier is not used when issuing a QUERY command)
* `query_limit_maximum` (type: _integer_, allowed: numbers, default: `100`) — Maximum search results limit for a query command (if the LIMIT command modifier is being used when issuing a QUERY command)

### Run Sonic

Sonic can be run as such:

`./sonic -c /path/to/config.cfg`

## Perform searches and manage objects

Both searches and object management (ie. data ingestion) is handled via the Sonic Channel protocol only. As we want to keep things simple with Sonic (similarly to how Redis does), connecting to Sonic Channel is the way to go when you need to interact with the Sonic search database.

Sonic Channel can be accessed via the `telnet` utility from your computer. The very same system is also used by all Sonic Channel libraries (eg. NodeJS).

## Sonic Channel (uninitialized)

* `START <mode>`: select mode to use for connection (either: `search` or `ingest`)

_Issuing any other command — eg. `QUIT` — in this mode will abort the TCP connection, effectively resulting in a `QUIT` with the `ENDED not_recognized` response._

## Sonic Channel (Search mode)

_The Sonic Channel Search mode is used for querying the search index. Once in this mode, you cannot switch to other modes or gain access to commands from other modes._

**➡️ Available commands:**

* `QUERY`: query database (syntax: `QUERY <collection> <bucket> <terms> [LIMIT(<count>)]?`)
* `PING`: ping server (syntax: `PING`)
* `QUIT`: stop connection (syntax: `QUIT`)

**⏩ Syntax terminology:**

* `<collection>`: index collection (ie. what you search in, eg. `messages`, `products`, etc.)
* `<bucket>`: index bucket name (ie. user-specific search classifier in the collection if you have any eg. `user-1, user-2, ..`, otherwise use a common bucket name eg. `generic, default, common, ..`)
* `<terms>`: text for search terms
* `<count>`: a positive integer number; set within allowed maximum & minimum limits

_Notice: the `bucket` terminology may confuse some Sonic users. As we are well-aware Sonic may be used in an environment where end-users may each hold their own search index graph in a given `collection`, we made it possible to manage per-end-user search graphs with `bucket`. If you only have a single index graph per `collection` (most Sonic users will), we advise you use a static generic name for your `bucket`, for instance: `default`._

**⬇️ Search flow example (via `telnet`):**

TODO

## Sonic Channel (Ingest mode)

_The Sonic Channel Ingest mode is used for altering the search index (push, pop and flush). Once in this mode, you cannot switch to other modes or gain access to commands from other modes._

**➡️ Available commands:**

* `PUSH`: Push search data in the index (syntax: `PUSH <collection> <bucket> <object> <text>`)
* `POP`: Pop search data from the index (syntax: `POP <collection> <bucket> <object>`)
* `COUNT`: Count indexed search data (syntax: `COUNT <collection> [<bucket> [<object>]?]?`)
* `FLUSHC`: Flush all indexed data from a collection (syntax: `FLUSHC <collection>`)
* `FLUSHB`: Flush all indexed data from a bucket in a collection (syntax: `FLUSHB <collection> <bucket>`)
* `FLUSHO`: Flush all indexed data from an object in a bucket in collection (syntax: `FLUSHO <collection> <bucket> <object>`)
* `PING`: ping server (syntax: `PING`)
* `QUIT`: stop connection (syntax: `QUIT`)

**⏩ Syntax terminology:**

* `<collection>`: index collection (ie. what you search in, eg. `messages`, `products`, etc.)
* `<bucket>`: index bucket name (ie. user-specific search classifier in the collection if you have any eg. `user-1, user-2, ..`, otherwise use a common bucket name eg. `generic, default, common, ..`)
* `<object>`: object identifier that refers to an entity in an external database, where the searched object is stored (eg. you use Sonic to index CRM contacts by name; full CRM contact data is stored in a MySQL database; in this case the object identifier in Sonic will be the MySQL primary key for the CRM contact)
* `<text>`: search text to be indexed (can be a single word, or a longer text; within maximum length safety limits)

_Notice: the `bucket` terminology may confuse some Sonic users. As we are well-aware Sonic may be used in an environment where end-users may each hold their own search index graph in a given `collection`, we made it possible to manage per-end-user search graphs with `bucket`. If you only have a single index graph per `collection` (most Sonic users will), we advise you use a static generic name for your `bucket`, for instance: `default`._

**⬇️ Ingest flow example (via `telnet`):**

TODO

**📦 Sonic Channel Libraries:**

* **NodeJS**: **[node-sonic-channel](https://www.npmjs.com/package/sonic-channel)**

👉 Cannot find the library for your programming language? Build your own and be referenced here! ([contact me](https://valeriansaliou.name/))

## How fast & lightweight is it?

Sonic was built for [Crisp](https://crisp.chat/) from the start. As Crisp was growing and indexing more and more search data into a full-text search SQL database, we decided it was time to switch to a proper search backend system. When reviewing Elasticsearch (ELS) and others, we found those were full-featured heavyweight systems that did not scale well with Crisp's freemium-based cost structure.

At the end, we decided to build our own search backend, designed to be simple and lightweight on resources. We did some benchmarks on how Sonic behaves at scale.

TODO: benchmarks

## :fire: Report A Vulnerability

If you find a vulnerability in Sonic, you are more than welcome to report it directly to [@valeriansaliou](https://github.com/valeriansaliou) by sending an encrypted email to [valerian@valeriansaliou.name](mailto:valerian@valeriansaliou.name). Do not report vulnerabilities in public GitHub issues, as they may be exploited by malicious people to target production servers running an unpatched Sonic instance.

**:warning: You must encrypt your email using [@valeriansaliou](https://github.com/valeriansaliou) GPG public key: [:key:valeriansaliou.gpg.pub.asc](https://valeriansaliou.name/files/keys/valeriansaliou.gpg.pub.asc).**

**:gift: Based on the severity of the vulnerability, I may offer a $200 (US) bounty to whomever reported it.**
