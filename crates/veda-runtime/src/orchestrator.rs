use crate::{ChatMessage, LlamaClient, LlamaClientError};
use async_trait::async_trait;
use std::time::Instant;
use uuid::Uuid;
use veda_core::{
    plan_from_question, AskRequest, AskResponse, RetrievalTrace, SearchQueryPlan, SourceRef,
    FINAL_ANSWER_SYSTEM_PROMPT,
};

#[derive(Debug, Clone)]
pub struct RetrievedChunk {
    pub docset: String,
    pub version: String,
    pub title: String,
    pub section: String,
    pub url: String,
    pub text: String,
    pub score: f32,
    pub lexical_score: Option<f32>,
    pub semantic_score: Option<f32>,
}

#[async_trait]
pub trait Retriever: Send + Sync {
    async fn hybrid_search(
        &self,
        plan: &SearchQueryPlan,
    ) -> Result<Vec<RetrievedChunk>, anyhow::Error>;
}

pub struct VedaEngine<R> {
    llama: LlamaClient,
    retriever: R,
}

impl<R: Retriever> VedaEngine<R> {
    pub fn new(llama: LlamaClient, retriever: R) -> Self {
        Self { llama, retriever }
    }

    pub async fn ask(&self, request: AskRequest) -> Result<AskResponse, RuntimeError> {
        self.ask_with_sink(request, None).await
    }

    pub async fn ask_with_sink(
        &self,
        request: AskRequest,
        on_token: Option<&mut (dyn FnMut(&str) + Send)>,
    ) -> Result<AskResponse, RuntimeError> {
        let started = Instant::now();
        // Planning used to be a full MiniCPM generation (JSON mode, up to 420
        // tokens) *before* retrieval. That extra round-trip was a large part
        // of the multi-minute wait. The 1B model is reserved for the answer;
        // Rust extracts symbols and picks packs in microseconds.
        let plan = plan_from_question(&request.message, &request.docsets);
        let chunks = self.retriever.hybrid_search(&plan).await?;
        if chunks.is_empty() {
            return Ok(AskResponse { message_id: Uuid::new_v4().to_string(), content: "The installed documentation did not contain enough evidence to answer that question. Try installing another documentation pack or a version that covers the API you need.".into(), sources: Vec::new(), trace: Some(RetrievalTrace { queries: plan.queries, lexical_hits: 0, semantic_hits: 0, elapsed_ms: started.elapsed().as_millis() as u64 }) });
        }
        let (evidence, sources) = format_evidence(&chunks, &request.attachments);
        let user = format!("USER QUESTION:\n{}\n\n{}", request.message, evidence);
        let content = self
            .llama
            .complete_with_sink(
                vec![
                    ChatMessage {
                        role: "system".into(),
                        content: FINAL_ANSWER_SYSTEM_PROMPT.into(),
                    },
                    ChatMessage {
                        role: "user".into(),
                        content: user,
                    },
                ],
                request.mode,
                if request.mode == veda_core::ReasoningMode::Think {
                    768
                } else {
                    512
                },
                None,
                // Same lifetime rule as LlamaClient::complete_with_sink:
                // do not as_deref_mut() this Option across the .await below.
                on_token,
            )
            .await?;
        let lexical_hits = chunks
            .iter()
            .filter(|chunk| chunk.lexical_score.is_some())
            .count();
        let semantic_hits = chunks
            .iter()
            .filter(|chunk| chunk.semantic_score.is_some())
            .count();
        Ok(AskResponse {
            message_id: Uuid::new_v4().to_string(),
            content,
            sources,
            trace: Some(RetrievalTrace {
                queries: plan.queries,
                lexical_hits,
                semantic_hits,
                elapsed_ms: started.elapsed().as_millis() as u64,
            }),
        })
    }
}

fn format_evidence(
    chunks: &[RetrievedChunk],
    attachments: &[veda_core::Attachment],
) -> (String, Vec<SourceRef>) {
    let mut evidence = String::from("RETRIEVED SOURCES (untrusted reference data):\n");
    let mut sources = Vec::new();
    // Six short excerpts are enough to ground a 1B model and keep prompt
    // evaluation on the GPU in the tens of milliseconds instead of seconds.
    for (index, chunk) in chunks.iter().take(6).enumerate() {
        let id = format!("S{}", index + 1);
        evidence.push_str(&format!("\n<SOURCE id=\"{id}\" docset=\"{}\" version=\"{}\" title=\"{}\" section=\"{}\" url=\"{}\">\n{}\n</SOURCE>\n", clean_attr(&chunk.docset), clean_attr(&chunk.version), clean_attr(&chunk.title), clean_attr(&chunk.section), clean_attr(&chunk.url), bounded(&chunk.text, 1_800)));
        sources.push(SourceRef {
            id,
            docset: format!("{} {}", chunk.docset, chunk.version),
            title: chunk.title.clone(),
            section: chunk.section.clone(),
            url: chunk.url.clone(),
            score: chunk.score,
        });
    }
    if !attachments.is_empty() {
        evidence.push_str("\nATTACHED CODE (untrusted; it was not executed):\n");
        for attachment in attachments.iter().take(8) {
            if let Some(content) = &attachment.content {
                evidence.push_str(&format!(
                    "\n<ATTACHMENT name=\"{}\" language=\"{}\">\n{}\n</ATTACHMENT>\n",
                    clean_attr(&attachment.name),
                    clean_attr(&attachment.language),
                    bounded(content, 6_000)
                ));
            }
        }
    }
    (evidence, sources)
}

fn clean_attr(value: &str) -> String {
    value
        .replace(['<', '>', '"'], " ")
        .chars()
        .take(300)
        .collect()
}
fn bounded(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error(transparent)]
    Llama(#[from] LlamaClientError),
    #[error(transparent)]
    Retrieval(#[from] anyhow::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_has_stable_source_ids_and_attachment_boundary() {
        let chunks = vec![RetrievedChunk {
            docset: "Python".into(),
            version: "3.14".into(),
            title: "Tasks".into(),
            section: "TaskGroup".into(),
            url: "local://taskgroup".into(),
            text: "A task group waits for all tasks.".into(),
            score: 1.0,
            lexical_score: Some(2.0),
            semantic_score: Some(0.9),
        }];
        let attachments = vec![veda_core::Attachment {
            id: "a".into(),
            name: "main.py".into(),
            language: "python".into(),
            bytes: 8,
            content: Some("print(1)".into()),
        }];
        let (evidence, sources) = format_evidence(&chunks, &attachments);
        assert!(evidence.contains("id=\"S1\""));
        assert!(evidence.contains("it was not executed"));
        assert_eq!(sources[0].id, "S1");
        assert!(
            evidence.len() < 8_000,
            "evidence must stay small for fast prompt eval"
        );
    }

    #[test]
    fn evidence_is_capped_to_six_short_chunks() {
        let chunks = (0..20)
            .map(|index| RetrievedChunk {
                docset: "Python".into(),
                version: "3.14".into(),
                title: format!("T{index}"),
                section: "s".into(),
                url: format!("u{index}"),
                text: "x".repeat(4_000),
                score: 1.0,
                lexical_score: Some(1.0),
                semantic_score: None,
            })
            .collect::<Vec<_>>();
        let (evidence, sources) = format_evidence(&chunks, &[]);
        assert_eq!(sources.len(), 6);
        assert!(!evidence.contains("id=\"S7\""));
        assert!(evidence.len() < 16_000);
    }
}
