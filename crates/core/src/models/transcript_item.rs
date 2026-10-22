use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TranscriptItemKind {
    Message,
    ToolCall,
    Subagent,
    Context,
    /// Returned by `from_storage` when the DB contains an unrecognised kind string.
    /// This should not be constructed directly during parsing.
    Unknown,
}

impl TranscriptItemKind {
    pub fn from_storage(value: &str) -> Self {
        match value {
            "message" => Self::Message,
            "tool_call" => Self::ToolCall,
            "subagent" => Self::Subagent,
            "context" => Self::Context,
            _ => Self::Unknown,
        }
    }

    pub fn to_storage(&self) -> &'static str {
        match self {
            Self::Message => "message",
            Self::ToolCall => "tool_call",
            Self::Subagent => "subagent",
            Self::Context => "context",
            Self::Unknown => "unknown",
        }
    }
}

/// A single entry in the ordered transcript stream for a session.
///
/// **Invariant:** exactly one of `message_index`, `tool_call_id`, `subagent_id`, or
/// `context_id` is `Some`, determined by `kind`:
/// - `kind == Message`  → `message_index` is `Some`
/// - `kind == ToolCall` → `tool_call_id` is `Some`
/// - `kind == Subagent` → `subagent_id` is `Some`
/// - `kind == Context`  → `context_id` is `Some`
/// - `kind == Unknown`  → all four are `None` (DB row with unrecognised kind)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptItem {
    pub session_id: String,
    pub item_index: i64,
    pub kind: TranscriptItemKind,
    /// Set when kind == Message
    pub message_index: Option<i64>,
    /// Set when kind == ToolCall
    pub tool_call_id: Option<String>,
    /// Set when kind == Subagent
    pub subagent_id: Option<String>,
    /// Set when kind == Context
    #[serde(default)]
    pub context_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_storage_unknown_returns_unknown_variant() {
        assert_eq!(
            TranscriptItemKind::from_storage("bogus"),
            TranscriptItemKind::Unknown
        );
        assert_eq!(
            TranscriptItemKind::from_storage(""),
            TranscriptItemKind::Unknown
        );
    }

    #[test]
    fn from_storage_known_values_round_trip() {
        for (s, expected) in [
            ("message", TranscriptItemKind::Message),
            ("tool_call", TranscriptItemKind::ToolCall),
            ("subagent", TranscriptItemKind::Subagent),
            ("context", TranscriptItemKind::Context),
        ] {
            let parsed = TranscriptItemKind::from_storage(s);
            assert_eq!(parsed, expected);
            assert_eq!(parsed.to_storage(), s);
        }
    }
}
