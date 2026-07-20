# Sonic Protocol

## ⚡️ Sonic Channel

**Sonic Channel is the protocol used to perform searches and ingest index data. You can also use it for Sonic administration operations. Sonic listens on TCP port 1491 by default.**

The `sonic` binary runs the server daemon. The separate `sonic-cli` binary is a
convenience client exposing `import`, `export`, `query`, `ping` and `consolidate`
subcommands over the protocol documented below.

The optional `sonic-router` binary exposes the same channel protocol and assigns
each `(collection, bucket)` to one Sonic backend. Bucket-scoped commands and
`UPSERTBATCH` are routed transparently. Collection-wide `COUNT`, `FLUSHC`,
`EXPORT`, `IMPORT`, `INFO` and `STATS` are rejected because one collection may
span multiple backends. `TRIGGER consolidate`, `TRIGGER backup <path>` and
`TRIGGER restore <path>` are broadcast to all online backends.

The router control plane listens on its configured admin address and accepts
newline-delimited commands after an optional `AUTH <password>` line:

* `PLACEMENTS <backend-id>`
* `MIGRATE START <collection> <bucket> <target>`
* `MIGRATE CATCHUP|CUTOVER|DRAIN|CLEANUP|FINISH|ROLLBACK <collection> <bucket>`
* `SNAPSHOT`

Server topology is configured through `[[servers]]` entries in `router.cfg`.
The router reloads topology changes every two seconds. New active servers
receive new buckets, draining servers retain existing buckets but receive no new
ones, and offline servers reject traffic. Removing a server from the file is
accepted only after all of its buckets and in-progress migrations have moved.

This document specifies the Sonic Channel protocol. Use it if you are looking to build your own Sonic Channel library, or if you are looking to debug Sonic using eg. `telnet` in command-line.

To start a `telnet` session with your local Sonic instance, execute: `telnet ::1 1491`

_Refer to sections below to interact with Sonic._

---

### 1️⃣ Before you start

**Please consider the following upon integrating the Sonic Channel protocol:**

1. Each command sent must be terminated with a new line character (`\n`) as to commit the command to the server;
2. Upon starting a Sonic Channel session, your library should read `buffer(20000)` for ordinary commands and `bulk_buffer(8388608)` for authenticated `UPSERTBATCH` commands;

---

### 2️⃣ Sonic Channel (uninitialized)

* `START <mode> <password>`: select mode to use for connection (either: `search`, `ingest` or `control`). The password is found in the `config.cfg` file at `channel.auth_password`.

_Issuing any other command — eg. `QUIT` — in this mode will abort the TCP connection, effectively resulting in a `QUIT` with the `ENDED not_recognized` response._

---

### 3️⃣ Sonic Channel (Search mode)

_The Sonic Channel Search mode is used for querying the search index. Once in this mode, you cannot switch to other modes or gain access to commands from other modes._

**➡️ Available commands:**

* `QUERY`: query database (syntax: `QUERY <collection> <bucket> "<terms>" [LIMIT(<count>)]? [OFFSET(<count>)]? [LANG(<locale>)]? [FROM(<unix_ms>)]? [TO(<unix_ms>)]?`)
* `QUERYDOCS`: query stored documents with the same options as `QUERY`; returns one Base64URL-encoded JSON document per event followed by `EVENT QUERYDOCS <id> DONE`
* `LIST`: enumerates all words in an index (syntax: `LIST <collection> <bucket> [LIMIT(<count>)]? [OFFSET(<count>)]?`; time complexity: `O(N)` where `N` is the number of words enumerated, within provided limits)
* `PING`: ping server (syntax: `PING`; time complexity: `O(1)`)
* `HELP`: show help (syntax: `HELP [<manual>]?`; time complexity: `O(1)`)
* `QUIT`: stop connection (syntax: `QUIT`; time complexity: `O(1)`)

**⏩ Syntax terminology:**

* `<collection>`: index collection (ie. what you search in, eg. `messages`, `products`, etc.);
* `<bucket>`: index bucket name (ie. user-specific search classifier in the collection if you have any eg. `user-1, user-2, ..`, otherwise use a common bucket name eg. `generic, default, common, ..`);
* `<terms>`: text for search terms (between quotes);
* `<count>`: a positive integer number; set within allowed maximum & minimum limits;
* `<locale>`: an ISO 639-3 locale code eg. `eng` for English (if set, the locale must be a valid ISO 639-3 code; if set to `none`, lexing will be disabled; if not set, the locale will be guessed from text);
* `<manual>`: help manual to be shown (available manuals: `commands`);

_Notice: the `bucket` terminology may confuse some Sonic users. As we are well-aware Sonic may be used in an environment where end-users may each hold their own search index in a given `collection`, we made it possible to manage per-end-user search indexes with `bucket`. If you only have a single index per `collection` (most Sonic users will), we advise you use a static generic name for your `bucket`, for instance: `default`._

