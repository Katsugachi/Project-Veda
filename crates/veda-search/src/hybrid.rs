use crate::{LexicalIndex, VectorIndex};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use veda_core::DocsetId;

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
            limit: 8,
            candidate_limit: 24,
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
    by_url: HashMap<String, usize>,
    by_docset: HashMap<DocsetId, Vec<usize>>,
}

impl HybridIndex {
    pub fn build(chunks: Vec<SearchChunk>) -> Result<Self, HybridIndexError> {
        let lexical = LexicalIndex::build(chunks.iter().map(indexable_text));
        let vectors =
            VectorIndex::build(chunks.iter().map(|chunk| chunk.embedding.clone()).collect())?;
        let mut by_url = HashMap::with_capacity(chunks.len());
        let mut by_docset: HashMap<DocsetId, Vec<usize>> = HashMap::new();
        for (index, chunk) in chunks.iter().enumerate() {
            by_url.insert(chunk.url.clone(), index);
            by_docset.entry(chunk.docset).or_default().push(index);
        }
        Ok(Self {
            chunks,
            lexical,
            vectors,
            by_url,
            by_docset,
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
        let allowed = self.allowed_set(docsets);
        let allowed_ref = allowed.as_ref();
        let lexical = self
            .lexical
            .search_filtered(query, options.candidate_limit, allowed_ref);
        let semantic =
            if self.vectors.is_usable() && query_vector.len() == self.vectors.dimensions() {
                self.vectors
                    .search_filtered(query_vector, options.candidate_limit, allowed_ref)?
            } else {
                Vec::new()
            };
        let mut merged: HashMap<usize, HybridHit> = HashMap::new();
        for (rank, hit) in lexical.iter().enumerate() {
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
            let item = merged.entry(hit.document).or_insert(HybridHit {
                document: hit.document,
                score: 0.0,
                lexical_score: None,
                semantic_score: None,
            });
            item.score += options.semantic_weight / (options.rrf_k + rank as f32 + 1.0);
            item.semantic_score = Some(hit.score);
        }
        if !symbols.is_empty() {
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
        }
        let mut results = merged.into_values().collect::<Vec<_>>();
        crate::lexical::take_top_k(&mut results, options.limit.clamp(1, 32), |left, right| {
            right.score.total_cmp(&left.score)
        });
        Ok(results)
    }

    fn allowed_set(&self, docsets: &[DocsetId]) -> Option<HashSet<usize>> {
        if docsets.is_empty() {
            return None;
        }
        let mut allowed = HashSet::new();
        for docset in docsets {
            if let Some(members) = self.by_docset.get(docset) {
                allowed.extend(members.iter().copied());
            }
        }
        Some(allowed)
    }

    pub fn chunk(&self, document: usize) -> Option<&SearchChunk> {
        self.chunks.get(document)
    }

    pub fn find_by_url(&self, url: &str) -> Option<&SearchChunk> {
        self.by_url
            .get(url)
            .and_then(|index| self.chunks.get(*index))
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
    Vector(crate::vector::VectorIndexError),
}

impl From<crate::vector::VectorIndexError> for HybridIndexError {
    fn from(error: crate::vector::VectorIndexError) -> Self {
        Self::Vector(error)
    }
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

    #[test]
    fn find_by_url_is_constant_time() {
        let index = HybridIndex::build(vec![chunk("one", "a", "a", vec![1.0, 0.0])]).unwrap();
        assert!(index.find_by_url("one").is_some());
        assert!(index.find_by_url("missing").is_none());
    }

    #[test]
    fn lexical_only_index_still_retrieves() {
        let index =
            HybridIndex::build(vec![chunk("one", "asyncio TaskGroup", "TaskGroup", vec![])])
                .unwrap();
        let hits = index
            .search(
                "TaskGroup",
                &[],
                &[],
                &[DocsetId::Python],
                HybridSearchOptions::default(),
            )
            .unwrap();
        assert_eq!(hits[0].document, 0);
        assert!(hits[0].semantic_score.is_none());
    }

    #[test]
    fn mixed_official_and_local_chunks_still_search() {
        let index = HybridIndex::build(vec![
            chunk("py", "asyncio TaskGroup", "TaskGroup", vec![1.0, 0.0]),
            SearchChunk {
                id: "local-1".into(),
                docset: DocsetId::Local,
                version: "local-notes".into(),
                title: "Notes".into(),
                section: "body".into(),
                url: "veda://docs/local-notes/readme#top".into(),
                text: "our house style for TaskGroup".into(),
                symbols: vec![],
                embedding: vec![],
            },
        ])
        .unwrap();
        let hits = index
            .search(
                "TaskGroup",
                &[1.0, 0.0],
                &[],
                &[DocsetId::Python, DocsetId::Local],
                HybridSearchOptions::default(),
            )
            .unwrap();
        assert!(hits.iter().any(|hit| hit.document == 0));
        assert!(hits.iter().any(|hit| hit.document == 1));
    }

    #[test]
    fn two_local_libraries_with_the_same_filename_both_retrieve() {
        // README.md in two user folders must not collapse. HybridIndex keys
        // by document slot; the retriever later keys by chunk.id, so the
        // ids themselves must also differ (see chunk_page).
        let index = HybridIndex::build(vec![
            SearchChunk {
                id: "local:local-notes:readme.md:0".into(),
                docset: DocsetId::Local,
                version: "local-notes".into(),
                title: "Notes".into(),
                section: "body".into(),
                url: "veda://docs/local-notes/readme.md#top".into(),
                text: "house style for TaskGroup in our notes".into(),
                symbols: vec![],
                embedding: vec![],
            },
            SearchChunk {
                id: "local:local-api:readme.md:0".into(),
                docset: DocsetId::Local,
                version: "local-api".into(),
                title: "API".into(),
                section: "body".into(),
                url: "veda://docs/local-api/readme.md#top".into(),
                text: "TaskGroup timeout rules in the internal API".into(),
                symbols: vec![],
                embedding: vec![],
            },
        ])
        .unwrap();
        let hits = index
            .search(
                "TaskGroup",
                &[],
                &[],
                &[DocsetId::Local],
                HybridSearchOptions::default(),
            )
            .unwrap();
        assert_eq!(hits.len(), 2);
        assert!(index
            .find_by_url("veda://docs/local-notes/readme.md#top")
            .is_some());
        assert!(index
            .find_by_url("veda://docs/local-api/readme.md#top")
            .is_some());
    }

    #[test]
    fn large_index_search_stays_in_the_millisecond_range() {
        let mut chunks = Vec::with_capacity(8_000);
        for index in 0..8_000 {
            let text = if index == 1234 {
                "asyncio.TaskGroup structured concurrency cancellation"
            } else {
                "unrelated documentation paragraph about something else entirely"
            };
            chunks.push(SearchChunk {
                id: format!("c{index}"),
                docset: if index % 2 == 0 {
                    DocsetId::Python
                } else {
                    DocsetId::Cpp
                },
                version: "1".into(),
                title: format!("Page {index}"),
                section: "body".into(),
                url: format!("veda://docs/x/{index}"),
                text: text.into(),
                symbols: vec![],
                embedding: vec![],
            });
        }
        let index = HybridIndex::build(chunks).unwrap();
        let started = std::time::Instant::now();
        let hits = index
            .search(
                "asyncio.TaskGroup",
                &[],
                &[],
                &[DocsetId::Python],
                HybridSearchOptions::default(),
            )
            .unwrap();
        let elapsed = started.elapsed();
        assert_eq!(hits[0].document, 1234);
        // 8k chunks, inverted BM25, filtered to one pack: must be well under
        // 50 ms on this sandbox CPU. The old full scan was the thing we
        // killed; this bound is the proof.
        assert!(
            elapsed.as_millis() < 50,
            "hybrid search over 8k chunks took {elapsed:?}"
        );
    }
}
