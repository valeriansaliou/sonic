# Sonic search results ranking

In search engines like Sonic, it’s common for results to be ranked using
[tf-idf] or its “evolution” [BM25]. However, to keep a very small index Sonic
doesn’t store some information that’s necessary to compute [term frequency], a
core element of both [tf-idf] and [BM25].

Out of simplicity, before `v1.7.0` Sonic used to return results that contained
all search terms (with some fuzzy matching), sorted by reverse ingestion order
(i.e. most recent documents first). While this was good enough for some use
cases, discrepancies in ingestion and query languages would silently affect
which words were considered stopwords and prevent some documents[^doc] from
showing up in results (because of the implicit `AND`)[^1].

[^doc]: Since this document is about information retrieval as a general concept, this document will use the term “document” to refer to Sonic objects as it’s the strandard term.
[^1]: For example, see [issue #262](https://github.com/valeriansaliou/sonic/issues/262).

In `v1.7.0`, we reworked Sonic’s ranking algorithm to yield better results.

## Ranking algorithm

For each search term, Sonic looks for documents containing exactly that search
term, then it performs fuzzy matching until enough results are found. For each
term, Sonic keeps track of the [Levenshtein distance] of the closest word found
in each document. Once that done, a documents’s score is calculated by summing
up its scores for each term, and documents are finally sorted by score. When
ties occur, results are sorted by reverse ingestion order[^rio].

[^rio]: This happens at no cost and is invisible in the code, but happens because it’s the order in which results are yielded by the index (on purpose).

Here is some more detailed and accurate pseudo-code:

```txt
MISSING_MATCH_SCORE = 100

fn query(query, app_config) -> [doc] {
  limit = app_config.store.kv.retain_word_objects
  mut alternates_try = app_config.search.query_alternates_try

  // Updates the document’s score for a given term.
  // Returns whether the document was inserted or not.
  fn update_score(mut results_matrix, doc, score, term_idx, term_count) -> bool {
    if results_matrix contains doc:
      // Update score to keep minimum between existing and new score.
      results_matrix[doc][term_idx] = min(results_matrix[doc][term_idx], score)
      return false
    else:
      // By default, assign a high score to each term.
      mut scores = [MISSING_MATCH_SCORE; term_count]
      scores[term_idx] = score
      results_matrix[doc] = scores
      return true
  }

  mut results_matrix = []

  // Look for exact matches.
  for (index, term) in normalized(query):
    for doc in find_exact_matches(term):
      if len(results_matrix) < limit:
        score = 0
        update_score(results_matrix, doc, score, index, len(normalized(query)))

  // Look for words containing `term` as prefix.
  if len(results_matrix) < limit && alternates_try > 0:
    for (index, term) in normalized(query):
      for suggested_term in lookup_begins(term):
        for doc in limit(find_exact_matches(term), alternates_try):
          if len(results_matrix) < limit:
            // Compute Levenshtein distance.
            score = len(suggested_term) - len(term)
            if update_score(results_matrix, doc, score, index, len(normalized(query))):
              alternates_try -= 1

  fn typo_factor(word_len) -> u32 {
    if      word_len <= 3: return 0
    else if word_len <= 6: return 1
    else if word_len <= 9: return 2
    else                 : return 3
  }

  // Look for words like `term` (fuzzy matching).
  if len(results_matrix) < limit && alternates_try > 0:
    for (index, term) in normalized(query):
      max_typo_factor = typo_factor(len(term))
      mut typo_factor = 1

      // Iterate on increasingly larger typo factors (Levenshtein distances).
      while len(results_matrix) < limit && typo_factor <= max_typo_factor:
        for (suggested_term, lev_distance) in lookup_typos(term, typo_factor):
          for doc in limit(find_exact_matches(term), alternates_try):
            if len(results_matrix) < limit:
              // Save Levenshtein distance as score.
              if update_score(results_matrix, doc, lev_distance, index, len(normalized(query))):
                alternates_try -= 1
        typo_factor += 1

  // Computes final score for each document, summing individual term scores.
  // NOTE: Takes missing matches into account thanks to `MISSING_MATCH_SCORE`.
  fn flatten_matrix(results_matrix) -> [(doc, score)] {
    mut list = []

    for (doc, scores) in results_matrix:
      list += (doc, sum(scores))

    return list
  }

  // Creates a B-Tree of `[doc]` keyed by score, then flattens it.
  fn sort_by_score(results_map) -> [doc] {
    btree = btree_by_score(results_map)
    return flatten(btree)
  }

  return sort_by_score(flatten_matrix(results_matrix))
}
```

### Algorithm properties

- At most `store.kv.retain_word_objects` are returned.
- Fuzzy matching yields at most `search.query_alternates_try` results.
- Memory usage is linear, proportional to the number of results.
- Although the pseudo-code doesn’t show it, most operations are lazy, meaning
  only useful computations are executed.
- Scoring is calculated using integer values only, without a single floating
  point computation, division, square root or any other expensive operation.

### Possible future improvements

- Assign a weight to each term, based on [inverse document frequency], to give
  more precedence to rare terms.
  - This would imply making some floating point operations, but they’d only be
    `O(len(results))`, with idf computation being `O(1)`.
  - This could be done in parallel from lookups, as it is independent.
- Parallelize some operations (e.g. each term scoring)?
  - Overhead might be higher than gain, we’d have to benchmark.

[tf-idf]: https://en.wikipedia.org/wiki/Tf%E2%80%93idf
[BM25]: https://en.wikipedia.org/wiki/Okapi_BM25 "“Okapi BM25” on Wikipedia"
[term frequency]: https://en.wikipedia.org/wiki/Tf%E2%80%93idf#Term_frequency "“Term frequency” in “tf-idf” on Wikipedia"
[inverse document frequency]: https://en.wikipedia.org/wiki/Tf%E2%80%93idf#Inverse_document_frequency "“Inverse document frequency” in “tf-idf” on Wikipedia"
[Levenshtein distance]: https://en.wikipedia.org/wiki/Levenshtein_distance "“Levenshtein distance” on Wikipedia"
