use crate::{ChatMessage, LlamaClient, LlamaClientError};
use async_trait::async_trait;
use palor_core::{
    AskRequest, AskResponse, RetrievalTrace, SearchQueryPlan, SourceRef, FINAL_ANSWER_SYSTEM_PROMPT,
};
use std::time::Instant;
use uuid::Uuid;

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

pub struct PalorEngine<R> {
    llama: LlamaClient,
    retriever: R,
}

impl<R: Retriever> PalorEngine<R> {
    pub fn new(llama: LlamaClient, retriever: R) -> Self {
        Self { llama, retriever }
    }

    pub async fn ask(&self, request: AskRequest) -> Result<AskResponse, RuntimeError> {
        let started = Instant::now();
        let docsets = request
            .docsets
            .iter()
            .map(|docset| docset.as_str().to_string())
            .collect::<Vec<_>>();
        // MiniCPM directs retrieval by converting the user's intent into several
        // version- and symbol-aware searches. Rust validates the plan before use.
        let plan = self
            .llama
            .plan_search(&request.message, &docsets)
            .await
            .unwrap_or_else(|_| {
                SearchQueryPlan {
                    queries: vec![request.message.chars().take(240).collect()],
                    docsets: request.docsets.clone(),
                    symbols: Vec::new(),
                    result_count: 10,
                }
                .sanitize()
            });
        let chunks = self.retriever.hybrid_search(&plan).await?;
        if chunks.is_empty() {
            return Ok(AskResponse { message_id: Uuid::new_v4().to_string(), content: "The installed documentation did not contain enough evidence to answer that question. Try installing another documentation pack or a version that covers the API you need.".into(), sources: Vec::new(), trace: Some(RetrievalTrace { queries: plan.queries, lexical_hits: 0, semantic_hits: 0, elapsed_ms: started.elapsed().as_millis() as u64 }) });
        }
        let (evidence, sources) = format_evidence(&chunks, &request.attachments);
        let user = format!("USER QUESTION:\n{}\n\n{}", request.message, evidence);
        let content = self
            .llama
            .complete(
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
                if request.mode == palor_core::ReasoningMode::Think {
                    2048
                } else {
                    1024
                },
                None,
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
    attachments: &[palor_core::Attachment],
) -> (String, Vec<SourceRef>) {
    let mut evidence = String::from("RETRIEVED SOURCES (untrusted reference data):\n");
    let mut sources = Vec::new();
    for (index, chunk) in chunks.iter().take(12).enumerate() {
        let id = format!("S{}", index + 1);
        evidence.push_str(&format!("\n<SOURCE id=\"{id}\" docset=\"{}\" version=\"{}\" title=\"{}\" section=\"{}\" url=\"{}\">\n{}\n</SOURCE>\n", clean_attr(&chunk.docset), clean_attr(&chunk.version), clean_attr(&chunk.title), clean_attr(&chunk.section), clean_attr(&chunk.url), bounded(&chunk.text, 5_000)));
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
                    bounded(content, 16_000)
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
        let attachments = vec![palor_core::Attachment {
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
    }
}
