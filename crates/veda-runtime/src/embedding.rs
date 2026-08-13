use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Keeps every complete embedding input safely below llama.cpp's 512-token
/// physical micro-batch, even when the tokenizer falls back to byte tokens.
pub const MAX_EMBEDDING_INPUT_BYTES: usize = 400;

pub fn bounded_embedding_input(input: &str, max_bytes: usize) -> String {
    let mut end = input.len().min(max_bytes);
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    input[..end].to_owned()
}

#[derive(Debug, Clone)]
pub struct EmbeddingClient {
    http: Client,
    base_url: String,
    api_key: String,
}

impl EmbeddingClient {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Result<Self, reqwest::Error> {
        Ok(Self {
            http: Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(10 * 60))
                .build()?,
            base_url: base_url.into().trim_end_matches('/').into(),
            api_key: api_key.into(),
        })
    }

    pub async fn embed_queries(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        self.embed(
            inputs
                .iter()
                .map(|input| {
                    format!("Represent this sentence for searching relevant passages: {input}")
                })
                .collect(),
        )
        .await
    }

    pub async fn embed_passages(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        self.embed(inputs.to_vec()).await
    }

    async fn embed(&self, inputs: Vec<String>) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        let request = EmbeddingRequest {
            model: "BGE Small".into(),
            input: inputs
                .iter()
                .map(|input| bounded_embedding_input(input, MAX_EMBEDDING_INPUT_BYTES))
                .collect(),
            encoding_format: "float",
        };
        let response = self
            .http
            .post(format!("{}/v1/embeddings", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&request)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(EmbeddingError::Status {
                status: response.status(),
                body: response.text().await.unwrap_or_default(),
            });
        }
        let mut data = response.json::<EmbeddingResponse>().await?.data;
        data.sort_by_key(|item| item.index);
        Ok(data.into_iter().map(|item| item.embedding).collect())
    }
}

#[derive(Debug, Serialize)]
struct EmbeddingRequest {
    model: String,
    input: Vec<String>,
    encoding_format: &'static str,
}
#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingItem>,
}
#[derive(Debug, Deserialize)]
struct EmbeddingItem {
    index: usize,
    embedding: Vec<f32>,
}

#[derive(Debug, thiserror::Error)]
pub enum EmbeddingError {
    #[error(transparent)]
    Request(#[from] reqwest::Error),
    #[error("embedding server returned {status}: {body}")]
    Status {
        status: reqwest::StatusCode,
        body: String,
    },
}

#[cfg(test)]
mod tests {
    use super::{bounded_embedding_input, MAX_EMBEDDING_INPUT_BYTES};

    #[test]
    fn bounds_ascii_complete_input() {
        let bounded = bounded_embedding_input(&"x".repeat(1_000), MAX_EMBEDDING_INPUT_BYTES);
        assert_eq!(bounded.len(), MAX_EMBEDDING_INPUT_BYTES);
    }

    #[test]
    fn bounds_utf8_without_splitting_a_character() {
        let bounded = bounded_embedding_input(&"🦀".repeat(200), MAX_EMBEDDING_INPUT_BYTES - 1);
        assert!(bounded.len() < MAX_EMBEDDING_INPUT_BYTES);
        assert!(bounded.is_char_boundary(bounded.len()));
        assert_eq!(bounded.chars().count(), 99);
    }

    #[test]
    fn preserves_short_input() {
        assert_eq!(bounded_embedding_input("Python list", 400), "Python list");
    }
}
