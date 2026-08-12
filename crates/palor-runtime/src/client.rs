use palor_core::{ReasoningMode, SearchQueryPlan, SEARCH_PLANNER_SYSTEM_PROMPT};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

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
        let thinking = mode == ReasoningMode::Think;
        let mut body = json!({
            "model": self.model,
            "messages": messages,
            "stream": false,
            "max_tokens": max_tokens,
            "temperature": if thinking { 0.9 } else { 0.7 },
            "top_p": 0.95,
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
        let response: CompletionResponse = response.json().await?;
        response
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message.content)
            .ok_or(LlamaClientError::EmptyCompletion)
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
    fn braces_inside_strings_do_not_end_object() {
        let value = r#"prefix {"queries":["what does {x} mean"]} suffix"#;
        assert_eq!(
            extract_json_object(value),
            Some(r#"{"queries":["what does {x} mean"]}"#)
        );
    }
}
