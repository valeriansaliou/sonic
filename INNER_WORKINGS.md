Sonic Inner Workings
====================

-> intro; tell that the document was written to stimulate contributions and let anyone build their simple search engine backend if they are willing to. not that hard of a task.
-> be liberal on schemas

# The Building Blocks of a Search Index

## Basic operations of a search index

-> base operations a search index should provide (push, pop, query, flushes)
-> access protocol (explain why it should be light & optimized; why HTTP is bad for this purpose); refer to the explanatory section

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

