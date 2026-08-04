use serde::{Deserialize, Serialize};

/// Normalized session-level token usage across supported AI assistants.
///
/// The shape is shared, but field semantics are not perfectly identical across
/// providers.
///
/// In particular, `cache_*` values are separate cache-related token counters, not always
/// extra tokens to add on top of `input_tokens`. For example, Codex/OpenAI
/// reports cached input as a subset of total input tokens, while other session
/// formats may report cache separately or not expose it at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Input or prompt tokens reported for the session.
    ///
    /// Depending on the source format, this may exclude cache hits, include
    /// them as a subset, or leave that distinction unspecified.
    pub input_tokens: i64,
    /// Output or completion tokens reported for the session.
    pub output_tokens: i64,
    /// Tokens served from cache when the source format exposes them.
    ///
    /// This may overlap with `input_tokens` rather than being an additional
    /// bucket to add on top.
    pub cache_read_tokens: Option<i64>,
    /// Tokens written into cache when the source format exposes them.
    pub cache_write_tokens: Option<i64>,
    /// Additional reasoning tokens when the source format exposes them.
    pub reasoning_tokens: Option<i64>,
}
