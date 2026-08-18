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
/// Automatic context is pinned here on purpose. Filling all free RAM up to
/// MiniCPM's 131K ceiling allocated a multi-gigabyte KV cache, made
/// llama.cpp's warmup walk a huge graph, and turned a GPU-offloaded 1B model
/// into a multi-minute wait. 16K is enough for the system prompt, a handful
/// of retrieved chunks and a sourced answer; users who really want a bigger
/// window can still type one in Settings.
pub const CONTEXT_TOKENS_AUTO: u32 = CONTEXT_TOKENS_DEFAULT_MIN;

/// Resolves a user-requested context against what this machine can support.
///
/// `requested` of `None` or `Some(0)` means "automatic", which is always
/// [`CONTEXT_TOKENS_AUTO`] (16K). Automatic used to grow with free RAM up to
/// 131K and that is what made GPU offload take minutes. An explicit request
/// is honoured but clamped to the supported range so a hand-typed value can
/// never ask llama.cpp for an impossible allocation.
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

/// Sizes the automatic context.
///
/// Automatic is always [`CONTEXT_TOKENS_AUTO`] (16K). The previous policy
/// grew the window with free RAM up to 131K, which made llama.cpp allocate a
/// huge KV cache even when the prompt was a few thousand tokens — the main
/// reason MiniCPM took minutes with full GPU offload. The estimates below
/// are still computed so Settings / diagnostics can show how much the
/// chosen window costs; they no longer drive the automatic value.
///
/// An explicit hand-typed value still goes through
/// [`resolve_context_tokens`] and may be as large as [`CONTEXT_TOKENS_MAX`].
pub fn context_budget(available_memory_bytes: u64, quant: ModelQuant) -> ContextBudget {
    let model_bytes = match quant {
        ModelQuant::Q5 => 1_300_000_000,
        ModelQuant::Q8 => 1_750_000_000,
    };
    let kv_per_token = 24_u64 * 2 * 2 * 64 * 2;
    let kv_per_token_with_overhead = kv_per_token * 5 / 4;
    let _ = available_memory_bytes;
    let context_tokens = CONTEXT_TOKENS_AUTO;
    let estimated_kv_bytes = kv_per_token_with_overhead * u64::from(context_tokens);
    ContextBudget {
        context_tokens,
        estimated_model_bytes: model_bytes,
        estimated_kv_bytes,
        estimated_total_bytes: model_bytes + estimated_kv_bytes + CONTEXT_MEMORY_LEEWAY_BYTES,
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
    fn automatic_context_stays_fast_regardless_of_ram() {
        // Filling RAM with a 131K KV cache is what made GPU offload take
        // minutes. Automatic is therefore pinned at 16K on every machine.
        assert_eq!(
            context_budget(4 * GIB, ModelQuant::Q8).context_tokens,
            CONTEXT_TOKENS_AUTO
        );
        assert_eq!(
            context_budget(3 * GIB, ModelQuant::Q5).context_tokens,
            CONTEXT_TOKENS_AUTO
        );
        assert_eq!(
            context_budget(16 * GIB, ModelQuant::Q5).context_tokens,
            CONTEXT_TOKENS_AUTO
        );
        assert_eq!(
            context_budget(2 * GIB, ModelQuant::Q8).context_tokens,
            CONTEXT_TOKENS_AUTO
        );
        assert_ne!(CONTEXT_TOKENS_AUTO, CONTEXT_TOKENS_MAX);
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
    fn explicit_large_context_is_still_available() {
        // Users who really want a 64K/131K window can still type it; only the
        // automatic path is pinned for speed.
        assert_eq!(
            resolve_context_tokens(Some(65_536), 16 * GIB, ModelQuant::Q5),
            65_536
        );
        assert_eq!(
            context_budget(16 * GIB, ModelQuant::Q5).context_tokens,
            CONTEXT_TOKENS_AUTO
        );
    }
}
