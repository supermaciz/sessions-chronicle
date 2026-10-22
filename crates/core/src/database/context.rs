use anyhow::{Context, Result};
use chrono::{TimeZone, Utc};
use rusqlite::{OptionalExtension, Transaction};
use std::path::Path;

use crate::models::ContextEntry;

use super::open_connection;

/// Insert a context entry (upsert by session_id + id) inside the session
/// transaction. The body is never mirrored into `messages` or `messages_fts`.
pub(crate) fn insert_context_entry(tx: &Transaction<'_>, entry: &ContextEntry) -> Result<()> {
    tx.execute(
        "INSERT OR REPLACE INTO context_entries
         (session_id, id, kind, name, text, timestamp, item_index)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            entry.session_id,
            entry.id,
            entry.kind.to_storage(),
            entry.name.as_deref(),
            entry.text,
            entry.timestamp.timestamp(),
            entry.item_index,
        ],
    )
    .context("Failed to insert context entry")?;
    Ok(())
}

/// Load the full body of one context entry, scoped to its session.
pub fn load_context_full_content(
    db_path: &Path,
    session_id: &str,
    context_id: &str,
) -> Result<Option<String>> {
    if !db_path.exists() {
        return Ok(None);
    }
    let db = open_connection(db_path)?;
    db.query_row(
        "SELECT text FROM context_entries WHERE session_id = ?1 AND id = ?2",
        rusqlite::params![session_id, context_id],
        |row| row.get(0),
    )
    .optional()
    .context("Failed to load context content")
}

