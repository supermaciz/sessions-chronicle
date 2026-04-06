use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasoningAttachment {
    pub session_id: String,
    pub transcript_item_index: i64,
    pub visible_text: Option<String>,
    pub summary_text: Option<String>,
    pub has_encrypted_content: bool,
    pub source_model: Option<String>,
    pub source_timestamp: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReasoningPreview {
    pub has_reasoning: bool,
    pub has_visible_reasoning: bool,
    pub encrypted_only: bool,
}
