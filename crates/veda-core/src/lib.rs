pub mod catalog;
pub mod context;
pub mod conversation;
pub mod errors;
pub mod preflight;
pub mod prompt;
pub mod types;

pub use catalog::{default_catalog, Asset, AssetKind, Catalog, ModelQuant};
pub use context::{
    context_budget, resolve_context_tokens, ContextBudget, CONTEXT_TOKENS_MAX, CONTEXT_TOKENS_MIN,
};
pub use conversation::is_conversational;
pub use errors::{contextual_error, friendly_error};
pub use preflight::{evaluate_preflight, HardwareSnapshot, PreflightReport};
pub use prompt::{
    CONVERSATIONAL_SYSTEM_PROMPT, FINAL_ANSWER_SYSTEM_PROMPT, SEARCH_PLANNER_SYSTEM_PROMPT,
};
pub use types::*;
