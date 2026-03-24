use crate::database::IndexingStats;
use crate::models::session::AiAssistant;

#[derive(Debug, Clone)]
pub struct IndexingRunResult {
    pub totals: IndexingStats,
    pub per_source: Vec<PerSourceResult>,
}

#[derive(Debug, Clone)]
pub struct PerSourceResult {
    pub assistant: AiAssistant,
    pub display_path: String,
    pub indexed: usize,
    pub skipped: usize,
    pub errors: usize,
    pub status: SourceStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceStatus {
    NotFound,
    Empty,
    Indexed,
    Degraded,
    Failed,
}