pub(super) fn context_timestamp(seconds: i64) -> chrono::DateTime<Utc> {
    Utc.timestamp_opt(seconds, 0)
        .single()
        .unwrap_or_else(Utc::now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::indexer::SessionIndexer;
    use crate::database::{find_session_match_positions, load_all_transcript_items};
    use crate::models::{
        AiAssistant, ContextEntry, ContextKind, Message, ReasoningAttachment, Role, Session,
        SessionEndingStatus, TranscriptItem, TranscriptItemKind,
    };
    use crate::parsers::ParsedSession;
    use chrono::{TimeZone, Utc};
    use rusqlite::Connection;
    use std::path::Path;
    use tempfile::NamedTempFile;

    const NEEDLE: &str = "contextonlyneedle";

    fn ts(seconds: i64) -> chrono::DateTime<Utc> {
        Utc.timestamp_opt(seconds, 0).single().unwrap()
    }

    fn entry(
        session_id: &str,
        id: &str,
        kind: ContextKind,
        name: Option<&str>,
        text: &str,
        item_index: i64,
    ) -> ContextEntry {
        ContextEntry {
            id: id.to_string(),
            session_id: session_id.to_string(),
            kind,
            name: name.map(str::to_string),
            text: text.to_string(),
            timestamp: ts(1_700_000_000 + item_index),
            item_index,
        }
    }

    fn context_item(session_id: &str, item_index: i64, context_id: &str) -> TranscriptItem {
        TranscriptItem {
            session_id: session_id.to_string(),
            item_index,
            kind: TranscriptItemKind::Context,
            message_index: None,
            tool_call_id: None,
            subagent_id: None,
            context_id: Some(context_id.to_string()),
        }
    }

    fn message_item(session_id: &str, item_index: i64, message_index: i64) -> TranscriptItem {
        TranscriptItem {
            session_id: session_id.to_string(),
            item_index,
            kind: TranscriptItemKind::Message,
            message_index: Some(message_index),
            tool_call_id: None,
            subagent_id: None,
            context_id: None,
        }
    }

    /// Layout: first context, the user message, then the remaining contexts.
    fn parsed_session(
        session_id: &str,
        tool: AiAssistant,
        file_path: &str,
        user_text: &str,
        mut contexts: Vec<ContextEntry>,
    ) -> ParsedSession {
        let mut transcript_items = Vec::new();
        for (position, context) in contexts.iter_mut().enumerate() {
            if position == 1 {
                transcript_items.push(message_item(session_id, 1, 0));
            }
            context.item_index = if position == 0 {
                0
            } else {
                position as i64 + 1
            };
            transcript_items.push(context_item(session_id, context.item_index, &context.id));
        }
        if contexts.len() < 2 {
            transcript_items.push(message_item(session_id, 1, 0));
        }

        ParsedSession {
            session: Session {
                id: session_id.to_string(),
                tool,
                project_path: None,
                project_id: None,
                start_time: ts(1_700_000_000),
                message_count: 1,
                file_path: file_path.to_string(),
                last_updated: ts(1_700_000_100),
                pinned_at: None,
                first_prompt: Some(user_text.to_string()),
                parent_session_id: None,
                is_subagent: false,
                token_usage: None,
                edit_count: 0,
                read_count: 0,
                command_count: 0,
                ending_status: SessionEndingStatus::Unknown,
            },
            messages: vec![Message {
                session_id: session_id.to_string(),
                index: 0,
                role: Role::User,
                content: user_text.to_string(),
                timestamp: ts(1_700_000_001),
                model: None,
            }],
            tool_calls: vec![],
            subagents: vec![],
            transcript_items,
            reasoning_attachments: vec![ReasoningAttachment {
                session_id: session_id.to_string(),
                transcript_item_index: 1,
                visible_text: Some(format!("reasoning for {user_text}")),
                summary_text: None,
                has_encrypted_content: false,
                source_model: None,
                source_timestamp: None,
            }],
            context_entries: contexts,
            token_usage: None,
            agent_path: None,
        }
    }

    fn three_contexts(session_id: &str, system_text: &str) -> Vec<ContextEntry> {
        let mut contexts = vec![
            entry(
                session_id,
                "msg_system",
                ContextKind::System,
                None,
                system_text,
                0,
            ),
            entry(
                session_id,
                "msg_skill",
                ContextKind::Skill,
                Some("brainstorming"),
                "skill body",
                0,
            ),
            entry(
                session_id,
                "msg_compaction",
                ContextKind::CompactionSummary,
                None,
                "compaction body",
                0,
            ),
        ];
        for (offset, context) in contexts.iter_mut().enumerate() {
            context.timestamp = ts(1_700_000_010 + offset as i64);
        }
        contexts
    }

    fn persist(indexer: &mut SessionIndexer, parsed: &ParsedSession) -> anyhow::Result<()> {
        let marker = NamedTempFile::new()?;
        indexer.insert_parsed_session_with_fingerprint(
            parsed,
            Path::new(&parsed.session.file_path),
            marker.path(),
        )
    }

    fn count(db: &Connection, sql: &str, session_id: &str) -> i64 {
        db.query_row(sql, [session_id], |row| row.get(0)).unwrap()
    }

    fn dependent_row_counts(db: &Connection, session_id: &str) -> Vec<(&'static str, i64)> {
        [
            ("sessions", "SELECT count(*) FROM sessions WHERE id = ?1"),
            (
                "messages",
                "SELECT count(*) FROM messages WHERE session_id = ?1",
            ),
            (
                "transcript_items",
                "SELECT count(*) FROM transcript_items WHERE session_id = ?1",
            ),
            (
                "reasoning_attachments",
                "SELECT count(*) FROM reasoning_attachments WHERE session_id = ?1",
            ),
            (
                "context_entries",
                "SELECT count(*) FROM context_entries WHERE session_id = ?1",
            ),
            (
                "opencode_session_sources",
                "SELECT count(*) FROM opencode_session_sources WHERE session_id = ?1",
            ),
        ]
        .into_iter()
        .map(|(table, sql)| (table, count(db, sql, session_id)))
        .collect()
    }

    fn seed_generation(db: &Connection, session_id: &str) {
        db.execute(
            "INSERT OR REPLACE INTO opencode_session_sources (session_id, generation)
             VALUES (?1, 'v1')",
            [session_id],
        )
        .unwrap();
    }

    fn open_db(path: &Path) -> Connection {
        crate::database::open_connection(path).unwrap()
    }

    #[test]
    fn context_round_trip_without_message_fts() -> anyhow::Result<()> {
        let temp = NamedTempFile::new()?;
        let path = temp.path().to_path_buf();
        let mut indexer = SessionIndexer::new(&path)?;

        let ses_a = parsed_session(
            "ses_a",
            AiAssistant::OpenCode,
            "/tmp/ses_a.db",
            "plain user text",
            three_contexts("ses_a", NEEDLE),
        );
        persist(&mut indexer, &ses_a)?;
        let ses_b = parsed_session(
            "ses_b",
            AiAssistant::OpenCode,
            "/tmp/ses_b.db",
            "another user text",
            vec![entry(
                "ses_b",
                "msg_system",
                ContextKind::System,
                None,
                "other session body",
                0,
            )],
        );
        persist(&mut indexer, &ses_b)?;

        let rows = load_all_transcript_items(&path, "ses_a", 100)?;
        assert_eq!(rows.iter().filter(|r| r.context.is_some()).count(), 3);
        assert_eq!(
            rows.iter().map(|r| r.kind.clone()).collect::<Vec<_>>(),
            vec![
                TranscriptItemKind::Context,
                TranscriptItemKind::Message,
                TranscriptItemKind::Context,
                TranscriptItemKind::Context,
            ]
        );
        let previews: Vec<_> = rows.iter().filter_map(|r| r.context.clone()).collect();
        assert_eq!(
            previews
                .iter()
                .map(|p| (p.id.as_str(), p.kind, p.name.as_deref()))
                .collect::<Vec<_>>(),
            vec![
                ("msg_system", ContextKind::System, None),
                ("msg_skill", ContextKind::Skill, Some("brainstorming")),
                ("msg_compaction", ContextKind::CompactionSummary, None),
            ]
        );
        assert_eq!(
            previews.iter().map(|p| p.timestamp).collect::<Vec<_>>(),
            vec![ts(1_700_000_010), ts(1_700_000_011), ts(1_700_000_012)]
        );

        assert_eq!(
            load_context_full_content(&path, "ses_a", "msg_system")?,
            Some("contextonlyneedle".into())
        );
        assert_eq!(
            load_context_full_content(&path, "ses_a", "msg_skill")?,
            Some("skill body".into())
        );
        assert_eq!(
            load_context_full_content(&path, "ses_b", "msg_system")?,
            Some("other session body".into())
        );
        assert_eq!(
            load_context_full_content(&path, "ses_b", "msg_skill")?,
            None
        );
        assert_eq!(
            load_context_full_content(&path, "ses_missing", "msg_system")?,
            None
        );
        assert_eq!(
            load_context_full_content(&temp.path().with_extension("absent"), "ses_a", "x")?,
            None
        );

        let db = open_db(&path);
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM messages WHERE content LIKE '%contextonlyneedle%'",
                [],
                |r| r.get::<_, i64>(0)
            )?,
            0
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM messages_fts WHERE messages_fts MATCH 'contextonlyneedle'",
                [],
                |r| r.get::<_, i64>(0)
            )?,
            0
        );
        assert!(find_session_match_positions(&db, "ses_a", NEEDLE)?.is_empty());
        assert_eq!(
            db.query_row("SELECT count(*) FROM messages", [], |r| r.get::<_, i64>(0))?,
            2
        );
        Ok(())
    }

    #[test]
    fn context_transcript_item_reference_invariant_is_enforced() -> anyhow::Result<()> {
        let temp = NamedTempFile::new()?;
        let mut indexer = SessionIndexer::new(temp.path())?;
        let build = || {
            parsed_session(
                "ses_inv",
                AiAssistant::OpenCode,
                "/tmp/ses_inv.db",
                "text",
                three_contexts("ses_inv", "kept"),
            )
        };
        persist(&mut indexer, &build())?;

        type Mutation = fn(&mut ParsedSession);
        let cases: [(&str, Mutation); 3] = [
            ("context without context_id", |p| {
                p.transcript_items[0].context_id = None;
            }),
            ("context that also has a message_index", |p| {
                p.transcript_items[0].message_index = Some(0);
            }),
            ("message that also has a context_id", |p| {
                p.transcript_items[1].context_id = Some("msg_system".into());
            }),
        ];
        for (label, mutate) in cases {
            let mut bad = build();
            bad.messages[0].content = "replacement must not land".into();
            mutate(&mut bad);
            assert!(
                persist(&mut indexer, &bad).is_err(),
                "{label} must be rejected"
            );
        }

        let db = open_db(temp.path());
        assert_eq!(
            count(
                &db,
                "SELECT count(*) FROM context_entries WHERE session_id = ?1",
                "ses_inv"
            ),
            3
        );
        assert_eq!(
            db.query_row(
                "SELECT content FROM messages WHERE session_id = 'ses_inv'",
                [],
                |r| r.get::<_, String>(0)
            )?,
            "text",
            "rejected writes leave the previous session intact"
        );
        Ok(())
    }

    #[test]
    fn context_replacement_and_deletion_are_atomic() -> anyhow::Result<()> {
        let temp = NamedTempFile::new()?;
        let path = temp.path().to_path_buf();
        let mut indexer = SessionIndexer::new(&path)?;
        let db = open_db(&path);

        let old = parsed_session(
            "ses_x",
            AiAssistant::OpenCode,
            "/tmp/ses_x.db",
            "old user text",
            three_contexts("ses_x", "old system"),
        );
        persist(&mut indexer, &old)?;
        seed_generation(&db, "ses_x");
        let baseline = dependent_row_counts(&db, "ses_x");
        assert!(baseline.iter().all(|(_, n)| *n > 0), "{baseline:?}");

        // Replacement failing on context insertion keeps the old session.
        db.execute_batch(
            "CREATE TRIGGER fail_context_insert BEFORE INSERT ON context_entries
             BEGIN SELECT RAISE(ABORT, 'context insert blocked'); END;",
        )?;
        let new = parsed_session(
            "ses_x",
            AiAssistant::OpenCode,
            "/tmp/ses_x.db",
            "new user text",
            three_contexts("ses_x", "new system"),
        );
        assert!(persist(&mut indexer, &new).is_err());
        assert_eq!(dependent_row_counts(&db, "ses_x"), baseline);
        assert_eq!(
            db.query_row(
                "SELECT content FROM messages WHERE session_id = 'ses_x'",
                [],
                |r| r.get::<_, String>(0)
            )?,
            "old user text"
        );
        assert_eq!(
            load_context_full_content(&path, "ses_x", "msg_system")?,
            Some("old system".into())
        );
        assert_eq!(
            db.query_row(
                "SELECT visible_text FROM reasoning_attachments WHERE session_id = 'ses_x'",
                [],
                |r| r.get::<_, String>(0)
            )?,
            "reasoning for old user text"
        );

        // Successful replacement swaps every dependent row.
        db.execute_batch("DROP TRIGGER fail_context_insert")?;
        persist(&mut indexer, &new)?;
        assert_eq!(
            load_context_full_content(&path, "ses_x", "msg_system")?,
            Some("new system".into())
        );
        assert_eq!(
            dependent_row_counts(&db, "ses_x"),
            baseline,
            "replacement neither leaks nor drops generation metadata"
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM messages_fts WHERE messages_fts MATCH 'old'",
                [],
                |r| r.get::<_, i64>(0)
            )?,
            0
        );

        // Deletion failing on context removal is rolled back as a whole.
        db.execute_batch(
            "CREATE TRIGGER fail_context_delete BEFORE DELETE ON context_entries
             BEGIN SELECT RAISE(ABORT, 'context delete blocked'); END;",
        )?;
        assert!(indexer.remove_session_by_id("ses_x").is_err());
        assert_eq!(dependent_row_counts(&db, "ses_x"), baseline);
        db.execute_batch("DROP TRIGGER fail_context_delete")?;

        let all_zero = |db: &Connection| {
            dependent_row_counts(db, "ses_x")
                .iter()
                .all(|(_, n)| *n == 0)
        };

        // Explicit deletion by id.
        assert_eq!(indexer.remove_session_by_id("ses_x")?, 1);
        assert!(all_zero(&db));

        // File removal.
        persist(&mut indexer, &new)?;
        seed_generation(&db, "ses_x");
        assert_eq!(
            indexer.remove_session_for_file(Path::new("/tmp/ses_x.db"))?,
            1
        );
        assert!(all_zero(&db));

        // Stale-session pruning.
        persist(&mut indexer, &new)?;
        seed_generation(&db, "ses_x");
        assert_eq!(
            indexer.prune_stale_opencode_sessions(&std::collections::HashSet::new())?,
            1
        );
        assert!(all_zero(&db));

        // Clearing the whole cache.
        persist(&mut indexer, &new)?;
        seed_generation(&db, "ses_x");
        db.execute(
            "INSERT INTO opencode_source_fingerprints
             (db_path, db_mtime_ns, db_size, wal_mtime_ns, wal_size, reader_generation)
             VALUES ('/tmp/ses_x.db', 1, 1, NULL, NULL, 2)",
            [],
        )?;
        indexer.clear_all_sessions()?;
        assert!(all_zero(&db));
        for table in ["context_entries", "opencode_source_fingerprints"] {
            assert_eq!(
                db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                    .get::<_, i64>(0))?,
                0,
                "{table} must be emptied"
            );
        }
        Ok(())
    }
}
