use crate::ModelQuant;
use serde::{Deserialize, Serialize};

/// Only the unit tests below need GiB-based inputs; the production code
/// works directly in bytes, so the constant is test-scoped to keep clippy's
/// `-D warnings` (dead_code) happy.
#[cfg(test)]
const GIB: u64 = 1024 * 1024 * 1024;

/// Safety margin kept free on top of whatever other applications are already
/// using: the model and its KV cache can never consume the machine's last
/// 1.4 GB, so the OS, the app and a busy desktop keep breathing room.
pub const CONTEXT_MEMORY_LEEWAY_BYTES: u64 = 1_400_000_000;

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
/// The smallest context that is still useful for a sourced answer, used as the
/// floor for an explicit hand-typed value.
pub const CONTEXT_TOKENS_MIN: u32 = 512;
/// The hard floor for the **automatic** ("0 / Auto") context. Regardless of how
/// little memory is free, Veda never asks llama.cpp for a sub-16K context,
/// because the RAG evidence plus a sourced answer routinely needs that much
/// room and a tiny context silently truncates the grounding material.
pub const CONTEXT_TOKENS_DEFAULT_MIN: u32 = 16_384;

/// Resolves a user-requested context against what this machine can support.
///
/// `requested` of `None` or `Some(0)` means "automatic", which sizes the
/// context to the memory that is actually available right now (total RAM
/// minus whatever other applications are using) with a 1.4 GB safety margin
/// reserved on top. An explicit request is honoured but clamped to the
/// supported range so a hand-typed value can never ask llama.cpp for an
/// impossible allocation.
pub fn resolve_context_tokens(
    requested: Option<u32>,
    available_memory_bytes: u64,
    quant: ModelQuant,
) -> u32 {
    match requested {
        None | Some(0) => context_budget(available_memory_bytes, quant).context_tokens,
        Some(value) => value.clamp(CONTEXT_TOKENS_MIN, CONTEXT_TOKENS_MAX),
    }
}

/// Sizes the automatic context to the memory that is actually free.
///
/// "Including existing used RAM": `available_memory_bytes` already excludes
/// what other processes are holding, so the budget can never over-commit a
/// machine that is already busy. The estimate assumes MiniCPM5-1B with F16
/// K/V, 24 layers, 2 KV heads and a 64-element head dimension, plus 25%
/// overhead. The largest whole number of tokens whose KV cache fits in the
/// RAM left after reserving the 1.4 GB leeway and the model itself is
/// chosen, rounded down to the UI's 1K step and capped at MiniCPM's
/// advertised ceiling. The result is never below
/// [`CONTEXT_TOKENS_DEFAULT_MIN`] (16K) regardless of how little RAM is
/// available: a sourced answer needs at least that much room, so a
/// memory-starved machine degrades to the floor instead of being handed an
/// unusably small context.
pub fn context_budget(available_memory_bytes: u64, quant: ModelQuant) -> ContextBudget {
    let model_bytes = match quant {
        ModelQuant::Q5 => 1_300_000_000,
        ModelQuant::Q8 => 1_750_000_000,
    };
    let headroom = available_memory_bytes.saturating_sub(CONTEXT_MEMORY_LEEWAY_BYTES);
    let usable_for_kv = headroom.saturating_sub(model_bytes);
    let kv_per_token = 24_u64 * 2 * 2 * 64 * 2;
    let kv_per_token_with_overhead = kv_per_token * 5 / 4;
    let raw_tokens = usable_for_kv / kv_per_token_with_overhead;
    let stepped = raw_tokens / 1024 * 1024;
    let context_tokens = stepped.clamp(
        u64::from(CONTEXT_TOKENS_DEFAULT_MIN),
        u64::from(CONTEXT_TOKENS_MAX),
    ) as u32;
    let estimated_kv_bytes = kv_per_token_with_overhead * u64::from(context_tokens);
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
    fn automatic_context_uses_the_available_memory_budget() {
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
    fn automatic_context_fills_available_ram_with_leeway() {
        // 4 GiB available with Q8: 1.4 GB leeway + 1.75 GB model leave
        // 1.07 GiB for the KV cache, which is 74,542 raw tokens rounded down
        // to the 1K step (73,728).
        assert_eq!(
            context_budget(4 * GIB, ModelQuant::Q8).context_tokens,
            73_728
        );
        // A machine with less free memory gets a smaller context…
        assert_eq!(
            context_budget(3 * GIB, ModelQuant::Q5).context_tokens,
            33_792
        );
        // …and once RAM is plentiful the ceiling is the model's 131K max.
        assert_eq!(
            context_budget(16 * GIB, ModelQuant::Q5).context_tokens,
            CONTEXT_TOKENS_MAX
        );
        // The automatic context never drops below the 16K floor, no matter how
        // little memory is available — it degrades to the floor rather than an
        // unusably tiny window.
        assert_eq!(
            context_budget(2 * GIB, ModelQuant::Q8).context_tokens,
            CONTEXT_TOKENS_DEFAULT_MIN
        );
    }

    #[test]
    fn automatic_context_has_a_16k_floor_regardless_of_ram() {
        // Zero, near-zero and "no headroom after leeway + model" all clamp up
        // to the 16K default floor rather than collapsing to a useless context.
        assert_eq!(context_budget(0, ModelQuant::Q5).context_tokens, 16_384);
        assert_eq!(context_budget(1, ModelQuant::Q5).context_tokens, 16_384);
        assert_eq!(
            context_budget(2 * GIB, ModelQuant::Q8).context_tokens,
            16_384
        );
        // resolve_context_tokens honours the same floor on the automatic path.
        assert_eq!(
            resolve_context_tokens(None, 0, ModelQuant::Q5),
            CONTEXT_TOKENS_DEFAULT_MIN
        );
        assert_eq!(
            resolve_context_tokens(Some(0), 0, ModelQuant::Q8),
            CONTEXT_TOKENS_DEFAULT_MIN
        );
        // An explicit hand-typed value below 16K is still honoured (clamped to
        // the per-entry minimum of 512) — only the automatic path is floored.
        assert_eq!(
            resolve_context_tokens(Some(2_048), 0, ModelQuant::Q5),
            2_048
        );
    }

    #[test]
    fn context_grows_with_available_ram() {
        // Skip the sub-floor region: start above the 16K default minimum so
        // the comparison reflects the budgeted growth, not the floor clamp.
        let small = context_budget(3 * GIB, ModelQuant::Q5).context_tokens;
        let medium = context_budget(4 * GIB, ModelQuant::Q5).context_tokens;
        let large = context_budget(5 * GIB, ModelQuant::Q5).context_tokens;
        assert!(small < medium && medium < large);
        assert_eq!(large, CONTEXT_TOKENS_MAX);
    }
}
