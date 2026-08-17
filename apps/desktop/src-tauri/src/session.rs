//! A warm, reusable model runtime.
//!
//! The old request path spawned a fresh llama.cpp sidecar for every question,
//! which meant re-reading the chat model (and the embedding model) off disk
//! each time — on a spinning disk or CPU-only device that is what turned a
//! simple "hi" into a multi-minute wait. This module keeps the sidecars alive
//! across requests and only restarts them when the configuration that matters
//! to llama.cpp (model file, context size, GPU layers, backend) changes.

use std::{path::Path, sync::Arc};
use tokio::sync::Mutex;
use veda_core::{contextual_error, ModelQuant};
use veda_runtime::{EmbeddingClient, LlamaClient, LlamaSidecar, SidecarConfig};

/// The identity of a running session. When any field changes the session is
/// torn down and rebuilt, because llama.cpp cannot resize its context or swap
/// models in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFingerprint {
    pub model_path: std::path::PathBuf,
    pub context_tokens: u32,
    pub gpu_layers: i32,
    pub backend: String,
    pub quant: ModelQuant,
}

pub struct EmbeddingSession {
    pub sidecar: LlamaSidecar,
    pub client: EmbeddingClient,
}

pub struct ModelSession {
    pub chat: LlamaSidecar,
    pub llama: LlamaClient,
    /// Lazily created on the first retrieval-backed ask. Greetings never need
    /// it, so the fast path stays cheap.
    pub embedding: Option<EmbeddingSession>,
    pub fingerprint: SessionFingerprint,
}

impl ModelSession {
    /// Whether this session matches the requested configuration and can be
    /// reused without restarting llama.cpp.
    pub fn matches(&self, fingerprint: &SessionFingerprint) -> bool {
        &self.fingerprint == fingerprint
    }

    /// Spawns the chat-model sidecar and connects a client to it.
    pub async fn spawn_chat(
        executable: &Path,
        fingerprint: &SessionFingerprint,
        threads: usize,
    ) -> Result<Self, String> {
        let chat = LlamaSidecar::spawn(SidecarConfig {
            executable: executable.to_owned(),
            model: fingerprint.model_path.clone(),
            context_tokens: fingerprint.context_tokens,
            gpu_layers: fingerprint.gpu_layers,
            embedding: false,
            pooling: None,
            threads,
        })
        .await
        .map_err(|error| contextual_error("Could not start the llama.cpp runtime", &error))?;
        let llama = LlamaClient::new(&chat.base_url, &chat.api_key)
            .map_err(|error| contextual_error("Could not reach the local model", &error))?;
        Ok(Self {
            chat,
            llama,
            embedding: None,
            fingerprint: fingerprint.clone(),
        })
    }

    /// Ensures the embedding sidecar exists and returns its client. Spawned at
    /// most once per session; retrieval asks reuse it.
    pub async fn ensure_embedding(
        &mut self,
        executable: &Path,
        embedding_model: &Path,
        threads: usize,
    ) -> Result<&EmbeddingClient, String> {
        if self.embedding.is_none() {
            let sidecar = LlamaSidecar::spawn(SidecarConfig {
                executable: executable.to_owned(),
                model: embedding_model.to_owned(),
                context_tokens: 512,
                gpu_layers: self.fingerprint.gpu_layers,
                embedding: true,
                pooling: Some("cls".into()),
                threads,
            })
            .await
            .map_err(|error| contextual_error("Could not start local search", &error))?;
            let client =
                EmbeddingClient::new(&sidecar.base_url, &sidecar.api_key).map_err(|error| {
                    contextual_error("Could not reach the local search runtime", &error)
                })?;
            self.embedding = Some(EmbeddingSession { sidecar, client });
        }
        Ok(&self
            .embedding
            .as_ref()
            .expect("embedding session was just created")
            .client)
    }

    /// Stops every sidecar held by this session. Called when the configuration
    /// changes and the session must be rebuilt.
    pub async fn stop_all(&mut self) {
        if let Some(embedding) = &mut self.embedding {
            let _ = embedding.sidecar.stop().await;
        }
        let _ = self.chat.stop().await;
    }
}

/// Shared across `ask_veda` invocations. Serializing requests through one
/// mutex is intentional: a single local model can only generate one reply at a
/// time anyway, and it prevents two chats from racing to spawn sidecars.
pub type SessionHandle = Arc<Mutex<Option<ModelSession>>>;
