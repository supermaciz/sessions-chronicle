#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionQuery<'a> {
    None,
    DirectId(&'a str),
    Fts(&'a str),
}

impl<'a> SessionQuery<'a> {
    pub fn classify(query: &'a str) -> Self {
        let query = query.trim();
        if query.is_empty() {
            Self::None
        } else if let Some(id) = query.strip_prefix("id:") {
            Self::DirectId(id.trim())
        } else {
            Self::Fts(query)
        }
    }

    pub fn is_fts(self) -> bool {
        matches!(self, Self::Fts(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_empty_direct_id_and_fts_queries() {
        assert_eq!(SessionQuery::classify(" \n "), SessionQuery::None);
        assert_eq!(
            SessionQuery::classify("id:abc"),
            SessionQuery::DirectId("abc")
        );
        assert_eq!(
            SessionQuery::classify(" id: abc "),
            SessionQuery::DirectId("abc")
        );
        assert_eq!(SessionQuery::classify("id:   "), SessionQuery::DirectId(""));
        assert_eq!(
            SessionQuery::classify("ID:abc"),
            SessionQuery::Fts("ID:abc")
        );
        assert_eq!(
            SessionQuery::classify(" needle "),
            SessionQuery::Fts("needle")
        );
    }

    #[test]
    fn only_full_text_queries_report_fts_context() {
        assert!(!SessionQuery::classify("").is_fts());
        assert!(!SessionQuery::classify("id:abc").is_fts());
        assert!(SessionQuery::classify("abc").is_fts());
    }
}
