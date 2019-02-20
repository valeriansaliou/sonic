TODO
====

# 19th February 2019

- [x] bootstrap code basis
- [x] build channel manager
- [x] commonize channel/handle.rs stuff w/ generics
- [x] write query format validator for all search + ingest
- [x] fix internal_error when command is not recognized
- [x] change how buffers are managed; allow a max buffer size for socket, else abort connection
- [x] implement optional arguments splitter
- [x] write a dummy search factory that always return the same dummy search results (pseudo-async)

# 20th February 2019

- [x] write the README explanations + install + protocol + etc
- [x] help command to list available commands (`HELP [<manual>]?`)
- [x] support for OFFSET in search results (after LIMIT argument)
- [x] beautify query meta value parser (commonize w/ generics)
- [x] support for quoted <terms> + quoted <text> args (both in search + ingest commands)
- [x] ensure metas still work with quote support
- [x] write base NodeJS library and make it work with dummy operations
- [x] write base NodeJS library README

# 21st February 2019

- [ ] Library: build automated tests (search + ingest)
- [ ] Library: finish 100% README
- [ ] re-write un-escaping of text (restrict to \n and " unescapes)
- [ ] fix text parser in all contexts (polish its edge cases)
- [ ] build base database managers (graph + key-value)
- [ ] setup base indexer
- [ ] setup base query system

# 22nd February 2019

- [ ] setup base lexer
