pub mod config;
pub mod database;
mod icon_names {
    pub use shipped::*;
    include!(concat!(env!("OUT_DIR"), "/icon_names.rs"));
}
pub mod models;
pub mod parsers;
pub mod project_resolver;
pub mod session_sources;
mod ui;
pub mod utils;

// Re-export commonly used types
pub use models::{
    AiAssistant, AnalyticsData, AnalyticsOverview, Message, ProjectFilter, ProjectInfo, Role,
    Session,
};
pub use session_sources::SessionSources;
