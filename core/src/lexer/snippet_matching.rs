// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::collections::HashSet;
use std::ops::{Range, RangeInclusive};

use indexmap::IndexMap;
use whatlang::Lang;

use crate::scoring::bm25_lite;

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

impl Default for QuerySnippetsConfig {
    fn default() -> Self {
        Self {
            window_count: 1,
            window_size: 42,
            max_window_padding: 41,
        }
    }
}

#[derive(Debug)]
pub struct TermScore {
    pub weight: f32,
    pub idf: f32,
}

pub fn get_best_snippets(
    doc: &str,
    lang: Option<Lang>,
    preprocessor: &Preprocessor,
    config: QuerySnippetsConfig,
    score_per_term: IndexMap<String, TermScore>,
) -> Vec<Snippet> {
    let QuerySnippetsConfig {
        window_count,
        window_size,
        max_window_padding,
    } = config;

    let doc_preprocessed = preprocessor.preprocess(doc, lang);

    let tokens = doc_preprocessed.tokens().collect::<Vec<Token>>();

    let Some(matches) = get_matches(&tokens, &score_per_term) else {
        return Vec::new();
    };

    let mut windows = get_windows(&config, &tokens, &matches);

    remove_overlaps(&mut windows, window_count);

    (windows.into_iter())
        .map(|mut window| {
            window.add_padding(window_size, max_window_padding, &tokens);
            window.into_snippet(doc)
        })
        .collect::<Vec<_>>()
}

fn get_matches<'a, 'b>(
    tokens: &'b [Token<'a>],
    score_per_term: &IndexMap<String, TermScore>,
) -> Option<Vec<(&'b Token<'a>, f32)>> {
    let mut term_counts = vec![0u8; score_per_term.len()];

    // PERF: Start with initial capacity, as we know there will be at least one match.
    let mut matches_tmp = Vec::with_capacity(16);

    for doc_token in tokens.iter().filter(|&t| t.is_not_stopword()) {
        if let Some((index, _, term_score)) = score_per_term.get_full(doc_token.as_normalized()) {
            term_counts[index] = term_counts[index].saturating_add(1);
            matches_tmp.push((doc_token, index, term_score));
        }
    }

    // There should always be at least one match, but in case some index error
    // (e.g. out-of-sync tf) causes this to run on a document with no match,
    // abort early (also acts as an assertion for later logic).
    debug_assert!(!matches_tmp.is_empty());
    if matches_tmp.is_empty() {
        return None;
    }

    let mut matches = Vec::with_capacity(matches_tmp.len());

    for (doc_token, index, TermScore { weight, idf }) in matches_tmp.into_iter() {
        let tf = term_counts[index];
        let score: f32 = weight * bm25_lite(tf, *idf);
        matches.push((doc_token, score));
    }

    Some(matches)
}

fn get_windows<'a, 'b, 'c: 'b>(
    config: &QuerySnippetsConfig,
    tokens: &'b [Token<'a>],
    matches: &'c [(&'b Token<'a>, f32)],
) -> Vec<SnippetWindow<'a, 'b>> {
    let window_size = config.window_size;

    let max_window_count = max_window_count(&matches, window_size);
    debug_assert!(max_window_count.is_some());
    // SAFETY: At this point, `matches` should not be empty, but there’s no
    //   harm in falling back to 0.
    let max_window_count = max_window_count.unwrap_or(0);

    let mut window_scores: Vec<SnippetWindow> = Vec::with_capacity(max_window_count);

    // PERF: To reduce the total number of computations, we avoid storing
    //   sub-windows of already-processed windows if we know for sure they
    //   will have a lower score. For example, `[3..5]` and `[4..5]` will
    //   always have a lower score than `[2..5]`, so we can ignore them.
    //   The rule is simple: if two windows have the same end index, then
    //   the one processed before has more matches and must have a higher
    //   score. If the end index is different, then they might still be
    //   overlapping but we’ll choose in another step which one to keep.
    let mut processed_windows_ends: HashSet<usize> = HashSet::with_capacity(max_window_count);

    // Compute window scores starting from match locations.
    for match_token_idx in 0..matches.len() {
        let start_token = &matches[match_token_idx].0;

        // PERF: While `partition_point` might be tempting here, we know
        //   that the partition point will be around the start so regular
        //   iteration becomes more efficient.
        let matches_in_window_count =
            (matches[match_token_idx..].iter()).position(|(token, _score)| {
                token.index_in_tokenized_text >= (start_token.index_in_tokenized_text + window_size)
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
            .map(|(_token, score)| score)
            .sum::<f32>();

        window_scores.push(SnippetWindow {
            tokens: tokens_in_window,
            matches: matches_in_window,
            score: window_score,
        });
    }

    drop(processed_windows_ends);

    window_scores.sort_by(|a, b| a.score.total_cmp(&b.score).reverse());

    window_scores
}

/// Ignore windows overlapping an already-yielded window (which has a better score).
fn remove_overlaps<'a, 'b>(windows: &mut Vec<SnippetWindow<'a, 'b>>, limit: usize) {
    let (all_windows, res) = {
        let res = Vec::with_capacity(limit.min(windows.len()));
        let all_windows = std::mem::replace(windows, res);
        (all_windows, windows)
    };

    let mut all_windows_iter = all_windows.into_iter();

    while let Some(window) = all_windows_iter.next()
        && res.len() < limit
    {
        // PERF: This makes the loop become `O(n^2)` but we don’t care as
        //   `n` will always be small.
        if (res.iter()).any(|existing| window.overlaps_with(existing)) {
            continue;
        }

        res.push(window);
    }
}

fn max_window_count<T>(matches: &[(&Token, T)], window_size: usize) -> Option<usize> {
    let first_match = matches.first()?.0;
    let last_match = matches.last()?.0;

    let matches_index_range =
        first_match.index_in_tokenized_text..last_match.index_in_tokenized_text;
    Some(matches_index_range.len() / window_size)
}

struct SnippetWindow<'a, 'b> {
    tokens: &'b [Token<'a>],
    matches: &'b [(&'b Token<'a>, f32)],
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

    fn overlaps_with(&self, other: &Self) -> bool {
        fn overlaps<T: Ord>(a: &RangeInclusive<T>, b: &RangeInclusive<T>) -> bool {
            a.start() <= b.end() && b.start() <= a.end()
        }

        overlaps(
            &self.range_in_tokenized_text(),
            &other.range_in_tokenized_text(),
        )
    }

    /// Number of tokens contained from start to end, including
    /// non-matches.
    fn token_count(&self) -> usize {
        self.tokens.last().unwrap().index_in_tokenized_text
            - self.tokens.first().unwrap().index_in_tokenized_text
            + 1
    }

    /// Add padding around windows so they don’t start and end with a matched
    /// token (or end up being 1 token long).
    fn add_padding(
        &mut self,
        window_size: usize,
        max_window_padding: usize,
        doc_tokens: &'b [Token<'a>],
    ) {
        let window_token_count = self.token_count();
        if window_token_count < window_size {
            let padding = (window_size - window_token_count).min(max_window_padding);

            let window_range = self.range_in_tokenized_text();

            // Split padding in two (start and end).
            // NOTE: We use `div_ceil` to get a bigger starting half for odd values.
            let padding_start = (padding.div_ceil(2))
                // Limit to document start.
                .min(*window_range.start());
            let padding_end = (padding - padding_start)
                // Limit to document end.
                .min(doc_tokens.len() - *window_range.end() - 1);

            let new_window_range =
                (*window_range.start() - padding_start)..=(*window_range.end() + padding_end);
            self.tokens = &doc_tokens[new_window_range];
        }
    }
}
