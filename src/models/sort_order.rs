#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    RecentActivity,
    OldestFirst,
    NewestFirst,
    MostMessages,
}

impl SortOrder {
    pub fn label_msgid(self) -> &'static str {
        match self {
            Self::RecentActivity => "Recent activity",
            Self::OldestFirst => "Oldest first",
            Self::NewestFirst => "Newest first",
            Self::MostMessages => "Most messages",
        }
    }

    pub fn as_setting_str(self) -> &'static str {
        match self {
            Self::RecentActivity => "recent-activity",
            Self::OldestFirst => "oldest-first",
            Self::NewestFirst => "newest-first",
            Self::MostMessages => "most-messages",
        }
    }

    pub fn from_setting_str(value: &str) -> Self {
        match value {
            "recent-activity" => Self::RecentActivity,
            "oldest-first" => Self::OldestFirst,
            "newest-first" => Self::NewestFirst,
            "most-messages" => Self::MostMessages,
            _ => Self::default(),
        }
    }
}

impl Default for SortOrder {
    fn default() -> Self {
        Self::RecentActivity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setting_values_round_trip() {
        for (order, value) in [
            (SortOrder::RecentActivity, "recent-activity"),
            (SortOrder::OldestFirst, "oldest-first"),
            (SortOrder::NewestFirst, "newest-first"),
            (SortOrder::MostMessages, "most-messages"),
        ] {
            assert_eq!(order.as_setting_str(), value);
            assert_eq!(SortOrder::from_setting_str(value), order);
        }
    }

    #[test]
    fn unknown_setting_falls_back_to_recent_activity() {
        assert_eq!(
            SortOrder::from_setting_str("relevance"),
            SortOrder::default()
        );
        assert_eq!(
            SortOrder::from_setting_str("hand-edited"),
            SortOrder::default()
        );
        assert_eq!(SortOrder::default(), SortOrder::RecentActivity);
    }

    #[test]
    fn labels_are_stable_untranslated_message_ids() {
        assert_eq!(SortOrder::RecentActivity.label_msgid(), "Recent activity");
        assert_eq!(SortOrder::OldestFirst.label_msgid(), "Oldest first");
        assert_eq!(SortOrder::NewestFirst.label_msgid(), "Newest first");
        assert_eq!(SortOrder::MostMessages.label_msgid(), "Most messages");
    }
}
