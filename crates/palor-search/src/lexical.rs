use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq)]
pub struct LexicalHit {
    pub document: usize,
    pub score: f32,
}

#[derive(Debug, Default)]
pub struct LexicalIndex {
    document_lengths: Vec<usize>,
    term_frequencies: Vec<HashMap<String, usize>>,
    document_frequencies: HashMap<String, usize>,
    average_length: f32,
}

impl LexicalIndex {
    pub fn build(texts: impl IntoIterator<Item = String>) -> Self {
        let mut index = Self::default();
        for text in texts {
            let terms = tokenize(&text);
            let mut frequencies = HashMap::new();
            for term in &terms {
                *frequencies.entry(term.clone()).or_insert(0) += 1;
            }
            let unique = frequencies.keys().cloned().collect::<HashSet<_>>();
            for term in unique {
                *index.document_frequencies.entry(term).or_insert(0) += 1;
            }
            index.document_lengths.push(terms.len());
            index.term_frequencies.push(frequencies);
        }
        if !index.document_lengths.is_empty() {
            index.average_length = index.document_lengths.iter().sum::<usize>() as f32
                / index.document_lengths.len() as f32;
        }
        index
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<LexicalHit> {
        let terms = tokenize(query).into_iter().collect::<HashSet<_>>();
        if terms.is_empty() || self.term_frequencies.is_empty() {
            return Vec::new();
        }
        let document_count = self.term_frequencies.len() as f32;
        let k1 = 1.2_f32;
        let b = 0.75_f32;
        let mut results = Vec::new();
        for (document, frequencies) in self.term_frequencies.iter().enumerate() {
            let length = self.document_lengths[document] as f32;
            let mut score = 0.0_f32;
            for term in &terms {
                let frequency = *frequencies.get(term).unwrap_or(&0) as f32;
                if frequency == 0.0 {
                    continue;
                }
                let document_frequency = *self.document_frequencies.get(term).unwrap_or(&0) as f32;
                let inverse_document_frequency = ((document_count - document_frequency + 0.5)
                    / (document_frequency + 0.5)
                    + 1.0)
                    .ln();
                let denominator =
                    frequency + k1 * (1.0 - b + b * length / self.average_length.max(1.0));
                score += inverse_document_frequency * frequency * (k1 + 1.0) / denominator;
            }
            if score > 0.0 {
                results.push(LexicalHit { document, score });
            }
        }
        results.sort_by(|left, right| right.score.total_cmp(&left.score));
        results.truncate(limit);
        results
    }

    pub fn len(&self) -> usize {
        self.term_frequencies.len()
    }

    pub fn is_empty(&self) -> bool {
        self.term_frequencies.is_empty()
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
}
