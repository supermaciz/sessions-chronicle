use crate::models::AiAssistant;
use anyhow::{Context, Result};
use chrono::{DateTime, TimeZone, Utc};
use rusqlite::{Connection, OpenFlags, OptionalExtension, ToSql};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

const SQLITE_BUSY_TIMEOUT_SECS: u64 = 5;
const MIN_NORMALIZED_CHARS: usize = 3;
const MAX_NORMALIZED_CHARS: usize = 256;
const MAX_TOKENS: usize = 32;
const MAX_RENDERED_NAME_CHARS: usize = 60;
const MAX_RENDERED_PROJECT_CHARS: usize = 60;
const MAX_RENDERED_SNIPPET_CHARS: usize = 100;
pub const RESULT_LIMIT: usize = 20;

pub fn build_match_expression(terms: &[String]) -> Option<String> {
    let mut tokens = Vec::new();
    let mut character_count = 0;
    for term in terms {
        for token in term
            .split(|character: char| !character.is_alphanumeric())
            .filter(|token| !token.is_empty())
        {
            if tokens.len() >= MAX_TOKENS {
                return None;
            }
            let remaining_chars = MAX_NORMALIZED_CHARS - character_count;
            let token_chars = token.chars().take(remaining_chars + 1).count();
            if token_chars > remaining_chars {
                return None;
            }
            character_count += token_chars;
            tokens.push(token);
        }
    }
    if tokens.is_empty() || character_count < MIN_NORMALIZED_CHARS {
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
    let mut collapsed = String::with_capacity(value.len().min(limit));
    let mut rendered_chars = 0;
    for word in value.split_whitespace() {
        if rendered_chars == limit {
            break;
        }
        if rendered_chars > 0 {
            collapsed.push(' ');
            rendered_chars += 1;
        }
        for character in word.chars() {
            if rendered_chars == limit {
                break;
            }
            collapsed.push(character);
            rendered_chars += 1;
        }
    }
    collapsed
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
            .map(|value| collapse_and_truncate(value, MAX_RENDERED_NAME_CHARS))
            .unwrap_or_else(|| format!("Untitled {} session", self.assistant.display_name()));
        let project_name = self
            .project_name
            .as_deref()
            .map(|value| collapse_and_truncate(value, MAX_RENDERED_PROJECT_CHARS))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "Unknown project".into());
        let safe_description = format!(
            "{} · {} · {}",
            self.assistant.display_name(),
            project_name,
            relative_time(now, self.last_updated)
        );
        let description = if show_excerpts {
            self.matched_snippet
                .as_deref()
                .map(|value| collapse_and_truncate(value, MAX_RENDERED_SNIPPET_CHARS))
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

    pub fn load_metadata(
        &self,
        ids: &[String],
        show_excerpts: bool,
        expression: Option<&str>,
    ) -> Result<Vec<Option<ShellSearchMetadata>>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut seen_ids = HashSet::new();
        let unique_ids = ids
            .iter()
            .filter(|id| seen_ids.insert((*id).clone()))
            .cloned()
            .collect::<Vec<_>>();
        let placeholders = (1..=unique_ids.len())
            .map(|index| format!("?{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT s.id, s.first_prompt, s.tool, p.name, s.last_updated
                 FROM sessions s
                 LEFT JOIN projects p ON p.id = s.project_id
                 WHERE s.is_subagent = 0 AND s.id IN ({placeholders})"
            ))
            .context("Failed to prepare shell search metadata query")?;
        let params = unique_ids
            .iter()
            .map(|id| id as &dyn ToSql)
            .collect::<Vec<_>>();
        let rows = statement
            .query_map(params.as_slice(), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })
            .context("Failed to execute shell search metadata query")?;
        let rows = rows
            .collect::<rusqlite::Result<Vec<_>>>()
            .context("Failed to read shell search metadata")?;

        let mut metadata_by_id = HashMap::with_capacity(rows.len());
        for (id, first_prompt, tool, project_name, last_updated) in rows {
            let assistant = AiAssistant::from_storage(&tool)
                .ok_or_else(|| anyhow::anyhow!("Unknown assistant tool in session {id}: {tool}"))?;
            let last_updated = Utc.timestamp_opt(last_updated, 0).single().ok_or_else(|| {
                anyhow::anyhow!("Malformed last_updated timestamp for session {id}")
            })?;
            metadata_by_id.insert(
                id.clone(),
                ShellSearchMetadata {
                    id,
                    first_prompt,
                    assistant,
                    project_name,
                    last_updated,
                    matched_snippet: None,
                },
            );
        }

        if show_excerpts {
            if let Some(expression) = expression {
                for metadata in metadata_by_id.values_mut() {
                    metadata.matched_snippet = self
                        .connection
                        .query_row(
                            "SELECT snippet(messages_fts, 0, '', '', '…', 32)
                             FROM messages_fts
                             JOIN messages m ON m.id = messages_fts.rowid
                             WHERE messages_fts MATCH ?1 AND m.session_id = ?2
                             ORDER BY messages_fts.rank ASC, m.id ASC
                             LIMIT 1",
                            rusqlite::params![expression, &metadata.id],
                            |row| row.get(0),
                        )
                        .optional()
                        .context("Failed to load shell search metadata snippet")?;
                }
            }
        }

        Ok(ids
            .iter()
            .map(|id| metadata_by_id.get(id).cloned())
            .collect())
    }
}

