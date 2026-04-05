use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasoningAttachment {
    pub session_id: String,
    pub transcript_item_index: i64,
    pub visible_text: Option<String>,
    pub summary_text: Option<String>,
    pub encrypted_content: Option<String>,
    pub source_model: Option<String>,
    pub source_timestamp: Option<DateTime<Utc>>,
}

impl ReasoningAttachment {
    #[allow(dead_code)]
    pub fn has_reasoning(&self) -> bool {
        self.visible_text.is_some()
            || self.summary_text.is_some()
            || self.encrypted_content.is_some()
    }

    #[allow(dead_code)]
    pub fn has_visible_reasoning(&self) -> bool {
        self.visible_text.is_some() || self.summary_text.is_some()
    }

    #[allow(dead_code)]
    pub fn encrypted_only(&self) -> bool {
        self.encrypted_content.is_some()
            && self.visible_text.is_none()
            && self.summary_text.is_none()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReasoningPreview {
    pub has_reasoning: bool,
    pub has_visible_reasoning: bool,
    pub encrypted_only: bool,
}

impl ReasoningPreview {
    #[allow(dead_code)]
    pub fn from_attachment(attachment: &ReasoningAttachment) -> Self {
        Self {
            has_reasoning: attachment.has_reasoning(),
            has_visible_reasoning: attachment.has_visible_reasoning(),
            encrypted_only: attachment.encrypted_only(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypted_only_requires_no_visible_or_summary_text() {
        let attachment = ReasoningAttachment {
            session_id: "s1".to_string(),
            transcript_item_index: 4,
            visible_text: None,
            summary_text: None,
            encrypted_content: Some("ciphertext".to_string()),
            source_model: None,
            source_timestamp: None,
        };

        assert!(attachment.has_reasoning());
        assert!(!attachment.has_visible_reasoning());
        assert!(attachment.encrypted_only());
    }
}
