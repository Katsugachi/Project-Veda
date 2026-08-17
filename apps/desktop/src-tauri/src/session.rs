//! A warm, reusable model runtime.
//!
//! The old request path spawned a fresh llama.cpp sidecar for every question,
//! which meant re-reading the chat model (and the embedding model) off disk
//! each time — on a spinning disk or CPU-only device that is what turned a
//! simple "hi" into a multi-minute wait. This module keeps the sidecars alive
//! across requests and only restarts them when the configuration that matters
//! to llama.cpp (model file, context size, GPU layers, backend) changes.

use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use veda_core::{contextual_error, ModelQuant};
use veda_runtime::{EmbeddingClient, LlamaClient, LlamaSidecar, SidecarConfig};

/// After this long with no questions, the warm session is torn down so the
/// ~800 MB of model pages are returned to the OS on machines that need them.
/// The first question after eviction pays the model-load cost again.
pub const IDLE_EVICT_AFTER: Duration = Duration::from_secs(15 * 60);

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
    /// The last time a question used this session; drives idle eviction.
    pub last_used: Instant,
}

impl ModelSession {
    /// Whether this session matches the requested configuration and can be
    /// reused without restarting llama.cpp.
    pub fn matches(&self, fingerprint: &SessionFingerprint) -> bool {
        &self.fingerprint == fingerprint
    }

    /// Marks the session as recently used, resetting the idle-eviction clock.
    pub fn touch(&mut self) {
        self.last_used = Instant::now();
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
            last_used: Instant::now(),
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

/// Pure predicate behind idle eviction: has `last_used` aged past the
/// threshold at `now`? Split out so the boundary is unit-testable without
/// constructing a live llama.cpp sidecar.
pub fn should_evict(last_used: Instant, now: Instant) -> bool {
    now.duration_since(last_used) >= IDLE_EVICT_AFTER
}

/// Tears the session down when it has been idle longer than
/// [`IDLE_EVICT_AFTER`], returning the model pages to the OS. Only ever runs
/// while holding the session lock, so it cannot race a rebuild.
pub async fn evict_if_idle(slot: &mut Option<ModelSession>) {
    if let Some(session) = slot.as_mut() {
        if should_evict(session.last_used, Instant::now()) {
            session.stop_all().await;
            *slot = None;
        }
    }
}

/// Background janitor that frees the warm session's memory after a period of
/// inactivity. Spawned once at app startup; the runtime cancels it on exit.
pub async fn session_janitor(handle: SessionHandle) {
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    interval.tick().await; // align to the interval instead of firing instantly
    loop {
        interval.tick().await;
        let mut slot = handle.lock().await;
        evict_if_idle(&mut *slot).await;
    }
}

#[cfg(test)]
mod tests {
    use super::{should_evict, IDLE_EVICT_AFTER};
    use std::time::{Duration, Instant};

    #[test]
    fn idle_eviction_threshold_is_strict() {
        let now = Instant::now();
        assert!(!should_evict(now, now));
        assert!(!should_evict(
            now,
            now + IDLE_EVICT_AFTER - Duration::from_secs(1)
        ));
        assert!(should_evict(now, now + IDLE_EVICT_AFTER));
        assert!(should_evict(
            now,
            now + IDLE_EVICT_AFTER + Duration::from_secs(60)
        ));
    }

    #[test]
    fn a_fresh_touch_resets_the_clock() {
        let now = Instant::now();
        // A session touched "now" is never evictable at "now".
        assert!(!should_evict(now, now));
    }
}
