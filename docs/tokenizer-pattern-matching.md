---
author: Rémi Bardon <remi@remibardon.name>
created: 2026-07-09
---

# Sonic tokenizer’s pattern matching

## Context

Since its very beginning, Sonic has always supported prefix and fuzzy matching.
However, at some point a bug creeped in and caused fuzzy matching to correct at
most 1 typo. This bug went unnoticed for years and users became used to it.

Let’s take a simplified example (pseudo-code):

```txt
Ingest (Sonic <= v1.7.3):
  PUSH doc:1 "olivia@example.org"
              └────┘ └─────────┘
  PUSH doc:2 "olivio@example.org" # Lev(doc:1, doc:2) = 1
              └────┘ └─────────┘
  PUSH doc:3 "alicia@example.org" # Lev(doc:1, doc:3) = 2
              └────┘ └─────────┘
  PUSH doc:4 "olivia@example.com" # Lev(doc:1, doc:4) = 3
              └────┘ └─────────┘
  PUSH doc:5 "olivia and olivio"
              └────┘ └─┘ └────┘

In the index: {
  "olivia", "example.org", "olivio", "alicia", "example.com", "and"
}
```

Before `v1.7.0`, because typo correction was always limited to 1 character,
searching for `olivia@example.org` would yield the following results:

```txt
Search (v1.6.0):
  QUERY "olivia@example.org"
         └─┬──┘ └────┬────┘
         1 typo    1 typo
       ┌───┴───┐     └───┐
     olivia  olivio  example.org
       └─ OR ──┘         │
          └───── AND ────┘
  RESULT doc:2 doc:1 # Too many results (expected: doc:1)
```

However, because 1 character differences in both the username or domain part of
an email address are very unlikely, no one ever noticed (and complained).

## The v1.7.0 problem

In v1.7.0 we fixed Sonic’s typo correction, which now allows more typos as the
word gets longer (which was supposed to happen since the beginning).
Unfortunately, this fix didn’t go unnoticed, as it only took a few hours after
deployment for [Crisp] users to start complaining about unexpected results when
querying phone numbers or email addresses.

[Crisp]: https://crisp.chat "Crisp homepage"

In `v1.7.0`, searching for `olivia@example.org` would now yield far too many
results:

```txt
Note: In `v1.7.0`, 6-letters words are allowed 1 typo. For example’s sake,
we’ll pretend `olivia` is 7-letters long so it matches up to 2 typos. Off the
top of my head I couldn’t find better examples.

Search (v1.7.0):
  QUERY "olivia@example.org"
         └─┬──┘ └────┬────┘
        2 typos      └── 3 typos ───┐
      ┌────┴──┬───────┐         ┌───┴────────┐
    olivia  olivio  alicia  example.org  example.com
      └───────┴─ OR ──┘         └──── OR ────┘
                  └─────── OR ────────┘
  RESULT doc:1 doc:2 doc:3 doc:4 doc:5 # Too many results (expected: doc:1)
```

Although results were ordered perfectly, users were expecting not to see fuzzy
matches at all. We didn’t want to revert the typo correction fix, as it made
sense for a ton of use cases, and had to come up with a non-breaking workaround.

## Workaround: Forcing some terms to be matched exactly, and at least once

