// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::collections::{HashMap, HashSet};
use std::ops::{Range, RangeInclusive};

use whatlang::Lang;

use super::preprocessor::{Preprocessor, Token};

#[derive(Debug)]
#[derive(serde::Serialize)]
pub struct Snippet {
    pub text: String,
    pub matches: Vec<MatchRange>,
    pub score: f32,
    pub range: MatchRange,
}

#[derive(Debug)]
#[derive(serde::Serialize)]
pub struct MatchRange {
    pub start: usize,
    pub end: usize,
}

impl<'a, 'b> SnippetWindow<'a, 'b> {
    fn into_snippet(self, original_text: &str) -> Snippet {
        let range = self.range_in_original_text();
        let shift = range.start;

        Snippet {
            range: MatchRange {
                start: range.start,
                end: range.end,
            },
            text: original_text[range].to_owned(),
            matches: (self.matches.iter())
                .map(|(token, _score)| MatchRange {
                    start: token.start - shift,
                    end: token.end - shift,
                })
                .collect(),
            score: self.score,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct QuerySnippetsConfig {
    pub window_count: usize,
    pub window_size: usize,
    pub max_window_padding: usize,
}

pub fn get_best_snippets(
    doc: &str,
    lang: Option<Lang>,
    preprocessor: &Preprocessor,
    config: QuerySnippetsConfig,
    idf_by_token: HashMap<String, (f32, usize)>,
) -> Vec<Snippet> {
    let QuerySnippetsConfig {
        window_count,
        window_size,
        max_window_padding,
    } = config;

    let doc_preprocessed = preprocessor.preprocess(doc, lang);

    let mut term_counts = vec![0usize; idf_by_token.len()];

    // PERF: Start with initial capacity, as we know there will be at least one match.
    let mut matches_tmp = Vec::with_capacity(16);

    for doc_token in doc_preprocessed.tokens().filter(Token::is_not_stopword) {
        if let Some((idf, index)) = idf_by_token.get(doc_token.as_normalized()) {
            term_counts[*index] += 1;
            matches_tmp.push((doc_token, *index, *idf));
        }
    }

    let mut matches = Vec::with_capacity(matches_tmp.len());

    for (doc_token, index, idf) in matches_tmp.into_iter() {
        let tf: f32 = term_counts[index] as f32 / doc_preprocessed.tokens().len() as f32;
        let score: f32 = tf * idf;
        matches.push((doc_token, Score { tf, idf, score }));
    }

    let tokens = (doc_preprocessed.tokens()).collect::<Vec<Token>>();
    let mut window_scores: Vec<SnippetWindow> = Vec::with_capacity(tokens.len());

    // PERF: To reduce the total number of computations, we avoid storing
    //   sub-windows of already-processed windows if we know for sure they
    //   will have a lower score. For example, `[3..5]` and `[4..5]` will
    //   always have a lower score than `[2..5]`, so we can ignore them.
    //   The rule is simple: if two windows have the same end index, then
    //   the one processed before has more matches and must have a higher
    //   score. If the end index is different, then they might still be
    //   overlapping but we’ll choose in another step which one to keep.
    let mut processed_windows_ends: HashSet<usize> = HashSet::with_capacity(matches.len());

    // Compute window scores starting from match locations.
    for match_token_idx in 0..matches.len() {
        let start_token = &matches[match_token_idx].0;

        // PERF: While `partition_point` might be tempting here, we know
        //   that the partition point will be around the start so regular
        //   iteration becomes more efficient.
        let matches_in_window_count =
            (matches[match_token_idx..].iter()).position(|(token, _score)| {
                token.index_in_tokenized_text >= start_token.index_in_tokenized_text + window_size
            });

        let end = match matches_in_window_count {
            Some(count) => match_token_idx + count,
            None => matches.len(),
        };

        // Ignore already processed windows.
        if !processed_windows_ends.insert(end) {
            continue;
        }

        let matches_in_window = &matches[match_token_idx..end];
        let tokens_in_window = &tokens[matches_in_window.first().unwrap().0.index_in_tokenized_text
            ..=matches_in_window.last().unwrap().0.index_in_tokenized_text];

        // Soundness check.
        #[rustfmt::skip]
        assert!(tokens_in_window.len() <= window_size, "{} > {}", tokens_in_window.len(), window_size);

        let window_score = (matches_in_window.iter())
            // Sum up idfs as tf has already been taken into account to sort
            // the documents themselves. In a single document, tf gives a
            // low score to rare tokens, which is the opposite of what we
            // want here. We want to highlight terms that are both rare in
            // the document (low tf) and rare in the corpus (high idf).
            .map(|(_token, score)| score.idf)
            .sum::<f32>();

        window_scores.push(SnippetWindow {
            tokens: tokens_in_window,
            matches: matches_in_window,
            score: window_score,
        });
    }

    drop(processed_windows_ends);

    window_scores.sort_by(|a, b| a.score.total_cmp(&b.score).reverse());

    let mut window_scores_iter = window_scores.into_iter();

    let mut res: Vec<SnippetWindow> = Vec::with_capacity(window_count);

    let mut window_count = window_count;
    while let Some(mut window) = window_scores_iter.next()
        && window_count > 0
    {
        // Ignore windows overlapping an already-yielded window (which has
        // a better score).
        // PERF: This makes the loop become `O(n^2)` but we don’t care as
        //   `n` will always be small.
        if (res.iter()).any(|existing| window.overlaps_with(&existing.tokens)) {
            continue;
        }

        // Add padding around windows so they don’t start and end with a matched
        // token (or end up being 1 token long).
        let window_token_count = window.token_count();
        if window_token_count < window_size {
            let padding = (window_size - window_token_count).min(max_window_padding);

            let window_range = window.range_in_tokenized_text();

            // Split padding in two (start and end).
            // NOTE: We use `div_ceil` to get a bigger starting half for odd values.
            let padding_start = (padding.div_ceil(2))
                // Limit to document start.
                .min(*window_range.start());
            let padding_end = (padding - padding_start)
                // Limit to document end.
                .min(tokens.len() - *window_range.end() - 1);

            let new_window_range =
                (*window_range.start() - padding_start)..=(*window_range.end() + padding_end);
            window.tokens = &tokens[new_window_range];
        }

        res.push(window);

        window_count -= 1;
    }

    // Map windows to ranges.
    res.into_iter()
        .map(|window| window.into_snippet(doc))
        .collect::<Vec<_>>()
}

struct SnippetWindow<'a, 'b> {
    tokens: &'b [Token<'a>],
    matches: &'b [(Token<'a>, Score)],
    score: f32,
}

