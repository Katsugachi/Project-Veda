#[derive(Debug, Clone, PartialEq)]
pub struct VectorHit {
    pub document: usize,
    pub score: f32,
}

#[derive(Debug, Default)]
pub struct VectorIndex {
    dimensions: usize,
    vectors: Vec<Vec<f32>>,
}

impl VectorIndex {
    pub fn build(vectors: Vec<Vec<f32>>) -> Result<Self, VectorIndexError> {
        let dimensions = vectors.first().map_or(0, Vec::len);
        if dimensions == 0 && !vectors.is_empty() {
            return Err(VectorIndexError::EmptyVector);
        }
        if vectors.iter().any(|vector| vector.len() != dimensions) {
            return Err(VectorIndexError::DimensionMismatch);
        }
        Ok(Self {
            dimensions,
            vectors,
        })
    }

    pub fn search(&self, query: &[f32], limit: usize) -> Result<Vec<VectorHit>, VectorIndexError> {
        if query.len() != self.dimensions {
            return Err(VectorIndexError::DimensionMismatch);
        }
        let mut hits = self
            .vectors
            .iter()
            .enumerate()
            .map(|(document, vector)| VectorHit {
                document,
                score: cosine_similarity(query, vector),
            })
            .collect::<Vec<_>>();
        hits.sort_by(|left, right| right.score.total_cmp(&left.score));
        hits.truncate(limit);
        Ok(hits)
    }

    pub fn dimensions(&self) -> usize {
        self.dimensions
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum VectorIndexError {
    #[error("embedding vector dimensions do not match")]
    DimensionMismatch,
    #[error("embedding vectors cannot be empty")]
    EmptyVector,
}

pub fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    if left.len() != right.len() || left.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0_f32;
    let mut left_norm = 0.0_f32;
    let mut right_norm = 0.0_f32;
    for (left_value, right_value) in left.iter().zip(right) {
        dot += left_value * right_value;
        left_norm += left_value * left_value;
        right_norm += right_value * right_value;
    }
    let denominator = left_norm.sqrt() * right_norm.sqrt();
    if denominator <= f32::EPSILON {
        0.0
    } else {
        dot / denominator
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_prefers_same_direction() {
        let index = VectorIndex::build(vec![vec![1.0, 0.0], vec![0.0, 1.0]]).unwrap();
        assert_eq!(index.search(&[0.9, 0.1], 2).unwrap()[0].document, 0);
    }
}
