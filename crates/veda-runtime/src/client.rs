use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use veda_core::{ReasoningMode, SearchQueryPlan, SEARCH_PLANNER_SYSTEM_PROMPT};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct LlamaClient {
    http: Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl LlamaClient {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Result<Self, LlamaClientError> {
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10 * 60))
            .build()?;
        Ok(Self {
            http,
            base_url: base_url.into().trim_end_matches('/').into(),
            api_key: api_key.into(),
            model: "MiniCPM 5".into(),
        })
    }

    pub async fn plan_search(
        &self,
        user_request: &str,
        available_docsets: &[String],
    ) -> Result<SearchQueryPlan, LlamaClientError> {
        let user = format!(
            "AVAILABLE DOCSETS: {}\n\nUSER REQUEST:\n{}",
            available_docsets.join(", "),
            user_request
        );
        let content = self
            .complete(
                vec![
                    ChatMessage {
                        role: "system".into(),
                        content: SEARCH_PLANNER_SYSTEM_PROMPT.into(),
                    },
                    ChatMessage {
                        role: "user".into(),
                        content: user,
                    },
                ],
                ReasoningMode::Fast,
                420,
                Some(json!({"type": "json_object"})),
            )
            .await?;
        let object = extract_json_object(&content)
            .ok_or_else(|| LlamaClientError::InvalidPlanner(content.clone()))?;
        let mut plan: SearchQueryPlan = serde_json::from_str(object)
            .map_err(|source| LlamaClientError::PlannerJson { source, content })?;
        if plan.queries.is_empty() {
            plan.queries.push(user_request.chars().take(240).collect());
        }
        Ok(plan.sanitize())
    }

    pub async fn complete(
        &self,
        messages: Vec<ChatMessage>,
        mode: ReasoningMode,
        max_tokens: u32,
        response_format: Option<Value>,
    ) -> Result<String, LlamaClientError> {
        self.complete_with_sink(messages, mode, max_tokens, response_format, None)
            .await
    }

    /// Same as [`complete`], but when `on_token` is set the request is streamed
    /// so the UI can paint tokens as they arrive instead of waiting for EOS.
    pub async fn complete_with_sink(
        &self,
        messages: Vec<ChatMessage>,
        mode: ReasoningMode,
        max_tokens: u32,
        response_format: Option<Value>,
        on_token: Option<&mut (dyn FnMut(&str) + Send)>,
    ) -> Result<String, LlamaClientError> {
        let thinking = mode == ReasoningMode::Think;
        let stream = on_token.is_some() && response_format.is_none();
        let mut body = json!({
            "model": self.model,
            "messages": messages,
            "stream": stream,
            "max_tokens": max_tokens,
            "temperature": if thinking { 0.6 } else { 0.4 },
            "top_p": 0.9,
            "chat_template_kwargs": { "enable_thinking": thinking }
        });
        if let Some(format) = response_format {
            body["response_format"] = format;
        }
        let response = self
            .http
            .post(format!("{}/v1/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(LlamaClientError::Status {
                status: response.status(),
                body: response.text().await.unwrap_or_default(),
            });
        }
        if !stream {
            let response: CompletionResponse = response.json().await?;
            return response
                .choices
                .into_iter()
                .next()
                .map(|choice| choice.message.content)
                .ok_or(LlamaClientError::EmptyCompletion);
        }
        // Pass the Option<&mut dyn …> through. as_deref_mut() reborrows the
        // local and that reborrow is held across .await (E0597).
        read_sse_completion(response, on_token).await
    }

    pub async fn health(&self) -> Result<(), LlamaClientError> {
        let response = self
            .http
            .get(format!("{}/health", self.base_url))
            .bearer_auth(&self.api_key)
            .send()
            .await?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(LlamaClientError::Status {
                status: response.status(),
                body: response.text().await.unwrap_or_default(),
            })
        }
    }
}

#[derive(Debug, Deserialize)]
struct CompletionResponse {
    choices: Vec<Choice>,
}
#[derive(Debug, Deserialize)]
struct Choice {
    message: AssistantMessage,
}
#[derive(Debug, Deserialize)]
struct AssistantMessage {
    content: String,
}

/// Applies one SSE `data:` line to the assembled completion. Extracted so the
/// last-line flush and the unit tests share the exact parser — a missing
/// function here is what made `veda-runtime` tests fail to compile, and
/// inlining it hid the leftover-buffer path.
fn append_sse_delta(
    line: &str,
    assembled: &mut String,
    mut on_token: Option<&mut (dyn FnMut(&str) + Send)>,
) {
    let Some(payload) = line.strip_prefix("data:") else {
        return;
    };
    let payload = payload.trim();
    if payload.is_empty() || payload == "[DONE]" {
        return;
    }
    let Ok(value) = serde_json::from_str::<Value>(payload) else {
        return;
    };
    let Some(delta) = value
        .pointer("/choices/0/delta/content")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
    else {
        return;
    };
    assembled.push_str(delta);
    if let Some(sink) = on_token.as_mut() {
        sink(delta);
    }
}