**⬇️ Search flow example (via `telnet`):**

```bash
T1: telnet sonic.local 1491
T2: Trying ::1...
T3: Connected to sonic.local.
T4: Escape character is '^]'.
T5: CONNECTED <sonic-server v1.0.0>
T6: START search SecretPassword
T7: STARTED search protocol(1) buffer(20000)
T8: QUERY messages user:0dcde3a6 "valerian saliou" LIMIT(10)
T9: PENDING Bt2m2gYa
T10: EVENT QUERY Bt2m2gYa conversation:71f3d63b conversation:6501e83a
T11: QUERY helpdesk user:0dcde3a6 "gdpr" LIMIT(50)
T12: PENDING y57KaB2d
T13: QUERY helpdesk user:0dcde3a6 "law" LIMIT(50) OFFSET(200)
T14: PENDING CjPvE5t9
T15: PING
T16: PONG
T17: EVENT QUERY CjPvE5t9
T18: EVENT QUERY y57KaB2d article:28d79959
T19: QUIT
T20: ENDED quit
T21: Connection closed by foreign host.
```

_Notes on what happens:_

* **T6:** we enter `search` mode (this is required to enable `search` commands);
* **T8:** we query collection `messages`, in bucket for platform user `user:0dcde3a6` with search terms `valerian saliou` and a limit of `10` on returned results;
* **T9:** Sonic received the query and stacked it for processing with marker `Bt2m2gYa` (the marker is used to track the asynchronous response);
* **T10:** Sonic processed search query of T8 with marker `Bt2m2gYa` and sends 2 search results (those are conversation identifiers, that refer to a primary key in an external database);
* **T11 + T13:** we query collection `helpdesk` twice (in the example, this one is heavy, so processing of results takes more time);
* **T17 + T18:** we receive search results for search queries of T11 + T13 (this took a while!);

---

### 4️⃣ Sonic Channel (Ingest mode)

_The Sonic Channel Ingest mode is used for altering the search index (push, pop and flush). Once in this mode, you cannot switch to other modes or gain access to commands from other modes._

**➡️ Available commands:**

* `PUSH`: Append text to a stored document and update its index (syntax: `PUSH <collection> <bucket> <object> "<text>" [LANG(<locale>)]?`; time complexity: `O(N)`)
* `UPSERT`: Atomically replace a stored document and its index (syntax: `UPSERT <collection> <bucket> <object> "<text>" TS(<unix_ms>) [META(<base64url-json>)]? [LANG(<locale>)]?`)
* `UPSERTBATCH`: Apply a compressed multi-document batch (syntax: `UPSERTBATCH <collection> <fresh|upsert> <base64-zstd-ndjson>`); `fresh` is insert-only and `upsert` replaces existing OIDs
* `POP`: Remove the first exact text occurrence from a stored document and update its index (syntax: `POP <collection> <bucket> <object> "<text>"`; time complexity: `O(N)`)
* `EXPORT`: Stream a collection, or one optional bucket, to a server-local NDJSON Zstd file; every record contains its bucket (syntax: `EXPORT <collection> [<bucket>]? <path>`)
* `IMPORT`: Rebuild buckets from a server-local NDJSON Zstd file containing a `bucket` field on each record (syntax: `IMPORT <collection> <path>`)
* `COUNT`: Count indexed search data (syntax: `COUNT <collection> [<bucket> [<object>]?]?`; object counts retokenize the document in `O(N)`)
* `FLUSHC`: Flush all indexed data from a collection (syntax: `FLUSHC <collection>`; time complexity: `O(1)`)
* `FLUSHB`: Flush all indexed data from a bucket in a collection (syntax: `FLUSHB <collection> <bucket>`; time complexity: `O(N)` where `N` is the number of bucket objects)
* `FLUSHO`: Flush all indexed data from an object in a bucket in collection (syntax: `FLUSHO <collection> <bucket> <object>`; time complexity: `O(N)`)
* `PING`: ping server (syntax: `PING`; time complexity: `O(1)`)
* `HELP`: show help (syntax: `HELP [<manual>]?`; time complexity: `O(1)`)
* `QUIT`: stop connection (syntax: `QUIT`; time complexity: `O(1)`)

**⏩ Syntax terminology:**

