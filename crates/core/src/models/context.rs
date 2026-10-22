use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextKind {
    System,
    Skill,
    CompactionSummary,
}

impl ContextKind {
    pub fn from_storage(value: &str) -> Option<Self> {
        match value {
            "system" => Some(Self::System),
            "skill" => Some(Self::Skill),
            "compaction_summary" => Some(Self::CompactionSummary),
            _ => None,
        }
    }

    pub fn to_storage(&self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Skill => "skill",
            Self::CompactionSummary => "compaction_summary",
        }
    }
}

/// Text injected into a conversation that is neither a user nor an assistant
/// message. Stored outside `messages`, so it never reaches search or token
/// accounting. Identity is `(session_id, id)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextEntry {
    pub id: String,
    pub session_id: String,
    pub kind: ContextKind,
    pub name: Option<String>,
    pub text: String,
    pub timestamp: DateTime<Utc>,
    pub item_index: i64,
}

/// Body-less metadata for a context entry, used by transcript listings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextPreview {
    pub id: String,
    pub kind: ContextKind,
    pub name: Option<String>,
    pub timestamp: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_kind_storage_round_trips() {
        for (s, kind) in [
            ("system", ContextKind::System),
            ("skill", ContextKind::Skill),
            ("compaction_summary", ContextKind::CompactionSummary),
        ] {
            assert_eq!(ContextKind::from_storage(s), Some(kind));
            assert_eq!(kind.to_storage(), s);
        }
        assert_eq!(ContextKind::from_storage("bogus"), None);
    }
}
