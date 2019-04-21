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

## How do result objects get indexed?

Sonic stores result objects in a key-value database (abbreviated KV), powered by RocksDB.

When a text is pushed to Sonic, this text gets normalized, cleaned up and splitted in separate words. Each word is then associated to the pushed object result, and commited to the KV database as `word <-> object`.

Upon cleaning the text, overhead is eluded. For instance, in the text `the lazy dog` there would be no point in indexing the word `the`, which is what is called a _stopword_. Sonic does not push stopwords to the index ([read more on stopwords](https://en.wikipedia.org/wiki/Stop_words)).

When objects are pushed to the search index for a given bucket in a given collection, for instance object `session_77f2e05e-5a81-49f0-89e3-177e9e1d1f32`, Sonic converts this object to a compact 32 bits format, for instance `10292198`. We call the user-provided object identifier the OID, while the compact internal identifier is named the IID. The IID is mapped internally to indexed words, and is much more compact in terms of storage than the OID. You can think of OIDs and IIDs as basically the same thing, except that the IID is the compact version of an OID. OIDs are only used for user-facing input and output objects, while IIDs are only used for internal storage of those objects. On very long indexed texts, this helps save **_a lot_** of disk space.

The KV store has a simple schema, where we associate a binary key to binary data. The following types of keys exist:

1. **Meta-To-Value**: state data for the bucket, eg. stores the count increment of indexed objects (data is in arbitrary format) (_code: [StoreKeyerIdx::MetaToValue](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/store/keyer.rs#L24)_);
2. **Term-To-IIDs**: maps a word (ie. term) to an internal identifier (ie. IID), which is essentialy a word-to-result mapping (data is an array of 32 bits numbers encoded to binary as little-endian) (_code: [StoreKeyerIdx::TermToIIDs](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/store/keyer.rs#L25)_);
3. **OID-To-IID**: maps an object identifier (ie. OID) to an internal identifier (ie. IID), which converts an user-provided object to a compact internal object (data is a 32 bits number encoded to binary as little-endian) (_code: [StoreKeyerIdx::OIDToIID](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/store/keyer.rs#L26)_);
4. **IID-To-OID**: this is the reverse mapping of OID-To-IID, which lets convert an IID back to an OID (data is a variable-length UTF-8 string encoded in binary) (_code: [StoreKeyerIdx::IIDToOID](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/store/keyer.rs#L27)_);
5. **IID-To-Terms**: this lists all words (ie. terms) associated to an internal identifier (ie. IID) (data is an array of 32 bits numbers encoded to binary as little-endian) (_code: [StoreKeyerIdx::IIDToTerms](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/store/keyer.rs#L28)_);

A key is formatted as such, in binary: `[idx<1B> | bucket<4B> | route<4B>]` (_code: [StoreKeyerBuilder::build_key](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/store/keyer.rs#L73)_), which makes it 9-bytes long. The index stands for the type of key, eg. Term-To-IIDs. The bucket and what we call the route are hashed as 32 bits numbers, and appended in little-endian binary format to the key.

Both IIDs and terms are stored as 32 bits numbers in binary format. 64 bits numbers could have been used instead, increasing the total number of objects that can be indexed per-bucket. Though, storing such 64 bits numbers instead of 32 bits numbers would double required storage space. As they make up most of stored space, it was important to keep them as small as possible. Those 32 bits numbers are generated using a fast and low-collision hash family called [XxHash](http://www.xxhash.com), from the OID in the case of the IID, and from the word in the case of the term hash (_code: [StoreTermHash](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/store/identifiers.rs#L32)_).

## How do word suggestion and user typo auto-correction work?

When most users input text to a computer system using an actual keyboard, they make typos and mistakes. A nice property of a good search system should be that those typos can be forgiven and accurate search results still come up for the bogus user query. Sonic implements a data structure that lets it correct typos or autocomplete incomplete words.

For instance, if our index has the word `english` but the user, for some reason, inputs `englich`, Sonic would still return results for `english`. Similarly, if the user inputs an incomplete word eg. `eng`, Sonic would expand this word to `english`, if there were no or not enough exact matches for `eng`.

The store system responsible for such a feat is the FST ([Finite-State Transducer](https://en.wikipedia.org/wiki/Finite-state_transducer)). It can be grossly compared to a graph of characters, where nodes are characters and edges connect those characters to produce words.

Sonic stores a single FST file per bucket. This FST file is memory-mapped, and read directly from the disk when Sonic needs to read it. The [fst](https://crates.io/crates/fst) crate is used to implement the FST data structure.

One downside of the FST implementation that Sonic uses, is that once built, an FST is immutable. It means that in order to add a new word to the search index (for a given bucket), Sonic needs to re-build the entire FST (ie. iterate word-by-word on the existing FST and stream those words plus the added word to a new on-disk FST file). In order to do that in an efficient manner, Sonic implements an FST consolidation tasker, which stores FST changes in-memory and consolidates them to disk at periodic intervals (this interval can be configured) (_code: [StoreFSTPool::consolidate](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/store/fst.rs#L173)_).

## How do texts get cleaned up? (via the lexer)

Any text that gets pushed to Sonic needs to be normalized (eg. lower-cased) and cleaned up (eg. remove stopwords) before it can be added to the index. This task is handled by the lexer system, also called [tokenizer](https://en.wikipedia.org/wiki/Lexical_analysis#Tokenization).

Sonic's tokenizer is built around an iterator pattern (_code: [Iterator->TokenLexer](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/lexer/token.rs#L244)_), and yields lexed words one-by-one. Iteration can be stopped before the end of the text is reached, for instance if we did not get enough search results for the first words of the query. This ensures no extraneous lexing work is done.

Given that stopwords depend on the text language, Sonic first needs to detect the language of the text that is being cleaned up. This is done using an hybrid method of either counting the number of stopwords that appear in the text for long-enough texts (which is faster) (_code: [TokenLexerBuilder::detect_lang_fast](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/lexer/token.rs#L177)_), or performing an [n-gram](https://en.wikipedia.org/wiki/N-gram) pass on the text for smaller texts (which is **_an order of magnitude_** slower) (_code: [TokenLexerBuilder::detect_lang_slow](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/lexer/token.rs#L126)_).

As the n-gram method is better at guessing the language for small texts than the stopwords method is, we prefer it, although it is crazy slow in comparison to the stopwords method. For long-enough texts, the stopwords method becomes reliable enough, so we can use it. In either cases, if the first chosen guessing method result is judged as non-reliable, Sonic fallbacks on the other method (_code: [!detector.is_reliable()](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/lexer/token.rs#L148)_).

By the way, Sonic builds up its own list of stopwords for all supported languages, [which can be found here](https://github.com/valeriansaliou/sonic/tree/master/src/stopwords) (languages are referred to via their ISO 639-3 codes). People are welcome to improve those lists of stopwords by [submitting a Pull Request](https://github.com/valeriansaliou/sonic/pulls).

## What is the purpose of the tasker system?

Looking at the source code of Sonic, you will find a module named `tasker` ([see here](https://github.com/valeriansaliou/sonic/tree/master/src/tasker)). This module performs background tasks, and is triggered periodically.

**The tasker performs the following actions:**

1. **Janitor**: it closes cached collection and bucket stores that were not used recently, freeing up memory (_code: [Tasker::tick](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/tasker/runtime.rs#L48)_);
2. **Consolidate**: it writes in-memory FST changes to the on-disk FST data structure (_code: [Tasker::tick](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/tasker/runtime.rs#L48)_);

As in all databases, a lot of locking is involved while the tasker is performing heavy-duty work on a KV or FST store. Thus, when the tasker system kicks-in, stores may experience higher than expected latency for all consumers attempting to read or write to them. The tasker system has been optimized to minimize thread contention caused by locks, so the impact of those locks on Sonic consumers should be minimum.

# On the Sonic Channel Protocol

In order for a client to communicate with the search index system, one needs a protocol. Sonic uses the Sonic Channel protocol, which defines a way for clients to send commands (ie. requests) to a Sonic server over the network (via a raw TCP socket); and get responses from the Sonic server. For instance, a client may send a search query command such as `QUERY collection bucket "search query"` and get a response with search results such as `EVENT QUERY isgsHQYu result_1 result_2`.

**On that Sonic Channel protocol, technical choices that may seem to go against common sense were made:**

1. **Sonic does not expose any HTTP API interface**, as it adds a network and processing overhead cost we do not want to bear;
2. **Sonic only exposes a raw TCP socket** with which clients interact via the Sonic Channel protocol, which was designed to be simple, lightweight and extensible;
3. **Most Sonic Channel commands are synchronous**, for simplicity's sake (Redis does the same). You can still run multiple Sonic Channel connections in parallel, and enjoy increased parallelism, but on a given Sonic Channel connection, you must wait for the previous command to return before issuing the next one;
4. **Some Sonic Channel commands are asynchronous**, when a lot of commands may be issued in a short period of time, in a burst pattern. This is typical of read operations such as search queries, which should be submitted as jobs to a dedicated thread pool, which can be upsized and downsized at will. To handle this, a special eventing protocol format should be used;

_The Sonic Channel protocol is specified in a separate document, which [you can read here](https://github.com/valeriansaliou/sonic/blob/master/PROTOCOL.md)._

# The Journey of a Search Query

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
8. If not enough search results are found, tries to suggest other words eg. typos corrections (_code: [fst_action.suggest_words](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/executor/search.rs#L81)_);
9. Perform paging on found OIDs from KV store to limit results (_code: [found_iids.iter](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/executor/search.rs#L163)_);
10. Return found OIDs from the executor (_code: [result_oids](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/executor/search.rs#L180)_);
11. Write back the final results to the TCP stream (_code: [response_args_groups](https://github.com/valeriansaliou/sonic/blob/5320b81afc1598ac1cd2af938df0b2ef6cb96dc4/src/channel/message.rs#L81)_);

_This is it!_ John Doe would receive the following response from Sonic Channel: `EVENT QUERY isgsHQYu conversation_3459 conversation_29398`, which indicates that there are 2 conversations that contain messages matching the search text `"The robber has stolen our corporate car"`.