/// Splits `pending` on newlines into SSE events. When `flush_tail` is set
/// (end of the HTTP body) the leftover line is consumed even without a
/// trailing newline — llama.cpp often ends the stream that way, and dropping
/// it lost the last tokens or the whole answer when the body was one chunk.
fn drain_sse_buffer(
    pending: &mut String,
    assembled: &mut String,
    on_token: &mut Option<&mut (dyn FnMut(&str) + Send)>,
    flush_tail: bool,
) {
    while let Some(split) = pending.find('\n') {
        let line = pending[..split].trim_end_matches('\r').to_string();
        pending.replace_range(..=split, "");
        append_sse_delta(&line, assembled, on_token.as_deref_mut());
    }
    if flush_tail && !pending.trim().is_empty() {
        let line = std::mem::take(pending);
        append_sse_delta(
            line.trim_end_matches('\r'),
            assembled,
            on_token.as_deref_mut(),
        );
    }
}

async fn read_sse_completion(
    mut response: reqwest::Response,
    mut on_token: Option<&mut (dyn FnMut(&str) + Send)>,
) -> Result<String, LlamaClientError> {
    let mut assembled = String::new();
    let mut pending = String::new();
    while let Some(chunk) = response.chunk().await? {
        pending.push_str(&String::from_utf8_lossy(&chunk));
        drain_sse_buffer(&mut pending, &mut assembled, &mut on_token, false);
    }
    drain_sse_buffer(&mut pending, &mut assembled, &mut on_token, true);
    if assembled.is_empty() {
        return Err(LlamaClientError::EmptyCompletion);
    }
    Ok(assembled)
}

pub fn extract_json_object(content: &str) -> Option<&str> {
    let without_thinking = content.rsplit("</think>").next().unwrap_or(content);
    let start = without_thinking.find('{')?;
    let mut depth = 0_u32;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, character) in without_thinking[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(&without_thinking[start..start + offset + character.len_utf8()]);
                }
            }
            _ => {}
        }
    }
    None
}

#[derive(Debug, thiserror::Error)]
pub enum LlamaClientError {
    #[error(transparent)]
    Request(#[from] reqwest::Error),
    #[error("llama.cpp returned {status}: {body}")]
    Status {
        status: reqwest::StatusCode,
        body: String,
    },
    #[error("llama.cpp returned an empty completion")]
    EmptyCompletion,
    #[error("search planner did not return a JSON object: {0}")]
    InvalidPlanner(String),
    #[error("search planner returned invalid JSON: {source}; content: {content}")]
    PlannerJson {
        source: serde_json::Error,
        content: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_json_after_private_thinking() {
        let value = "<think>private</think>\n```json\n{\"queries\":[\"TaskGroup\"]}\n```";
        assert_eq!(
            extract_json_object(value),
            Some("{\"queries\":[\"TaskGroup\"]}")
        );
    }

    #[test]
    fn sse_delta_appends_content_and_ignores_done() {
        let mut assembled = String::new();
        let mut seen = String::new();
        append_sse_delta(
            r#"data: {"choices":[{"delta":{"content":"Hel"}}]}"#,
            &mut assembled,
            Some(&mut |piece| seen.push_str(piece)),
        );
        append_sse_delta(
            "data: [DONE]",
            &mut assembled,
            Some(&mut |piece| seen.push_str(piece)),
        );
        append_sse_delta(
            r#"data: {"choices":[{"delta":{"content":"lo"}}]}"#,
            &mut assembled,
            Some(&mut |piece| seen.push_str(piece)),
        );
        assert_eq!(assembled, "Hello");
        assert_eq!(seen, "Hello");
    }

    #[test]
    fn sse_flushes_the_last_line_without_a_newline() {
        // One complete event plus a tail with no trailing `\n`. Without the
        // end-of-body flush the second token is dropped and a single-chunk
        // reply becomes EmptyCompletion.
        let mut pending = String::from(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\ndata: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}",
        );
        let mut assembled = String::new();
        let mut sink: Option<&mut (dyn FnMut(&str) + Send)> = None;
        drain_sse_buffer(&mut pending, &mut assembled, &mut sink, false);
        assert_eq!(assembled, "Hel");
        drain_sse_buffer(&mut pending, &mut assembled, &mut sink, true);
        assert_eq!(assembled, "Hello");
        assert!(pending.is_empty());
    }

    #[test]
    fn braces_inside_strings_do_not_end_object() {
        let value = r#"prefix {"queries":["what does {x} mean"]} suffix"#;
        assert_eq!(
            extract_json_object(value),
            Some(r#"{"queries":["what does {x} mean"]}"#)
        );
    }
}
