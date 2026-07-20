# Sonic Inner Workings

This document was written with the goal of explaining the inner workings of Sonic, as well as the whys of the design choices that were made while building Sonic.

Anyone reading this documentation should quickly get more familiar in how such a search index can be built from scratch, to the point that they should be able to start building their own Sonic from scratch.

_If you feel something is missing from this document, or if it did not help you understand a concept Sonic implements, please [open an issue](https://github.com/valeriansaliou/sonic/issues/new) and explain precisely which part you did not get and why you think you did not get it._

## The Building Blocks of a Search Index

### Basics of a search index

A search index is nothing more than a specialized database. It should expose primitives such as: query the index, push text in the index, pop text from the index, flush parts of the index.

The search index server is responsible for organizing the index data in a way that makes writes and reads efficient. It makes uses of specialized data structures for some very specific operations like typos corrections. The overall goal of such a search index system is: speed, lightweightness and data compactness (ie. it should minimize the resulting output database size given a text input size).

As to provide flexibility to organized indexed data, the search index is organized into collections that contain buckets. Buckets contain indexed objects. This means that you can organize your search index within a depth of 2 layers. Objects are actual search results; you could push an object `result_1` to collection `messages` within bucket `user_1`. This would index `messages` for `user_1` with result `result_1`. Later on, one could search for `messages` matching a given query for `user_1`. If the Sonic user use case does not require using buckets, the bucket value can still be set to a generic value, eg. `default`.

Sonic can operate as a compact identifier-only index through PUSH and QUERY, or store complete documents through UPSERT and QUERYDOCS. Stored documents contain the searchable text, an explicit UTC timestamp and extensible JSON metadata.

It is worth nothing that any project initiated as of 2019 should make use of modern server hardware, which is mostly all about multi-core CPUs and SSDs. Also, Sonic should be very wary of minimizing its resource requirements — _from a cold start to running under high load_ — as a lot of developers nowadays expect to run software on cheap VPS servers with limited CPU time, small disk space and little RAM. Those modern VPS are nonetheless powered by modern SSDs with fast random I/O. Last but not least, it would definitely be a plus if we could make software a bit greener.

In order to address the above, Sonic is capable to run queries over multiple CPUs in parallel. It leverages SSDs fast random I/O by using RocksDB as its main key-value store. It also avoids eating all available RAM by storing most data on-disk (via memory mapping), which is not an issue anymore as of 2019, as SSDs have low I/O latency and can sustain an unlimited number of reads over their lifetimes. Though, as writes are Achilles' heel of SSD disks, Sonic aims at minimizing writes and buffers a lot of those writes in RAM, which are committed to disk at periodic intervals. This should maximize the lifespan of the SSD disk under heavy index write load. Unfortunately, the side-effect of doing this is that in case of server power loss, non-committed writes will vanish.

### How do result objects get indexed?

Sonic stores result objects in a key-value database (abbreviated KV), powered by RocksDB.

When a text is pushed to Sonic, Charabia splits it into script-aware words and detects their language. Each normalized word is then associated to the pushed object result and committed to the KV database as `word <-> object`. Common words remain indexed so ingestion and queries cannot disagree because of language-dependent filtering.

When objects are pushed to the search index for a given bucket in a given collection, for instance object `session_77f2e05e-5a81-49f0-89e3-177e9e1d1f32`, Sonic converts this object to a compact 32 bits format, for instance `10292198`. We call the user-provided object identifier the OID, while the compact internal identifier is named the IID. The IID is mapped internally to indexed words, and is much more compact in terms of storage than the OID. You can think of OIDs and IIDs as basically the same thing, except that the IID is the compact version of an OID. OIDs are only used for user-facing input and output objects, while IIDs are only used for internal storage of those objects. On very long indexed texts, this helps save **_a lot_** of disk space.

The KV schema uses collision-free dictionaries while keeping frequently repeated identifiers compact:

1. **Bucket-Name-To-ID / Bucket-ID-To-Name** assign a sequential 32-bit ID to each complete bucket name.
2. **Term-Postings** maps a length-prefixed normalized term and IID-range shard to the compact object IDs contained in that range.
3. **OID-To-IID / IID-To-OID** map the complete user-provided object ID to a sequential 32-bit internal ID and back.
4. **IID-To-Terms** lists sorted normalized UTF-8 terms associated with an IID using length-prefixed encoding.
5. **Meta-To-Value** stores the bucket and object counters.
6. **IID-To-Timestamp** stores the exact document timestamp used for boundary checks and ranking.
7. **Time-Postings** map hourly UTC slices and IID shards to compact object offsets for date intersections.

Each collection RocksDB has separate metadata, postings and document column families. Active levels use LZ4 compression and the bottommost level uses Zstandard. Keys are bucket-first and numeric components use ordered big-endian encoding. Posting keys include a length-prefixed normalized term and a 16-bit shard selected from the high IID bits, so each value covers at most 65,536 objects. Sparse shards choose the smaller of raw u16 and delta-VarInt encoding, while dense shards use an 8 KiB bitmap.

Term frequencies are derived from posting cardinalities rather than stored in a separate count index. Multi-shard frequencies sum the cardinality of every shard sharing the same bucket and normalized-term prefix.

Bucket names, OIDs and terms are never identified solely by a hash. Exact postings are exhaustive, while query candidate scoring remains independently bounded. The schema is versioned and intentionally incompatible with earlier indexes; schema changes require a full re-ingestion.

Identifiers are scoped rather than global: bucket IDs are local to a collection and IIDs are local to a bucket. Normalized terms are encoded directly in posting and count keys, avoiding a separate term-ID namespace. Each numeric namespace can hold about 4.29 billion values. Allocation fails explicitly on overflow; deployments approaching that limit must split the affected collection or bucket.

### How does adaptive typo correction work?

When users type search queries, they make mistakes. Sonic uses a bounded typo lexicon to map misspelled query terms to close terms that occur frequently in the indexed corpus.

For instance, if the corpus frequently contains `messenger` and the user searches for `mesengr`, Sonic can include the postings for `messenger` with a lower score than exact matches.

The FST ([Finite-State Transducer](https://en.wikipedia.org/wiki/Finite-state_transducer)) stores terms with a score combining document frequency and recency. Terms below the configured admission threshold are ignored. During consolidation, the hottest terms are retained within the configured word and size budgets, while colder terms are evicted.

Sonic stores one memory-mapped typo lexicon per bucket. RocksDB remains exhaustive and authoritative; the FST is only a best-effort correction index and never affects exact term availability.

Because the FST is immutable, frequency changes are buffered and periodically consolidated into a new file. Consolidation selects terms by frequency before rebuilding the lexicographically ordered map.

### How do texts get cleaned up? (via the lexer)

Any text pushed to Sonic is segmented by Charabia, then normalized by the lexer before it is added to the index. Charabia detects scripts and languages per token, while an explicit `LANG` hint restricts that detection. Structured values such as email addresses, URLs, phone numbers and identifiers remain atomic and bypass fuzzy matching.

Sonic's tokenizer is built around an iterator pattern and yields lexed words one-by-one. Charabia's detected language is carried with each word and selects the optional stemming algorithm. Stopwords are not removed: this keeps all terms searchable and avoids inconsistent ingestion and query behavior on short or multilingual text.

### What is the purpose of the tasker system?

Looking at the source code of Sonic, you will find a module named `tasker` ([see here](https://github.com/valeriansaliou/sonic/blob/1d4c49ed348abbb8411afe7ecee6014703784d48/server/src/tasker)). This module performs background tasks, and is triggered periodically.

**The tasker performs the following actions:**

1. **Janitor**: it closes cached collection and bucket stores that were not used recently, freeing up memory (_code: [Tasker::tick](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/tasker/runtime.rs#L48)_);
2. **Consolidate**: it writes in-memory FST changes to the on-disk FST data structure (_code: [Tasker::tick](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/tasker/runtime.rs#L48)_);

As in all databases, a lot of locking is involved while the tasker is performing heavy-duty work on a KV or FST store. Thus, when the tasker system kicks-in, stores may experience higher than expected latency for all consumers attempting to read or write to them. The tasker system has been optimized to minimize thread contention caused by locks, so the impact of those locks on Sonic consumers should be minimum.

## On the Sonic Channel Protocol

In order for a client to communicate with the search index system, one needs a protocol. Sonic uses the Sonic Channel protocol, which defines a way for clients to send commands (ie. requests) to a Sonic server over the network (via a raw TCP socket); and get responses from the Sonic server. For instance, a client may send a search query command such as `QUERY collection bucket "search query"` and get a response with search results such as `EVENT QUERY isgsHQYu result_1 result_2`.

**On that Sonic Channel protocol, technical choices that may seem to go against common sense were made:**

1. **Sonic does not expose any HTTP API interface**, as it adds a network and processing overhead cost we do not want to bear;
2. **Sonic only exposes a raw TCP socket** with which clients interact via the Sonic Channel protocol, which was designed to be simple, lightweight and extensible;
3. **Most Sonic Channel commands are synchronous**, for simplicity's sake (Redis does the same). You can still run multiple Sonic Channel connections in parallel, and enjoy increased parallelism, but on a given Sonic Channel connection, you must wait for the previous command to return before issuing the next one;
4. **Some Sonic Channel commands are asynchronous**, when a lot of commands may be issued in a short period of time, in a burst pattern. This is typical of read operations such as search queries, which should be submitted as jobs to a dedicated thread pool, which can be upsized and downsized at will. To handle this, a special eventing protocol format should be used;

_The Sonic Channel protocol is specified in a separate document, which [you can read here](https://github.com/valeriansaliou/sonic/blob/master/PROTOCOL.md)._

## The Journey of a Search Query

As always, examples are the way to go to explain any complex system. This section drafts the journey of a search query in Sonic, from receiving the search query command over Sonic Channel, to serving results to the Sonic Channel consumer.

Given a collection `messages` and a bucket `acme_corp` (ie. indexed messages for Acme Corp), John Doe wants to find messages that match the query text `"The robber has stolen our corporate car"`.

First off, John Doe would connect to Sonic over a Sonic Channel client, for instance [node-sonic-channel](https://github.com/valeriansaliou/node-sonic-channel). Using this client, he would issue the following query: `QUERY messages acme_corp "The robber has stolen our corporate car"` to find conversations that contain messages about a recent robbery at Acme Corp.

**After receiving the raw command above, the Sonic server would, in order:**

1. Read the raw command from the Sonic Channel TCP stream buffer (_code: [Self::on_message](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/channel/handle.rs#L163)_);
2. Route the unpacked command message to the proper command handler, which would be `ChannelCommandSearch::dispatch_query` (_code: [ChannelMessage::on](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/channel/message.rs#L39)_);
3. Commit the search query for processing (_code: [ChannelCommandBase::commit_pending_operation](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/channel/command.rs#L428)_);
4. Dispatch the search query to its executor (_code: [StoreOperationDispatch::dispatch](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/channel/command.rs#L351)_);
5. Run the search executor (_code: [ExecutorSearch::search](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/executor/search.rs#L21)_);
6. Open both the KV and FST stores for the collection `messages` and bucket `acme_corp` (_code: [StoreKVPool::acquire + StoreFSTPool::acquire](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/executor/search.rs#L34)_);
7. Perform search query text lexing, and search word-by-word, which would yield in order: `robber`, `stolen`, `corporate`, `car` (_code: [lexer.next](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/executor/search.rs#L50)_);
8. Expand eligible query terms through the adaptive typo lexicon and merge matching postings with a Levenshtein penalty;
9. Perform paging on found OIDs from KV store to limit results (_code: [found_iids.iter](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/executor/search.rs#L163)_);
10. Return found OIDs from the executor (_code: [result_oids](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/executor/search.rs#L180)_);
11. Write back the final results to the TCP stream (_code: [response_args_groups](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/channel/message.rs#L81)_);

_This is it!_ John Doe would receive the following response from Sonic Channel: `EVENT QUERY isgsHQYu conversation_3459 conversation_29398`, which indicates that there are 2 conversations that contain messages matching the search text `"The robber has stolen our corporate car"`.