* `<collection>`: index collection (ie. what you search in, eg. `messages`, `products`, etc.);
* `<bucket>`: index bucket name (ie. user-specific search classifier in the collection if you have any eg. `user-1, user-2, ..`, otherwise use a common bucket name eg. `generic, default, common, ..`);
* `<object>`: object identifier that refers to an entity in an external database, where the searched object is stored (eg. you use Sonic to index CRM contacts by name; full CRM contact data is stored in a MySQL database; in this case the object identifier in Sonic will be the MySQL primary key for the CRM contact);
* `<text>`: search text to be indexed (can be a single word, or a longer text; within maximum length safety limits; should be quoted using `"` quotes; internal quotes should be escaped using `\"`);
* `<locale>`: an ISO 639-3 locale code eg. `eng` for English (if set, the locale must be a valid ISO 639-3 code; if set to `none`, lexing will be disabled; if not set, the locale will be guessed from text);
* `<manual>`: help manual to be shown (available manuals: `commands`);

_Notice: the `bucket` terminology may confuse some Sonic users. As we are well-aware Sonic may be used in an environment where end-users may each hold their own search index in a given `collection`, we made it possible to manage per-end-user search indexes with `bucket`. If you only have a single index per `collection` (most Sonic users will), we advise you use a static generic name for your `bucket`, for instance: `default`._

**⬇️ Ingest flow example (via `telnet`):**

```bash
T1: telnet sonic.local 1491
T2: Trying ::1...
T3: Connected to sonic.local.
T4: Escape character is '^]'.
T5: CONNECTED <sonic-server v1.0.0>
T6: START ingest SecretPassword
T7: STARTED ingest protocol(1) buffer(20000)
T8: PUSH messages user:0dcde3a6 conversation:71f3d63b Hey Valerian
T9: ERR invalid_format(PUSH <collection> <bucket> <object> "<text>")
T10: PUSH messages user:0dcde3a6 conversation:71f3d63b "Hello Valerian Saliou, how are you today?"
T11: OK
T12: COUNT messages user:0dcde3a6
T13: RESULT 43
T14: COUNT messages user:0dcde3a6 conversation:71f3d63b
T15: RESULT 1
T16: FLUSHO messages user:0dcde3a6 conversation:71f3d63b
T17: RESULT 1
T18: FLUSHB messages user:0dcde3a6
T19: RESULT 42
T20: PING
T21: PONG
T22: QUIT
T23: ENDED quit
T24: Connection closed by foreign host.
```

_Notes on what happens:_

* **T6:** we enter `ingest` mode (this is required to enable `ingest` commands);
* **T8:** we try to push text `Hey Valerian` to the index, in collection `messages`, bucket `user:0dcde3a6` and object `conversation:71f3d63b` (the syntax that was used is invalid);
* **T9:** Sonic refuses the command we issued in T8, and provides us with the correct command format (notice that `<text>` should be quoted);
* **T10:** we attempt to push another text in the same collection, bucket and object as in T8;
* **T11:** this time, our push command in T10 was valid (Sonic acknowledges the push commit to the search index);
* **T12:** we count the number of indexed terms in collection `messages` and bucket `user:0dcde3a6`;
* **T13:** there are 43 terms (ie. words) in index for query in T12;
* **T18:** we flush all index data from collection `messages` and bucket `user:0dcde3a6`;
* **T19:** 42 terms have been flushed from index for command in T18;

---

### 5️⃣ Sonic Channel (Control mode)

_The Sonic Channel Control mode is used for administration purposes. Once in this mode, you cannot switch to other modes or gain access to commands from other modes._

**➡️ Available commands:**

* `TRIGGER`: trigger an action (syntax: `TRIGGER [<action>]? [<data>]?`; time complexity: `O(1)`)
* `INFO`: get server information (syntax: `INFO`; time complexity: `O(1)`)
* `STATS`: get physical collection storage statistics, optionally scanning logical index families and documents (syntax: `STATS <collection> [DEEP]?`)
* `PING`: ping server (syntax: `PING`; time complexity: `O(1)`)
* `HELP`: show help (syntax: `HELP [<manual>]?`; time complexity: `O(1)`)
* `QUIT`: stop connection (syntax: `QUIT`; time complexity: `O(1)`)

**⏩ Syntax terminology:**

* `<action>`: action to be triggered (available actions: `consolidate`, `backup`, `restore`);
* `<data>`: additional data to provide to the action (required for: `backup`, `restore`);
* `<manual>`: help manual to be shown (available manuals: `commands`);

**⬇️ Control flow example (via `telnet`):**

```bash
T1: telnet sonic.local 1491
T2: Trying ::1...
T3: Connected to sonic.local.
T4: Escape character is '^]'.
T5: CONNECTED <sonic-server v1.0.0>
T6: START control SecretPassword
T7: STARTED control protocol(1) buffer(20000)
T8: TRIGGER consolidate
T9: OK
T10: PING
T11: PONG
T12: QUIT
T13: ENDED quit
T14: Connection closed by foreign host.
```

_Notes on what happens:_

* **T6:** we enter `control` mode (this is required to enable `control` commands);
* **T8:** we trigger a database consolidation (instead of waiting for the next automated consolidation tick);
