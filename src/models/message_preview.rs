use chrono::{DateTime, Utc};

use crate::models::Role;

#[derive(Debug, Clone)]
pub struct MessagePreview {
    pub role: Role,
    pub content_preview: String,
    pub content_len: usize,
    pub timestamp: DateTime<Utc>,
}

impl MessagePreview {
    pub fn is_truncated(&self) -> bool {
        self.content_preview.chars().count() < self.content_len
    }
}