In [Pull Request #365] and [Pull Request #367] we made changes to the `QUERY`
executor so it would consider any term containing non-prose characters (e.g.
digits, `.`, `_`, etc.) to be an identifier. Identifiers would then not be
subject to prefix nor fuzzy matching, and they would be required to be present
in all results (implicit `AND`). This was a huge win, but it still had flaws:

[Pull Request #365]: https://github.com/valeriansaliou/sonic/pull/365
[Pull Request #367]: https://github.com/valeriansaliou/sonic/pull/367

```txt
Search (v1.7.3):
  QUERY "olivia@example.org"
         └─┬──┘ └────┬────┘
        2 typos      └─ Exact ──┐
      ┌────┴──┬───────┐         │
    olivia  olivio  alicia  example.org
      └───────┴─ OR ──┘         │
                  └──── AND ────┘
  RESULT doc:1 doc:2 doc:3 # Better than v1.7.0, but still too many results
                           # and arguably worse than v1.6.0 (expected: doc:1)
```

Because the tokenizer would split on `@`, `olivia` couldn’t be detected as an
identifier, and would still be fuzzy matched too broadly. Without making
changes to the tokenizer, working around this issue would have been very hacky,
so we went on and changed it.

## Solution: Making it so the tokenizer doesn’t split specific patterns

In [Pull Request #368], we made changes to the tokenizer so it could detect
some patterns (e.g. emails, phone numbers, UUIDs…). For those portions of the
query, because users expect exact matches, we’d disable fuzzy matching and
force the term to be present in results.

[Pull Request #368]: https://github.com/valeriansaliou/sonic/pull/368

Unfortunately, without rebuilding Sonic’s index, the `QUERY` executor would
look for terms that are not in the index, making this a breaking change:

```txt
Index (<= v1.7.3): {
  "olivia", "example.org", "olivio", "alicia", "example.com", "and"
}

Search (f290006):
  QUERY "olivia@example.org"
         └──────┬─────────┘
              Exact
                │
                ø
  RESULT # Regression: no more result
```

To avoid having to make a major release for Sonic, we hid this new feature
behind an opt-in configuration flag: `tokenization.detect_special_patterns`.
At least indexes wouldn’t become useless on update, but we were back to square
one (or `v1.7.3` to be more precise):

```txt
Search (ddd6848 with `tokenization.detect_special_patterns = false`):
  Same as in v1.7.3 (not great)
```

## Improvement: Making the solution non-breaking

At [Crisp] we can’t just rebuild the index because we’re bumping Sonic, because
—even though Sonic is fast— ingesting billions of messages still takes _hours_,
so it was time to look for a smarter idea.

If we want to get the results users expect (i.e. just `doc:1` in our example),
the tokenizer _has to_ be aware of patterns. But we also can’t force a rebuild
of the index in a non-breaking release, so we have to act in-between.

The solution we came up with is to enable `tokenization.detect_special_patterns`
by default but add `tokenization.split_special_patterns` to control whether or
not they should be further split. When `true`, terms are split just like in
`v1.6.0`, but they are now marked special and we can force exact matching. Here
is an example:

```txt
Index (<= v1.7.3): {
  "olivia", "example.org", "olivio", "alicia", "example.com", "and"
}

Search (v1.7.4 with `tokenization.split_special_patterns = true`):
  QUERY "olivia@example.org"
         └── Special ─────┘
         └─┬──┘ └────┬────┘
         Exact     Exact
           │         │
         olivia  example.org
           └── AND ──┘
  RESULT doc:1 # Expected result
```

In this example, the result is exactly what we want, but there is still one
edge case where a document contains both `olivia` and `example.org` (but not
`olivia@example.org`). Unfortunately it’s not possible for Sonic to work around
this case without its index being rebuilt with
`tokenization.split_special_patterns = false` (which will be the default in
Sonic `v2.0.0`). Here is why it would be perfect:

```txt
Ingest (v1.7.4 with `tokenization.split_special_patterns = false`):
  PUSH doc:1 "olivia@example.org"
              └────────────────┘
  PUSH doc:2 "olivio@example.org"
              └────────────────┘
  PUSH doc:3 "alicia@example.org"
              └────────────────┘
  PUSH doc:4 "olivia@example.com"
              └────────────────┘
  PUSH doc:5 "olivia and olivio"
              └────┘ └─┘ └────┘

In the index: {
  "olivia@example.org", "olivio@example.org", "alicia@example.org",
  "olivia@example.com", "olivia", "and", "olivio"
}

Search (v1.7.4 with `tokenization.split_special_patterns = false`):
  QUERY "olivia@example.org"
         └── Special ─────┘
                │
              Exact
                │
         olivia@example.org
  RESULT doc:1 # Perfect!
```
