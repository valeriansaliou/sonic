Sonic Inner Workings
====================

This document was written with the goal of explaining the inner workings of Sonic, as well as the whys of the design choices that were made while building Sonic.

Anyone reading this documentation should quickly get more familiar in how such a search index can be built from scratch, to the point that they should be able to start building their own Sonic from scratch.

_If you feel something is missing from this document, or if it did not help you understand a concept Sonic implements, please [open an issue](https://github.com/valeriansaliou/sonic/issues/new) and explain precisely which part you did not get and why you think you did not get it._

# The Building Blocks of a Search Index

## Basics of a search index

A search index is nothing more than a specialized database. It should expose primitives such as: query the index, push text in the index, pop text from the index, flush parts of the index.

The search index server is responsible for organizing the index data in a way that makes writes and reads efficient. It makes uses of specialized data structures for some very specific operations like typos corrections. The overall goal of such a search index system is: speed, lightweightness and data compactness (ie. it should minimize the resulting output database size given a text input size).

It is worth nothing that any project initiated as of 2019 should make use of modern server hardware, which is mostly all about multi-core CPUs and SSDs. Also, Sonic should be very wary of minimizing its resource requirements — from a cold start to running under high load — as a lot of developers nowadays expect to run software on cheap VPS servers with limited CPU time, small disk space and little RAM. Those modern VPS are nonetheless powered by modern SSDs with fast random I/O. Last but not least, it would definitely be a plus if we could make software a bit greener. Sonic addresses all that.

In order for a client to communicate with the search index system, one needs a protocol. Sonic uses the Sonic Channel protocol, which defines a way for clients to send commands (ie. requests) to a Sonic server over the network (via a raw TCP socket); and get responses from the Sonic server. For instance, a client may send a search query command such as `QUERY collection bucket "search query"` and get a response with search results such as `EVENT QUERY isgsHQYu result_1 result_2`.

**On that Sonic Channel protocol, technical choices that may seem to go against common sense were made:**

1. **Sonic does not expose any HTTP API interface**, as it adds a network and processing overhead cost we do not want to bear;
2. **Sonic only exposes a raw TCP socket** with which clients interact via the Sonic Channel protocol, which was designed to be simple, lightweight and extensible;
3. **Most Sonic Channel commands are synchronous**, for simplicity's sake (Redis does the same). You can still run multiple Sonic Channel connections in parallel, and enjoy increased parallelism, but on a given Sonic Channel connection, you must wait for the previous command to return before issuing the next one;
4. **Some Sonic Channel commands are asynchronous**, when a lot of commands may be issued in a short period of time, in a burst pattern. This is typical of read operations such as search queries, which should be submitted as jobs to a dedicated thread pool, which can be upsized and downsized at will. To handle this, a special eventing protocol format should be used;

_The Sonic Channel protocol is specified in a separate document, which [you can read here](https://github.com/valeriansaliou/sonic/blob/master/PROTOCOL.md)._

## How do result objects get indexed?

-> schema of the kv store (with data types and so)
-> explain why rocksdb has been chosen
-> store only useful words (ie. not stopwords)
-> storage compactness: how to achieve that (small hashes w/ low collision probability, binary storage, IID<>OID mappings)
-> keyer system (how does it work)

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

