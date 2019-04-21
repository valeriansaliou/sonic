Sonic Inner Workings
====================

This document was written with the goal of explaining the inner workings of Sonic, as well as the whys of the design choices that were made while building Sonic.

Anyone reading this documentation should quickly get more familiar in how such a search index can be built from scratch, to the point that they should be able to start building their own Sonic from scratch.

_If you feel something is missing from this document, or if it did not help you understand a concept Sonic implements, please [open an issue](https://github.com/valeriansaliou/sonic/issues/new) and explain precisely which part you did not get and why you think you did not get it._

# The Building Blocks of a Search Index

## Basics of a search index

A search index is nothing more than a specialized database. It should expose primitives such as: query the index, push text in the index, pop text from the index, flush parts of the index.

The search index server is responsible for organizing the index data in a way that makes writes and reads efficient. It makes uses of specialized data structures for some very specific operations like typos corrections. The overall goal of such a search index system is: speed, lightweightness and data compactness (ie. it should minimize the resulting output database size given a text input size).

As to provide flexibility to organized indexed data, the search index is organized into collections that contain buckets. Buckets contain indexed objects. This means that you can organize your search index within a depth of 2 layers. Objects are actual search results; you could push an object `result_1` to collection `messages` within bucket `user_1`. This would index `messages` for `user_1` with result `result_1`. Later on, one could search for `messages` matching a given query for `user_1`. If the Sonic user use case does not require using buckets, the bucket value can still be set to a generic value, eg. `default`.

Sonic, unlike many other search index systems, does not serve actual documents as search results. A strategic choice was made to store only identifiers refering to primary keys in an external database, which makes the data stored on-disk as compact as it can be. Users can still refer to their external database to fetch actual search result documents, using identifiers provided by Sonic.

It is worth nothing that any project initiated as of 2019 should make use of modern server hardware, which is mostly all about multi-core CPUs and SSDs. Also, Sonic should be very wary of minimizing its resource requirements — _from a cold start to running under high load_ — as a lot of developers nowadays expect to run software on cheap VPS servers with limited CPU time, small disk space and little RAM. Those modern VPS are nonetheless powered by modern SSDs with fast random I/O. Last but not least, it would definitely be a plus if we could make software a bit greener.

In order to address the above, Sonic is capable to run queries over multiple CPUs in parallel. It leverages SSDs fast random I/O by using RocksDB as its main key-value store. It also avoids eating all available RAM by storing most data on-disk (via memory mapping), which is not an issue anymore as of 2019, as SSDs have low I/O latency and can sustain an unlimited number of reads over their lifetimes. Though, as writes are Achilles' heel of SSD disks, Sonic aims at minimizing writes and buffers a lot of those writes in RAM, which are commited to disk at periodic intervals. This should maximize the lifespan of the SSD disk under heavy index write load. Unfortunately, the side-effect of doing this is that in case of server power loss, non-commited writes will vanish.

In order for a client to communicate with the search index system, one needs a protocol. Sonic uses the Sonic Channel protocol, which defines a way for clients to send commands (ie. requests) to a Sonic server over the network (via a raw TCP socket); and get responses from the Sonic server. For instance, a client may send a search query command such as `QUERY collection bucket "search query"` and get a response with search results such as `EVENT QUERY isgsHQYu result_1 result_2`.

**On that Sonic Channel protocol, technical choices that may seem to go against common sense were made:**

1. **Sonic does not expose any HTTP API interface**, as it adds a network and processing overhead cost we do not want to bear;
2. **Sonic only exposes a raw TCP socket** with which clients interact via the Sonic Channel protocol, which was designed to be simple, lightweight and extensible;
3. **Most Sonic Channel commands are synchronous**, for simplicity's sake (Redis does the same). You can still run multiple Sonic Channel connections in parallel, and enjoy increased parallelism, but on a given Sonic Channel connection, you must wait for the previous command to return before issuing the next one;
4. **Some Sonic Channel commands are asynchronous**, when a lot of commands may be issued in a short period of time, in a burst pattern. This is typical of read operations such as search queries, which should be submitted as jobs to a dedicated thread pool, which can be upsized and downsized at will. To handle this, a special eventing protocol format should be used;

_The Sonic Channel protocol is specified in a separate document, which [you can read here](https://github.com/valeriansaliou/sonic/blob/master/PROTOCOL.md)._

## How do result objects get indexed?

Sonic stores result objects in a key-value database (abbreviated KV), powered by RocksDB.

When a text is pushed to Sonic, this text gets normalized, cleaned up and splitted in separate words. Each word is then associated to the pushed object result, and commited to the KV database as `word <-> object`.

Upon cleaning the text, overhead is eluded. For instance, in the text `the lazy dog` there would be no point in indexing the word `the`, which is what is called a _stopword_. Sonic does not push stopwords to the index ([read more on stopwords](https://en.wikipedia.org/wiki/Stop_words)).

When objects are pushed to the search index for a given bucket in a given collection, for instance object `session_77f2e05e-5a81-49f0-89e3-177e9e1d1f32`, Sonic converts this object to a compact 32 bits format, for instance `10292198`. We call the user-provided object identifier the OID, while the compact internal identifier is named the IID. The IID is mapped internally to indexed words, and is much more compact in terms of storage than the OID. You can think of OIDs and IIDs as basically the same thing, except that the IID is the compact version of an OID. OIDs are only used for user-facing input and output objects, while IIDs are only used for internal storage of those objects. On very long indexed texts, this helps save **_a lot_** of disk space.

The KV store has a simple schema, where we associate a binary key to binary data. The following types of keys exist:

1. **Meta-To-Value**: state data for the bucket, eg. stores the count increment of indexed objects (data is in arbitrary format) (code: [StoreKeyerIdx::MetaToValue](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/store/keyer.rs#L24));
2. **Term-To-IIDs**: maps a word (ie. term) to an internal identifier (ie. IID), which is essentialy a word-to-result mapping (data is an array of 32 bits numbers encoded to binary as little-endian) (code: [StoreKeyerIdx::TermToIIDs](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/store/keyer.rs#L25));
3. **OID-To-IID**: maps an object identifier (ie. OID) to an internal identifier (ie. IID), which converts an user-provided object to a compact internal object (data is a 32 bits number encoded to binary as little-endian) (code: [StoreKeyerIdx::OIDToIID](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/store/keyer.rs#L26));
4. **IID-To-OID**: this is the reverse mapping of OID-To-IID, which lets convert an IID back to an OID (data is a variable-length UTF-8 string encoded in binary) (code: [StoreKeyerIdx::IIDToOID](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/store/keyer.rs#L27));
5. **IID-To-Terms**: this lists all words (ie. terms) associated to an internal identifier (ie. IID) (data is an array of 32 bits numbers encoded to binary as little-endian) (code: [StoreKeyerIdx::IIDToTerms](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/store/keyer.rs#L28));

A key is formatted as such, in binary: `[idx<1B> | bucket<4B> | route<4B>]` (code: [StoreKeyerBuilder::build_key](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/store/keyer.rs#L73)), which makes it 9-bytes long. The index stands for the type of key, eg. Term-To-IIDs. The bucket and what we call the route are hashed as 32 bits numbers, and appended in little-endian binary format to the key.

Both IIDs and terms are stored as 32 bits numbers in binary format. 64 bits numbers could have been used instead, increasing the total number of objects that can be indexed per-bucket. Though, storing such 64 bits numbers instead of 32 bits numbers would double required storage space. As they make up most of stored space, it was important to keep them as small as possible. Those 32 bits numbers are generated using a fast and low-collision hash family called [XxHash](http://www.xxhash.com), from the OID in the case of the IID, and from the word in the case of the term hash (code: [StoreTermHash](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/store/identifiers.rs#L32)).

## How do word suggestion and user typo auto-correction work?

-> how words are suggested / predicted
-> schema of the graph data structure

## How do texts get cleaned up? (via the lexer)

-> explain lexing & tokenization: language detection, normalization, stopwords, etc.
-> explain the concept of stopwords, and why its useless to store them in the search system

## What is the purpose of the tasker system?

-> fst consolidate: explain how does it work; why tasks are being commited to fst at periodic intervals (ie. fst is immutable and memory-mapped; and thus it needs to be fully re-built on disk and bad locks are involved)
-> janitor (kv + fst cache)

# On the Sonic Channel Protocol

-> explain the why-s of a custom protocol, and not a HTTP REST API
-> refer to the protocol.md document for specs

# Trade-Offs Sonic Makes

-> explain limit on block size, why this limit, etc. (configurable w/ default to 1k objects per word)
-> explain the 2^32 indexed objects limit due to IID mapping (we could have gone for 64 bits IIDs, but required storage space would have gone up 30%-40%; on big indexes this is overkill as only very specific applications would store more than 4B objects)
-> suggestion words / typos corrections are not made ready immediately (they need to be consolidated first; max time for the word to be available is set to the configured consolidate interval)
-> does a lot of disk random-access, thus it does not perform well on HDDs (which prefer sequential reads), but excels on modern SSDs

# The Journey of a Query

=> notice: refer to lines of code in the source code (versioned at commit hash)

-> push
-> query
-> pop

