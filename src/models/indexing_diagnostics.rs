use crate::database::IndexingStats;
use crate::models::session::AiAssistant;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexingError {
    pub assistant: AiAssistant,
    pub location: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct IndexingRunResult {
    pub totals: IndexingStats,
    pub per_source: Vec<PerSourceResult>,
    pub errors_detail: Vec<IndexingError>,
}

#[derive(Debug, Clone)]
pub struct PerSourceResult {
    pub assistant: AiAssistant,
    pub display_path: String,
    pub indexed: usize,
    pub skipped: usize,
    pub removed: usize,
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
