#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ProjectFilter {
    #[default]
    AllSessions,
    Pinned,
    Project(i64),
    Unassigned,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectInfo {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub session_count: usize,
}
