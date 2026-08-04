use crate::models::AiAssistant;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

const SQLITE_BUSY_TIMEOUT_SECS: u64 = 5;
const MIN_NORMALIZED_CHARS: usize = 3;
const MAX_NORMALIZED_CHARS: usize = 256;
const MAX_TOKENS: usize = 32;

pub fn build_match_expression(terms: &[String]) -> Option<String> {
    let tokens = terms
        .iter()
        .flat_map(|term| {
            term.split(|character: char| !character.is_alphanumeric())
                .filter(|token| !token.is_empty())
        })
        .collect::<Vec<_>>();
    let character_count = tokens
        .iter()
        .map(|token| token.chars().count())
        .sum::<usize>();
    if tokens.is_empty()
        || tokens.len() > MAX_TOKENS
        || !(MIN_NORMALIZED_CHARS..=MAX_NORMALIZED_CHARS).contains(&character_count)
    {
        return None;
    }
    let mut quoted = tokens
        .iter()
        .map(|token| format!("\"{token}\""))
        .collect::<Vec<_>>();
    quoted.last_mut()?.push('*');
    Some(quoted.join(" AND "))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellSearchMetadata {
    pub id: String,
    pub first_prompt: Option<String>,
    pub assistant: AiAssistant,
    pub project_name: Option<String>,
    pub last_updated: DateTime<Utc>,
    pub matched_snippet: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedShellSearchMetadata {
    pub name: String,
    pub description: String,
}

fn collapse_and_truncate(value: &str, limit: usize) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(limit)
        .collect()
}

fn relative_time(now: DateTime<Utc>, then: DateTime<Utc>) -> String {
    let duration = now
        .signed_duration_since(then)
        .max(chrono::Duration::zero());
    if duration < chrono::Duration::minutes(1) {
        "Just now".into()
    } else if duration < chrono::Duration::hours(1) {
        let value = duration.num_minutes();
        format!("{value} minute{} ago", if value == 1 { "" } else { "s" })
    } else if duration < chrono::Duration::days(1) {
        let value = duration.num_hours();
        format!("{value} hour{} ago", if value == 1 { "" } else { "s" })
    } else {
        let value = duration.num_days();
        format!("{value} day{} ago", if value == 1 { "" } else { "s" })
    }
}

impl ShellSearchMetadata {
    pub fn render(&self, now: DateTime<Utc>, show_excerpts: bool) -> RenderedShellSearchMetadata {
        let prompt = self
            .first_prompt
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let name = prompt
            .map(|value| collapse_and_truncate(value, 60))
            .unwrap_or_else(|| format!("Untitled {} session", self.assistant.display_name()));
        let safe_description = format!(
            "{} · {} · {}",
            self.assistant.display_name(),
            self.project_name.as_deref().unwrap_or("Unknown project"),
            relative_time(now, self.last_updated)
        );
        let description = if show_excerpts {
            self.matched_snippet
                .as_deref()
                .map(|value| collapse_and_truncate(value, 100))
                .filter(|value| !value.is_empty())
                .unwrap_or(safe_description)
        } else {
            safe_description
        };
        RenderedShellSearchMetadata { name, description }
    }
}

pub struct ShellSearchConnection {
    #[allow(dead_code)]
    connection: Connection,
}

#[derive(Clone)]
pub struct ShellSearchInterrupt {
    handle: Arc<rusqlite::InterruptHandle>,
}

impl ShellSearchConnection {
    pub fn open_read_only(path: &Path) -> Result<Option<(Self, ShellSearchInterrupt)>> {
        if !path.is_file() {
            return Ok(None);
        }

        // This connection is owned by the dedicated search worker.
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let connection = Connection::open_with_flags(path, flags)
            .with_context(|| format!("Failed to open shell search database: {}", path.display()))?;
        connection
            .busy_timeout(Duration::from_secs(SQLITE_BUSY_TIMEOUT_SECS))
            .context("Failed to set shell search SQLite busy timeout")?;
        let interrupt = ShellSearchInterrupt {
            handle: Arc::new(connection.get_interrupt_handle()),
        };

        Ok(Some((Self { connection }, interrupt)))
    }
}

impl ShellSearchInterrupt {
    pub fn interrupt(&self) {
        self.handle.interrupt();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AiAssistant;
    use chrono::{TimeZone, Utc};
    use std::fs;

    #[test]
    fn shell_terms_are_bounded_and_fts_safe() {
        let cases = [
            (vec!["ak"], None),
            (vec!["aki"], Some("\"aki\"*")),
            (
                vec!["foo-bar", "baz"],
                Some("\"foo\" AND \"bar\" AND \"baz\"*"),
            ),
            (vec!["AND"], Some("\"AND\"*")),
            (vec!["\""], None),
            (vec!["🙂"], None),
            (vec!["é中"], None),
            (vec!["é中a"], Some("\"é中a\"*")),
        ];
        for (terms, expected) in cases {
            let terms = terms.into_iter().map(str::to_string).collect::<Vec<_>>();
            assert_eq!(build_match_expression(&terms).as_deref(), expected);
        }

        let thirty_three = (0..33)
            .map(|index| format!("term{index}"))
            .collect::<Vec<_>>();
        assert_eq!(build_match_expression(&thirty_three), None);
        assert_eq!(build_match_expression(&["x".repeat(257)]), None);
        assert_eq!(build_match_expression(&["x".repeat(4096)]), None);
    }

    #[test]
    fn rendered_metadata_collapses_and_bounds_user_text() {
        let metadata = ShellSearchMetadata {
            id: "session-1".into(),
            first_prompt: Some(format!("  {}\nnext  ", "é".repeat(70))),
            assistant: AiAssistant::ClaudeCode,
            project_name: Some("sessions-chronicle".into()),
            last_updated: Utc.timestamp_opt(1_700_000_000, 0).unwrap(),
            matched_snippet: Some(format!("match\n{}", "中".repeat(120))),
        };
        let now = metadata.last_updated + chrono::Duration::days(3);

        let hidden = metadata.render(now, false);
        assert_eq!(hidden.name.chars().count(), 60);
        assert_eq!(
            hidden.description,
            "Claude Code · sessions-chronicle · 3 days ago"
        );
        assert!(!hidden.description.contains("match"));

        let shown = metadata.render(now, true);
        assert_eq!(shown.description.chars().count(), 100);
        assert!(shown.description.starts_with("match "));
    }

    #[test]
    fn missing_prompt_has_human_readable_assistant_fallback() {
        let metadata = ShellSearchMetadata {
            id: "opaque-id".into(),
            first_prompt: None,
            assistant: AiAssistant::OpenCode,
            project_name: None,
            last_updated: Utc::now(),
            matched_snippet: None,
        };
        assert_eq!(
            metadata.render(metadata.last_updated, false).name,
            "Untitled OpenCode session"
        );
    }

    #[test]
    fn missing_database_is_not_created() {
        let temp_dir = tempfile::tempdir().unwrap();
        let database_path = temp_dir.path().join("missing.db");

        assert!(
            ShellSearchConnection::open_read_only(&database_path)
                .unwrap()
                .is_none()
        );
        assert!(!database_path.exists());

        let directory_path = temp_dir.path().join("database-dir");
        fs::create_dir(&directory_path).unwrap();
        assert!(
            ShellSearchConnection::open_read_only(&directory_path)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn opened_database_rejects_writes() {
        let temp_dir = tempfile::tempdir().unwrap();
        let database_path = temp_dir.path().join("existing.db");
        let setup = Connection::open(&database_path).unwrap();
        setup
            .execute_batch("CREATE TABLE sessions (id INTEGER PRIMARY KEY)")
            .unwrap();
        drop(setup);

        let (connection, _interrupt) = ShellSearchConnection::open_read_only(&database_path)
            .unwrap()
            .unwrap();

        assert!(
            connection
                .connection
                .execute("INSERT INTO sessions DEFAULT VALUES", [])
                .is_err()
        );
    }
}