pub fn search_session_ids(
    connection: &ShellSearchConnection,
    match_expression: &str,
) -> Result<Vec<String>> {
    let mut statement = connection
        .connection
        .prepare(
            "WITH ranked_messages AS MATERIALIZED (
             SELECT s.id AS session_id,
                    s.last_updated,
                    messages_fts.rank AS message_rank
             FROM messages_fts
             JOIN messages m ON m.id = messages_fts.rowid
             JOIN sessions s ON s.id = m.session_id
             WHERE messages_fts MATCH ?1
               AND s.is_subagent = 0
         )
         SELECT session_id,
                MIN(message_rank) AS session_rank,
                MAX(last_updated) AS session_last_updated
         FROM ranked_messages
         GROUP BY session_id
         ORDER BY session_rank ASC, session_last_updated DESC, session_id ASC
         LIMIT ?2",
        )
        .context("Failed to prepare ranked shell search query")?;
    let result_limit = RESULT_LIMIT as i64;
    let rows = statement
        .query_map([&match_expression as &dyn ToSql, &result_limit], |row| {
            row.get(0)
        })
        .context("Failed to execute ranked shell search query")?;
    rows.collect::<rusqlite::Result<Vec<String>>>()
        .context("Failed to read ranked shell search results")
}

pub fn subsearch_session_ids(
    connection: &ShellSearchConnection,
    match_expression: &str,
    previous_ids: &[String],
) -> Result<Vec<String>> {
    if previous_ids.is_empty() {
        return Ok(Vec::new());
    }

    let previous_ids = &previous_ids[..previous_ids.len().min(RESULT_LIMIT)];
    let placeholders = (2..=previous_ids.len() + 1)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let query = format!(
        "WITH ranked_messages AS MATERIALIZED (
             SELECT s.id AS session_id
             FROM messages_fts
             JOIN messages m ON m.id = messages_fts.rowid
             JOIN sessions s ON s.id = m.session_id
             WHERE messages_fts MATCH ?1
               AND s.is_subagent = 0
               AND s.id IN ({placeholders})
         )
         SELECT DISTINCT session_id
         FROM ranked_messages
         LIMIT ?{}",
        previous_ids.len() + 2
    );
    let mut params: Vec<&dyn ToSql> = Vec::with_capacity(previous_ids.len() + 2);
    params.push(&match_expression);
    params.extend(previous_ids.iter().map(|id| id as &dyn ToSql));
    let result_limit = RESULT_LIMIT as i64;
    params.push(&result_limit);

    let mut statement = connection
        .connection
        .prepare(&query)
        .context("Failed to prepare bounded shell subsearch query")?;
    let rows = statement
        .query_map(params.as_slice(), |row| row.get(0))
        .context("Failed to execute bounded shell subsearch query")?;
    let matching_ids = rows
        .collect::<rusqlite::Result<HashSet<String>>>()
        .context("Failed to read bounded shell subsearch results")?;

    Ok(previous_ids
        .iter()
        .filter(|id| matching_ids.contains(*id))
        .filter_map({
            let mut seen = HashSet::new();
            move |id| seen.insert(id.as_str()).then_some(id.clone())
        })
        .take(RESULT_LIMIT)
        .collect())
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

        let thirty_two = (0..32)
            .map(|index| format!("term{index}"))
            .collect::<Vec<_>>();
        assert!(build_match_expression(&thirty_two).is_some());
        assert!(build_match_expression(&["x".repeat(256)]).is_some());
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

        let multiline_project = ShellSearchMetadata {
            project_name: Some("project\nwith\tmultiple lines".into()),
            ..metadata.clone()
        };
        assert_eq!(
            multiline_project.render(now, false).description,
            "Claude Code · project with multiple lines · 3 days ago"
        );

        let oversized_project = ShellSearchMetadata {
            project_name: Some("p".repeat(61)),
            ..metadata.clone()
        };
        assert_eq!(
            oversized_project
                .render(now, false)
                .description
                .chars()
                .count(),
            "Claude Code · ".chars().count() + 60 + " · 3 days ago".chars().count()
        );
        assert!(
            !oversized_project
                .render(now, false)
                .description
                .contains('\n')
        );

        let shown = metadata.render(now, true);
        assert_eq!(shown.description.chars().count(), 100);
        assert!(shown.description.starts_with("match "));
    }

    #[test]
    fn relative_time_formats_units_and_boundaries() {
        let now = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let cases = [
            ("future", chrono::Duration::seconds(-1), "Just now"),
            ("just now", chrono::Duration::seconds(59), "Just now"),
            ("one minute", chrono::Duration::minutes(1), "1 minute ago"),
            (
                "multiple minutes",
                chrono::Duration::minutes(59),
                "59 minutes ago",
            ),
            ("one hour", chrono::Duration::hours(1), "1 hour ago"),
            (
                "multiple hours",
                chrono::Duration::hours(23),
                "23 hours ago",
            ),
            ("one day", chrono::Duration::days(1), "1 day ago"),
            ("multiple days", chrono::Duration::days(2), "2 days ago"),
        ];

        for (label, elapsed, expected) in cases {
            assert_eq!(relative_time(now, now - elapsed), expected, "{label}");
        }
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
