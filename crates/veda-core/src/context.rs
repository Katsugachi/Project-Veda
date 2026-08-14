use crate::ModelQuant;
use serde::{Deserialize, Serialize};

const GIB: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextBudget {
    pub context_tokens: u32,
    pub estimated_model_bytes: u64,
    pub estimated_kv_bytes: u64,
    pub estimated_total_bytes: u64,
}

/// The largest context the UI may request.
pub const CONTEXT_TOKENS_MAX: u32 = 131_072;
/// The smallest context that is still useful for a sourced answer.
pub const CONTEXT_TOKENS_MIN: u32 = 512;

/// Resolves a user-requested context against what this machine can support.
///
/// `requested` of `None` or `Some(0)` means "automatic", which falls back to
/// the conservative memory-derived budget. An explicit request is honoured but
/// clamped to the supported range so a hand-typed value can never ask
/// llama.cpp for an impossible allocation.
pub fn resolve_context_tokens(
    requested: Option<u32>,
    total_memory_bytes: u64,
    quant: ModelQuant,
) -> u32 {
    match requested {
        None | Some(0) => context_budget(total_memory_bytes, quant).context_tokens,
        Some(value) => value.clamp(CONTEXT_TOKENS_MIN, CONTEXT_TOKENS_MAX),
    }
}

/// Conservative MiniCPM5-1B memory budget. The KV estimate assumes F16 K/V,
/// 24 layers, 2 KV heads and a 64-element head dimension, plus 25% overhead.
pub fn context_budget(total_memory_bytes: u64, quant: ModelQuant) -> ContextBudget {
    let model_bytes = match quant {
        ModelQuant::Q5 => 1_300_000_000,
        ModelQuant::Q8 => 1_750_000_000,
    };
    let reserved = 2 * GIB;
    let usable = total_memory_bytes.saturating_sub(reserved + model_bytes);
    let context_tokens = match usable {
        value if value >= 18 * GIB => 32_768,
        value if value >= 8 * GIB => 16_384,
        value if value >= 3 * GIB => 8_192,
        _ => 4_096,
    };
    let kv_per_token = 24_u64 * 2 * 2 * 64 * 2;
    let estimated_kv_bytes = kv_per_token * u64::from(context_tokens) * 5 / 4;
    ContextBudget {
        context_tokens,
        estimated_model_bytes: model_bytes,
        estimated_kv_bytes,
        estimated_total_bytes: model_bytes + estimated_kv_bytes + 512 * 1024 * 1024,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_context_uses_the_memory_budget() {
        assert_eq!(
            resolve_context_tokens(None, 16 * GIB, ModelQuant::Q8),
            context_budget(16 * GIB, ModelQuant::Q8).context_tokens
        );
        assert_eq!(
            resolve_context_tokens(Some(0), 16 * GIB, ModelQuant::Q8),
            context_budget(16 * GIB, ModelQuant::Q8).context_tokens
        );
    }

    #[test]
    fn explicit_context_is_honoured_and_clamped() {
        assert_eq!(
            resolve_context_tokens(Some(65_536), 16 * GIB, ModelQuant::Q8),
            65_536
        );
        assert_eq!(
            resolve_context_tokens(Some(CONTEXT_TOKENS_MAX), 8 * GIB, ModelQuant::Q5),
            CONTEXT_TOKENS_MAX
        );
        // Above the ceiling and below the floor are both brought into range.
        assert_eq!(
            resolve_context_tokens(Some(999_999), 8 * GIB, ModelQuant::Q5),
            CONTEXT_TOKENS_MAX
        );
        assert_eq!(
            resolve_context_tokens(Some(1), 8 * GIB, ModelQuant::Q5),
            CONTEXT_TOKENS_MIN
        );
    }

    #[test]
    fn context_grows_with_ram() {
        assert_eq!(
            context_budget(8 * GIB, ModelQuant::Q5).context_tokens,
            8_192
        );
        assert_eq!(
            context_budget(16 * GIB, ModelQuant::Q8).context_tokens,
            16_384
        );
        assert_eq!(
            context_budget(32 * GIB, ModelQuant::Q8).context_tokens,
            32_768
        );
    }
}