impl<'a, 'b> SnippetWindow<'a, 'b> {
    fn range_in_tokenized_text(&self) -> RangeInclusive<usize> {
        self.tokens.first().unwrap().index_in_tokenized_text
            ..=self.tokens.last().unwrap().index_in_tokenized_text
    }

    fn range_in_original_text(&self) -> Range<usize> {
        self.tokens.first().unwrap().start..self.tokens.last().unwrap().end
    }

    fn overlaps_with(&self, tokens: &[Token]) -> bool {
        fn overlaps<T: Ord>(a: &RangeInclusive<T>, b: &RangeInclusive<T>) -> bool {
            a.start() <= b.end() && b.start() <= a.end()
        }

        let a = self.range_in_tokenized_text();
        let b = tokens.first().unwrap().index_in_tokenized_text
            ..=tokens.last().unwrap().index_in_tokenized_text;
        overlaps(&a, &b)
    }

    /// Number of tokens contained from start to end, including
    /// non-matches.
    fn token_count(&self) -> usize {
        self.tokens.last().unwrap().index_in_tokenized_text
            - self.tokens.first().unwrap().index_in_tokenized_text
            + 1
    }
}

#[derive(PartialEq)]
struct Score {
    tf: f32,
    idf: f32,
    score: f32,
}

impl std::fmt::Debug for Score {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Score { tf, idf, score } = self;
        write!(f, "{score} (tf={tf}, idf={idf})")
    }
}

impl PartialOrd for Score {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.score.partial_cmp(&other.score)
    }
}
