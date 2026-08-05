pub mod database;
pub mod models;
pub mod parsers;
pub mod project_resolver;
pub mod session_sources;
pub mod utils;

pub use models::{
    AiAssistant, AnalyticsData, AnalyticsOverview, DateCounts, DateFilter, Message, ProjectFilter,
    ProjectInfo, Role, Session, SortOrder,
};
pub use session_sources::SessionSources;

#[cfg(test)]
pub(crate) fn fixture_path(relative: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(relative)
}
