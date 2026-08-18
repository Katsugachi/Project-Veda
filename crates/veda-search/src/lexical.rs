use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq)]
pub struct LexicalHit {
    pub document: usize,
    pub score: f32,
}

/// Inverted BM25 index. The previous implementation scanned every document
/// for every query term (O(N · |q|)). Technical questions typically hit a
/// handful of identifiers, so walking the postings list is orders of magnitude
/// cheaper once a pack has thousands of chunks.
#[derive(Debug, Default)]
pub struct LexicalIndex {
    document_lengths: Vec<usize>,
    /// term → (document, term-frequency)
    postings: HashMap<String, Vec<(usize, u16)>>,
    document_frequencies: HashMap<String, usize>,
    average_length: f32,
}

impl LexicalIndex {
    pub fn build(texts: impl IntoIterator<Item = String>) -> Self {
        let mut index = Self::default();
        for text in texts {
            let document = index.document_lengths.len();
            let terms = tokenize(&text);
            let mut frequencies = HashMap::new();
            for term in &terms {
                *frequencies.entry(term.clone()).or_insert(0_u16) += 1;
            }
            for (term, frequency) in frequencies {
                *index.document_frequencies.entry(term.clone()).or_insert(0) += 1;
                index
                    .postings
                    .entry(term)
                    .or_default()
                    .push((document, frequency));
            }
            index.document_lengths.push(terms.len());
        }
        if !index.document_lengths.is_empty() {
            index.average_length = index.document_lengths.iter().sum::<usize>() as f32
                / index.document_lengths.len() as f32;
        }
        index
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<LexicalHit> {
        self.search_filtered(query, limit, None)
    }

    /// BM25 over the postings of `query`, optionally restricted to `allowed`
    /// document ids. Restriction happens *before* scoring so a C++-heavy
    /// index cannot drown a Python question.
    pub fn search_filtered(
        &self,
        query: &str,
        limit: usize,
        allowed: Option<&HashSet<usize>>,
    ) -> Vec<LexicalHit> {
        let terms = tokenize(query).into_iter().collect::<HashSet<_>>();
        if terms.is_empty() || self.postings.is_empty() {
            return Vec::new();
        }
        let document_count = self.document_lengths.len() as f32;
        let k1 = 1.2_f32;
        let b = 0.75_f32;
        let mut scores: HashMap<usize, f32> = HashMap::new();
        for term in &terms {
            let Some(postings) = self.postings.get(term) else {
                continue;
            };
            let document_frequency = *self.document_frequencies.get(term).unwrap_or(&0) as f32;
            let inverse_document_frequency =
                ((document_count - document_frequency + 0.5) / (document_frequency + 0.5) + 1.0)
                    .ln();
            for &(document, frequency) in postings {
                if allowed.is_some_and(|set| !set.contains(&document)) {
                    continue;
                }
                let frequency = frequency as f32;
                let length = self.document_lengths[document] as f32;
                let denominator =
                    frequency + k1 * (1.0 - b + b * length / self.average_length.max(1.0));
                *scores.entry(document).or_insert(0.0) +=
                    inverse_document_frequency * frequency * (k1 + 1.0) / denominator;
            }
        }
        let mut results = scores
            .into_iter()
            .filter(|(_, score)| *score > 0.0)
            .map(|(document, score)| LexicalHit { document, score })
            .collect::<Vec<_>>();
        take_top_k(&mut results, limit, |left, right| {
            right.score.total_cmp(&left.score)
        });
        results
    }

    pub fn len(&self) -> usize {
        self.document_lengths.len()
    }

    pub fn is_empty(&self) -> bool {
        self.document_lengths.is_empty()
    }
}

pub fn tokenize(text: &str) -> Vec<String> {
    let lowercase = text.to_lowercase();
    let mut terms = Vec::new();
    let mut current = String::new();
    for character in lowercase.chars() {
        if character.is_alphanumeric() || matches!(character, '_' | ':' | '.' | '#' | '-') {
            current.push(character);
        } else if !current.is_empty() {
            push_term(&mut terms, &current);
            current.clear();
        }
    }
    if !current.is_empty() {
        push_term(&mut terms, &current);
    }
    terms
}

fn push_term(terms: &mut Vec<String>, term: &str) {
    terms.push(term.to_string());
    // Technical identifiers are searchable both exactly and by component.
    for part in term
        .split(['.', ':', '#', '-'])
        .filter(|part| part.len() > 1)
    {
        if part != term {
            terms.push(part.to_string());
        }
    }
}

/// Partial top-k: only the winners are fully sorted.
pub(crate) fn take_top_k<T, F>(items: &mut Vec<T>, k: usize, compare: F)
where
    F: Fn(&T, &T) -> std::cmp::Ordering,
{
    if items.len() > k {
        items.select_nth_unstable_by(k, |left, right| compare(left, right));
        items.truncate(k);
    }
    items.sort_by(compare);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_technical_symbol_ranks_first() {
        let index = LexicalIndex::build([
            "asyncio.TaskGroup structured concurrency".into(),
            "asyncio event loop policy".into(),
            "CSS grid layout".into(),
        ]);
        let hits = index.search("asyncio.TaskGroup", 3);
        assert_eq!(hits[0].document, 0);
    }

    #[test]
    fn tokenizer_preserves_and_splits_symbols() {
        let terms = tokenize("std::vector::push_back");
        assert!(terms.contains(&"std::vector::push_back".into()));
        assert!(terms.contains(&"vector".into()));
        assert!(terms.contains(&"push_back".into()));
    }

    #[test]
    fn filtered_search_ignores_out_of_scope_documents() {
        let index = LexicalIndex::build([
            "asyncio.TaskGroup".into(),
            "asyncio.TaskGroup again".into(),
            "unrelated".into(),
        ]);
        let mut allowed = HashSet::new();
        allowed.insert(1);
        let hits = index.search_filtered("asyncio.TaskGroup", 8, Some(&allowed));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].document, 1);
    }

    #[test]
    fn missing_terms_do_not_scan_the_corpus() {
        let index = LexicalIndex::build(std::iter::repeat_with(|| "alpha beta".into()).take(2_000));
        let hits = index.search("zzzz-not-a-term", 8);
        assert!(hits.is_empty());
    }
}
