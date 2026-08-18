use crate::lexical::take_top_k;
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq)]
pub struct VectorHit {
    pub document: usize,
    pub score: f32,
}

/// Cosine index. Vectors are L2-normalised at build time so a search is a
/// plain dot product, and an optional allowed-set skips every other pack.
#[derive(Debug, Default)]
pub struct VectorIndex {
    dimensions: usize,
    vectors: Vec<Vec<f32>>,
}

impl VectorIndex {
    pub fn build(mut vectors: Vec<Vec<f32>>) -> Result<Self, VectorIndexError> {
        // Local packs are indexed without embeddings (empty vecs) while
        // official packs carry 384-d BGE vectors. Mixing the two used to
        // return DimensionMismatch and take down *every* search, including
        // the official packs the user already had.
        let dimensions = vectors
            .iter()
            .find(|vector| !vector.is_empty())
            .map(Vec::len)
            .unwrap_or(0);
        if dimensions == 0 {
            return Ok(Self {
                dimensions: 0,
                vectors,
            });
        }
        if vectors
            .iter()
            .any(|vector| !vector.is_empty() && vector.len() != dimensions)
        {
            return Err(VectorIndexError::DimensionMismatch);
        }
        for vector in &mut vectors {
            if vector.is_empty() {
                vector.resize(dimensions, 0.0);
            }
            normalize_in_place(vector);
        }
        Ok(Self {
            dimensions,
            vectors,
        })
    }

    pub fn search(&self, query: &[f32], limit: usize) -> Result<Vec<VectorHit>, VectorIndexError> {
        self.search_filtered(query, limit, None)
    }

    pub fn search_filtered(
        &self,
        query: &[f32],
        limit: usize,
        allowed: Option<&HashSet<usize>>,
    ) -> Result<Vec<VectorHit>, VectorIndexError> {
        if self.dimensions == 0 || self.vectors.is_empty() {
            return Ok(Vec::new());
        }
        if query.len() != self.dimensions {
            return Err(VectorIndexError::DimensionMismatch);
        }
        let mut query = query.to_vec();
        normalize_in_place(&mut query);
        let mut hits = Vec::new();
        if let Some(allowed) = allowed {
            hits.reserve(allowed.len().min(self.vectors.len()));
            for &document in allowed {
                let Some(vector) = self.vectors.get(document) else {
                    continue;
                };
                hits.push(VectorHit {
                    document,
                    score: dot(&query, vector),
                });
            }
        } else {
            hits.extend(
                self.vectors
                    .iter()
                    .enumerate()
                    .map(|(document, vector)| VectorHit {
                        document,
                        score: dot(&query, vector),
                    }),
            );
        }
        take_top_k(&mut hits, limit, |left, right| {
            right.score.total_cmp(&left.score)
        });
        Ok(hits)
    }

    pub fn dimensions(&self) -> usize {
        self.dimensions
    }

    pub fn is_usable(&self) -> bool {
        self.dimensions > 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum VectorIndexError {
    #[error("embedding vector dimensions do not match")]
    DimensionMismatch,
}

pub fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    if left.len() != right.len() || left.is_empty() {
        return 0.0;
    }
    let mut left_normed = left.to_vec();
    let mut right_normed = right.to_vec();
    normalize_in_place(&mut left_normed);
    normalize_in_place(&mut right_normed);
    dot(&left_normed, &right_normed)
}

fn normalize_in_place(vector: &mut [f32]) {
    let mut norm = 0.0_f32;
    for value in vector.iter() {
        norm += *value * *value;
    }
    let norm = norm.sqrt();
    if norm <= f32::EPSILON {
        return;
    }
    for value in vector.iter_mut() {
        *value /= norm;
    }
}

fn dot(left: &[f32], right: &[f32]) -> f32 {
    left.iter()
        .zip(right)
        .map(|(left_value, right_value)| left_value * right_value)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_prefers_same_direction() {
        let index = VectorIndex::build(vec![vec![1.0, 0.0], vec![0.0, 1.0]]).unwrap();
        assert_eq!(index.search(&[0.9, 0.1], 2).unwrap()[0].document, 0);
    }

    #[test]
    fn empty_vectors_build_a_lexical_only_index() {
        let index = VectorIndex::build(vec![vec![], vec![]]).unwrap();
        assert!(!index.is_usable());
        assert!(index.search(&[0.1, 0.2], 4).unwrap().is_empty());
    }

    #[test]
    fn mixed_empty_and_real_vectors_do_not_poison_the_index() {
        // Official pack (real embedding) + local pack (empty) used to fail
        // HybridIndex::build and make ask_veda return "could not build the
        // search index" for every subsequent question.
        let index = VectorIndex::build(vec![vec![1.0, 0.0], vec![], vec![0.0, 1.0]]).unwrap();
        assert!(index.is_usable());
        assert_eq!(index.dimensions(), 2);
        assert_eq!(index.search(&[1.0, 0.0], 1).unwrap()[0].document, 0);
    }

    #[test]
    fn empty_first_vector_does_not_collapse_the_index() {
        let index = VectorIndex::build(vec![vec![], vec![0.0, 1.0]]).unwrap();
        assert_eq!(index.search(&[0.0, 1.0], 1).unwrap()[0].document, 1);
    }

    #[test]
    fn filtered_search_only_scores_allowed_documents() {
        let index =
            VectorIndex::build(vec![vec![1.0, 0.0], vec![0.9, 0.1], vec![0.0, 1.0]]).unwrap();
        let mut allowed = HashSet::new();
        allowed.insert(2);
        let hits = index
            .search_filtered(&[1.0, 0.0], 8, Some(&allowed))
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].document, 2);
    }
}
