use chrono::{DateTime, Utc};

use crate::models::{ReasoningPreview, Role};

#[derive(Debug, Clone)]
pub struct MessagePreview {
    pub session_id: String,
    pub message_index: usize,
    pub role: Role,
    pub content_preview: String,
    pub content_len: usize,
    pub timestamp: DateTime<Utc>,
    pub model: Option<String>,
    pub reasoning_preview: ReasoningPreview,
}

impl MessagePreview {
    pub fn is_truncated(&self) -> bool {
        self.content_preview.chars().count() < self.content_len
    }
}
