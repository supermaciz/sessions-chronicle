#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[allow(dead_code)] // Used by upcoming project sidebar wiring tasks.
pub enum ProjectFilter {
    #[default]
    AllSessions,
    Project(i64),
    Unassigned,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // Used by upcoming project sidebar wiring tasks.
pub struct ProjectInfo {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub session_count: usize,
}
