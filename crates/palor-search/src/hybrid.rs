use crate::{LexicalIndex, VectorIndex};
use palor_core::DocsetId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchChunk {
    pub id: String,
    pub docset: DocsetId,
    pub version: String,
    pub title: String,
    pub section: String,
    pub url: String,
    pub text: String,
    #[serde(default)]
    pub symbols: Vec<String>,
    #[serde(default)]
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, Copy)]
pub struct HybridSearchOptions {
    pub limit: usize,
    pub candidate_limit: usize,
    pub lexical_weight: f32,
    pub semantic_weight: f32,
    pub rrf_k: f32,
}

impl Default for HybridSearchOptions {
    fn default() -> Self {
        Self {
            limit: 10,
            candidate_limit: 32,
            lexical_weight: 1.0,
            semantic_weight: 1.0,
            rrf_k: 60.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HybridHit {
    pub document: usize,
    pub score: f32,
    pub lexical_score: Option<f32>,
    pub semantic_score: Option<f32>,
}

#[derive(Debug)]
pub struct HybridIndex {
    chunks: Vec<SearchChunk>,
    lexical: LexicalIndex,
    vectors: VectorIndex,
}

impl HybridIndex {
    pub fn build(chunks: Vec<SearchChunk>) -> Result<Self, HybridIndexError> {
        let lexical = LexicalIndex::build(chunks.iter().map(indexable_text));
        let vectors =
            VectorIndex::build(chunks.iter().map(|chunk| chunk.embedding.clone()).collect())?;
        Ok(Self {
            chunks,
            lexical,
            vectors,
        })
    }

    pub fn search(
        &self,
        query: &str,
        query_vector: &[f32],
        symbols: &[String],
        docsets: &[DocsetId],
        options: HybridSearchOptions,
    ) -> Result<Vec<HybridHit>, HybridIndexError> {
        let lexical = self.lexical.search(query, options.candidate_limit);
        let semantic = self.vectors.search(query_vector, options.candidate_limit)?;
        let mut merged: HashMap<usize, HybridHit> = HashMap::new();
        for (rank, hit) in lexical.iter().enumerate() {
            if !docsets.is_empty() && !docsets.contains(&self.chunks[hit.document].docset) {
                continue;
            }
            let item = merged.entry(hit.document).or_insert(HybridHit {
                document: hit.document,
                score: 0.0,
                lexical_score: None,
                semantic_score: None,
            });
            item.score += options.lexical_weight / (options.rrf_k + rank as f32 + 1.0);
            item.lexical_score = Some(hit.score);
        }
        for (rank, hit) in semantic.iter().enumerate() {
            if !docsets.is_empty() && !docsets.contains(&self.chunks[hit.document].docset) {
                continue;
            }
            let item = merged.entry(hit.document).or_insert(HybridHit {
                document: hit.document,
                score: 0.0,
                lexical_score: None,
                semantic_score: None,
            });
            item.score += options.semantic_weight / (options.rrf_k + rank as f32 + 1.0);
            item.semantic_score = Some(hit.score);
        }
        for item in merged.values_mut() {
            let chunk = &self.chunks[item.document];
            if symbols.iter().any(|wanted| {
                chunk
                    .symbols
                    .iter()
                    .any(|found| found.eq_ignore_ascii_case(wanted))
            }) {
                item.score += 0.0125;
            }
        }
        let mut results = merged.into_values().collect::<Vec<_>>();
        results.sort_by(|left, right| right.score.total_cmp(&left.score));
        results.truncate(options.limit.clamp(1, 32));
        Ok(results)
    }

    pub fn chunk(&self, document: usize) -> Option<&SearchChunk> {
        self.chunks.get(document)
    }

    pub fn find_by_url(&self, url: &str) -> Option<&SearchChunk> {
        self.chunks.iter().find(|chunk| chunk.url == url)
    }

    pub fn len(&self) -> usize {
        self.chunks.len()
    }
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }
}

fn indexable_text(chunk: &SearchChunk) -> String {
    format!(
        "{} {} {} {}",
        chunk.title,
        chunk.section,
        chunk.symbols.join(" "),
        chunk.text
    )
}

#[derive(Debug, thiserror::Error)]
pub enum HybridIndexError {
    #[error(transparent)]
    Vector(#[from] crate::vector::VectorIndexError),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(id: &str, text: &str, symbol: &str, embedding: Vec<f32>) -> SearchChunk {
        SearchChunk {
            id: id.into(),
            docset: DocsetId::Python,
            version: "3.14".into(),
            title: text.into(),
            section: text.into(),
            url: id.into(),
            text: text.into(),
            symbols: vec![symbol.into()],
            embedding,
        }
    }

    #[test]
    fn hybrid_fusion_and_symbol_bonus_work() {
        let index = HybridIndex::build(vec![
            chunk(
                "one",
                "structured concurrency",
                "asyncio.TaskGroup",
                vec![1.0, 0.0],
            ),
            chunk("two", "event loop", "asyncio.run", vec![0.8, 0.2]),
            chunk("three", "css grid", "grid-template", vec![0.0, 1.0]),
        ])
        .unwrap();
        let hits = index
            .search(
                "TaskGroup concurrency",
                &[1.0, 0.0],
                &["asyncio.TaskGroup".into()],
                &[DocsetId::Python],
                HybridSearchOptions::default(),
            )
            .unwrap();
        assert_eq!(hits[0].document, 0);
        assert!(hits[0].lexical_score.is_some());
        assert!(hits[0].semantic_score.is_some());
    }
}
