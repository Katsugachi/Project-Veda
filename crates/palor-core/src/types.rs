use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DocsetId {
    Python,
    Cpp,
    Html,
    Css,
    Javascript,
}

impl DocsetId {
    pub const ALL: [Self; 5] = [
        Self::Python,
        Self::Cpp,
        Self::Html,
        Self::Css,
        Self::Javascript,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Python => "python",
            Self::Cpp => "cpp",
            Self::Html => "html",
            Self::Css => "css",
            Self::Javascript => "javascript",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningMode {
    Fast,
    Think,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub id: String,
    pub name: String,
    pub language: String,
    pub bytes: u64,
    #[serde(default)]
    pub content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskRequest {
    pub chat_id: String,
    pub message: String,
    pub mode: ReasoningMode,
    #[serde(default)]
    pub docsets: Vec<DocsetId>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRef {
    pub id: String,
    pub docset: String,
    pub title: String,
    pub section: String,
    pub url: String,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalTrace {
    pub queries: Vec<String>,
    pub lexical_hits: usize,
    pub semantic_hits: usize,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskResponse {
    pub message_id: String,
    pub content: String,
    pub sources: Vec<SourceRef>,
    pub trace: Option<RetrievalTrace>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SearchQueryPlan {
    pub queries: Vec<String>,
    #[serde(default)]
    pub docsets: Vec<DocsetId>,
    #[serde(default)]
    pub symbols: Vec<String>,
    #[serde(default = "default_result_count")]
    pub result_count: usize,
}

const fn default_result_count() -> usize {
    10
}

impl SearchQueryPlan {
    pub fn sanitize(mut self) -> Self {
        self.queries = self
            .queries
            .into_iter()
            .map(|query| query.trim().chars().take(240).collect::<String>())
            .filter(|query| !query.is_empty())
            .take(4)
            .collect();
        self.symbols = self
            .symbols
            .into_iter()
            .map(|symbol| symbol.trim().chars().take(100).collect::<String>())
            .filter(|symbol| !symbol.is_empty())
            .take(12)
            .collect();
        self.docsets.sort_by_key(|docset| docset.as_str());
        self.docsets.dedup();
        self.result_count = self.result_count.clamp(4, 16);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planner_is_bounded() {
        let plan = SearchQueryPlan {
            queries: vec![
                " one ".into(),
                "two".into(),
                "three".into(),
                "four".into(),
                "five".into(),
            ],
            docsets: vec![DocsetId::Python, DocsetId::Python],
            symbols: vec![" asyncio.TaskGroup ".into()],
            result_count: 500,
        }
        .sanitize();
        assert_eq!(plan.queries.len(), 4);
        assert_eq!(plan.docsets, vec![DocsetId::Python]);
        assert_eq!(plan.symbols, vec!["asyncio.TaskGroup"]);
        assert_eq!(plan.result_count, 16);
    }
}
