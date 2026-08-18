use crate::ModelQuant;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Official language packs plus user-imported libraries.
///
/// User libraries serialize as `"local"` (and any unknown / `local-*` key
/// deserializes as [`DocsetId::Local`]) so adding a custom pack never breaks
/// the existing on-disk index format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DocsetId {
    Python,
    Cpp,
    Html,
    Css,
    Javascript,
    Local,
}

impl DocsetId {
    /// Catalogued language packs. User-imported libraries are discovered from
    /// disk (`local-*.json.zst`) rather than listed here.
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
            Self::Local => "local",
        }
    }

    pub const fn is_official(self) -> bool {
        !matches!(self, Self::Local)
    }

    /// Parses a catalog or UI id. Official names map 1:1; anything else
    /// (`local`, `local-notes`, a user-typed folder slug) is a local library.
    pub fn from_key(value: &str) -> Self {
        match value {
            "python" => Self::Python,
            "cpp" => Self::Cpp,
            "html" => Self::Html,
            "css" => Self::Css,
            "javascript" => Self::Javascript,
            _ => Self::Local,
        }
    }

    pub fn parse_official(value: &str) -> Option<Self> {
        match value {
            "python" => Some(Self::Python),
            "cpp" => Some(Self::Cpp),
            "html" => Some(Self::Html),
            "css" => Some(Self::Css),
            "javascript" => Some(Self::Javascript),
            _ => None,
        }
    }
}

impl Serialize for DocsetId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for DocsetId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Ok(Self::from_key(&value))
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
    /// Requested context window. `None` or `Some(0)` means "choose
    /// automatically from this machine's memory".
    #[serde(default)]
    pub context_tokens: Option<u32>,
    /// The model quantization the UI is configured for. `None` falls back to
    /// the first installed model file.
    #[serde(default)]
    pub model_quant: Option<ModelQuant>,
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
    fn local_and_unknown_keys_deserialize_as_local() {
        assert_eq!(DocsetId::from_key("python"), DocsetId::Python);
        assert_eq!(DocsetId::from_key("local"), DocsetId::Local);
        assert_eq!(DocsetId::from_key("local-notes"), DocsetId::Local);
        assert_eq!(DocsetId::parse_official("html"), Some(DocsetId::Html));
        assert_eq!(DocsetId::parse_official("local-notes"), None);
        let json = serde_json::to_string(&DocsetId::Local).unwrap();
        assert_eq!(json, "\"local\"");
        let back: DocsetId = serde_json::from_str("\"local-my-folder\"").unwrap();
        assert_eq!(back, DocsetId::Local);
    }

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
