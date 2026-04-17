use anyhow::{Context, Result};
use rusqlite::Connection;
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::models::{AiAssistant, IndexingError, IndexingRunResult, PerSourceResult, SourceStatus};
use crate::parsers::ParsedSession;
use crate::parsers::claude_code::{ClaudeCodeParser, ParseError as ClaudeCodeParseError};
use crate::parsers::codex::{CodexParser, ParseError as CodexParseError};
use crate::parsers::mistral_vibe::{MistralVibeParser, ParseError as MistralVibeParseError};
use crate::parsers::opencode::{
    OpenCodeBackend, OpenCodeParser, ParseError as OpenCodeParseError, SessionEntry, SessionSource,
    json_backend::JsonBackend, sqlite_backend::SqliteBackend,
};
use crate::session_sources::SessionSources;

pub struct SessionIndexer {
    db: Connection,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct IndexingStats {
    pub indexed: usize,
    pub skipped: usize,
    pub errors: usize,
}

pub(crate) fn derive_source_status(
    source_available: bool,
    indexed: usize,
    skipped: usize,
    errors: usize,
) -> SourceStatus {
    // Count both newly indexed and skipped sessions as "processed":
    // in incremental runs, skipped means the source already contains
    // valid sessions that were up to date, so it should still be
    // reported as Indexed/Degraded rather than Empty.
    let processed = indexed + skipped;
    match (source_available, processed, errors) {
        (false, _, _) => SourceStatus::NotFound,
        (true, 0, 0) => SourceStatus::Empty,
        (true, n, 0) if n > 0 => SourceStatus::Indexed,
        (true, n, e) if n > 0 && e > 0 => SourceStatus::Degraded,
        (true, 0, e) if e > 0 => SourceStatus::Failed,
        _ => SourceStatus::Empty,
    }
}

pub(crate) fn opencode_source_available(storage_root: &Path, db_paths: &[PathBuf]) -> bool {
    storage_root.exists() || db_paths.iter().any(|path| path.exists())
}

fn opencode_display_path(storage_root: &Path, db_paths: &[PathBuf]) -> String {
    if storage_root.exists() {
        storage_root.display().to_string()
    } else if let Some(db) = db_paths.first() {
        db.display().to_string()
    } else {
        storage_root.display().to_string()
    }
}

fn build_per_source_result(
    assistant: AiAssistant,
    display_path: String,
    source_available: bool,
    stats: IndexingStats,
) -> PerSourceResult {
    PerSourceResult {
        assistant,
        display_path,
        indexed: stats.indexed,
        skipped: stats.skipped,
        errors: stats.errors,
        status: derive_source_status(source_available, stats.indexed, stats.skipped, stats.errors),
    }
}

const MAX_INDEXING_ERRORS: usize = 50;
const SESSION_UPSERT_SQL: &str = "INSERT INTO sessions
             (id, tool, project_path, project_id, start_time, message_count, file_path, last_updated,
              first_prompt, parent_session_id, is_subagent,
              input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens,
              edit_count, read_count, command_count, ending_status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                     ?17, ?18, ?19, ?20)
             ON CONFLICT(id) DO UPDATE SET
                 tool = excluded.tool,
                 project_path = excluded.project_path,
                 project_id = excluded.project_id,
                 start_time = excluded.start_time,
                 message_count = excluded.message_count,
                 file_path = excluded.file_path,
                 last_updated = excluded.last_updated,
                 first_prompt = excluded.first_prompt,
                 parent_session_id = excluded.parent_session_id,
                 is_subagent = excluded.is_subagent,
                 input_tokens = excluded.input_tokens,
                 output_tokens = excluded.output_tokens,
                 cache_read_tokens = excluded.cache_read_tokens,
                 cache_write_tokens = excluded.cache_write_tokens,
                 reasoning_tokens = excluded.reasoning_tokens,
                 edit_count = excluded.edit_count,
                 read_count = excluded.read_count,
                 command_count = excluded.command_count,
                 ending_status = excluded.ending_status";

#[derive(Debug, Default, Clone, Copy)]
struct OpencodeEnumerationFlags {
    enumeration_succeeded: bool,
    sqlite_enumerated: bool,
}

struct OpencodeIndexContext<'a> {
    parser: &'a OpenCodeParser,
    incremental: bool,
    indexed_ids: &'a mut HashSet<String>,
    flags: &'a mut OpencodeEnumerationFlags,
    stats: &'a mut IndexingStats,
    errors_detail: &'a mut VecDeque<IndexingError>,
}

fn push_indexing_error(
    errors_detail: &mut VecDeque<IndexingError>,
    assistant: AiAssistant,
    location: Option<String>,
    message: impl Into<String>,
) {
    if errors_detail.len() >= MAX_INDEXING_ERRORS {
        errors_detail.pop_front();
    }

    errors_detail.push_back(IndexingError {
        assistant,
        location,
        message: message.into(),
    });
}

fn is_opencode_error(err: &anyhow::Error) -> bool {
    err.downcast_ref::<OpenCodeParseError>().is_some()
}

fn is_codex_error(err: &anyhow::Error) -> bool {
    err.downcast_ref::<CodexParseError>().is_some()
}

fn is_claude_empty_session_error(err: &anyhow::Error) -> bool {
    matches!(
        err.downcast_ref::<ClaudeCodeParseError>(),
        Some(ClaudeCodeParseError::NoMessages | ClaudeCodeParseError::NoUserMessages)
    )
}

impl SessionIndexer {
    pub fn new(db_path: &Path) -> Result<Self> {
        let db = crate::database::open_connection(db_path)?;
        db.pragma_update(None, "journal_mode", "WAL")
            .context("Failed to enable WAL mode")?;
        crate::database::schema::initialize_database(&db)
            .context("Failed to initialize database schema")?;
        Ok(Self { db })
    }

    #[allow(dead_code)]
    pub fn index_claude_sessions(&mut self, sessions_dir: &Path) -> Result<usize> {
        let mut errors_detail = VecDeque::new();
        Ok(self
            .index_claude_sessions_internal(sessions_dir, false, &mut errors_detail)?
            .indexed)
    }

    fn index_claude_sessions_internal(
        &mut self,
        sessions_dir: &Path,
        incremental: bool,
        errors_detail: &mut VecDeque<IndexingError>,
    ) -> Result<IndexingStats> {
        let parser = ClaudeCodeParser;
        let mut stats = IndexingStats::default();

        for entry in walkdir::WalkDir::new(sessions_dir)
            .max_depth(5)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if !Self::is_claude_session_file(path) {
                continue;
            }

            if Self::is_prunable_claude_sidechain_file(path, sessions_dir) {
                self.prune_sidechain_session(AiAssistant::ClaudeCode, path, errors_detail);
                continue;
            }

            if incremental && !self.should_reindex(path)? {
                stats.skipped += 1;
                continue;
            }

            self.process_claude_session_file(path, &parser, &mut stats, errors_detail);
        }

        self.prune_orphan_fingerprints()?;
        Ok(stats)
    }

    #[allow(dead_code)]
    pub fn index_opencode_sessions(
        &mut self,
        storage_root: &Path,
        db_paths: &[PathBuf],
    ) -> Result<usize> {
        let mut errors_detail = VecDeque::new();
        Ok(self
            .index_opencode_sessions_internal(storage_root, db_paths, false, &mut errors_detail)?
            .indexed)
    }

    fn index_opencode_sessions_internal(
        &mut self,
        storage_root: &Path,
        db_paths: &[PathBuf],
        incremental: bool,
        errors_detail: &mut VecDeque<IndexingError>,
    ) -> Result<IndexingStats> {
        let has_storage_root = storage_root.exists();
        let has_db = db_paths.iter().any(|path| path.exists());

        if !has_storage_root && !has_db {
            return Ok(IndexingStats::default());
        }

        let parser = OpenCodeParser::new(storage_root);
        let mut indexed_ids: HashSet<String> = HashSet::new();
        let mut stats = IndexingStats::default();
        let mut flags = OpencodeEnumerationFlags::default();
        let mut context = OpencodeIndexContext {
            parser: &parser,
            incremental,
            indexed_ids: &mut indexed_ids,
            flags: &mut flags,
            stats: &mut stats,
            errors_detail,
        };

        self.index_opencode_sqlite_sources(db_paths, &mut context)?;

        if has_storage_root {
            self.index_opencode_json_sessions(storage_root, &mut context)?;
        }

        self.prune_stale_opencode_sessions_if_needed(incremental, flags, &indexed_ids)?;

        self.prune_orphan_fingerprints()?;

        Ok(stats)
    }

    #[allow(dead_code)]
    pub fn index_codex_sessions(&mut self, sessions_dir: &Path) -> Result<usize> {
        let mut errors_detail = VecDeque::new();
        Ok(self
            .index_codex_sessions_internal(sessions_dir, false, &mut errors_detail)?
            .indexed)
    }

    fn index_codex_sessions_internal(
        &mut self,
        sessions_dir: &Path,
        incremental: bool,
        errors_detail: &mut VecDeque<IndexingError>,
    ) -> Result<IndexingStats> {
        if !sessions_dir.exists() {
            return Ok(IndexingStats::default());
        }

        let parser = CodexParser;
        let mut stats = IndexingStats::default();

        for entry in walkdir::WalkDir::new(sessions_dir)
            .max_depth(5)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if !Self::is_codex_session_file(path) {
                continue;
            }

            if incremental && !self.should_reindex(path)? {
                stats.skipped += 1;
                continue;
            }

            self.process_codex_session_file(path, &parser, &mut stats, errors_detail);
        }

        self.prune_orphan_fingerprints()?;
        Ok(stats)
    }

    #[allow(dead_code)]
    pub fn index_vibe_sessions(&mut self, sessions_dir: &Path) -> Result<usize> {
        let mut errors_detail = VecDeque::new();
        Ok(self
            .index_vibe_sessions_internal(sessions_dir, false, &mut errors_detail)?
            .indexed)
    }

    fn index_vibe_sessions_internal(
        &mut self,
        sessions_dir: &Path,
        incremental: bool,
        errors_detail: &mut VecDeque<IndexingError>,
    ) -> Result<IndexingStats> {
        if !sessions_dir.exists() {
            return Ok(IndexingStats::default());
        }

        let parser = MistralVibeParser;
        let mut stats = IndexingStats::default();

        let entries = std::fs::read_dir(sessions_dir)
            .with_context(|| format!("Failed to read {}", sessions_dir.display()))?;

        for entry in entries {
            let Some((path, fingerprint_target)) =
                self.next_vibe_session_path(entry, sessions_dir, errors_detail)?
            else {
                continue;
            };
            if incremental && !self.should_reindex(&fingerprint_target)? {
                stats.skipped += 1;
                continue;
            }

            self.process_vibe_session_dir(
                &path,
                &fingerprint_target,
                &parser,
                &mut stats,
                errors_detail,
            )?;
        }

        self.prune_orphan_fingerprints()?;
        Ok(stats)
    }

    fn process_vibe_session_dir(
        &mut self,
        path: &Path,
        fingerprint_target: &Path,
        parser: &MistralVibeParser,
        stats: &mut IndexingStats,
        errors_detail: &mut VecDeque<IndexingError>,
    ) -> Result<()> {
        match parser.parse(path) {
            Ok(parsed) => {
                self.insert_parsed_session_with_fingerprint(&parsed, path, fingerprint_target)?;
                stats.indexed += 1;
            }
            Err(err) => {
                if matches!(
                    err.downcast_ref::<MistralVibeParseError>(),
                    Some(MistralVibeParseError::NoUserMessages)
                ) {
                    self.prune_session_after_parse_skip(
                        AiAssistant::MistralVibe,
                        path,
                        errors_detail,
                    );
                } else {
                    self.record_index_failure(
                        AiAssistant::MistralVibe,
                        path,
                        &err,
                        stats,
                        errors_detail,
                    );
                }
            }
        }

        Ok(())
    }

    fn process_claude_session_file(
        &mut self,
        path: &Path,
        parser: &ClaudeCodeParser,
        stats: &mut IndexingStats,
        errors_detail: &mut VecDeque<IndexingError>,
    ) {
        match self.index_session_file(path, parser) {
            Ok(()) => stats.indexed += 1,
            Err(err) => {
                if is_claude_empty_session_error(&err) {
                    tracing::debug!(
                        "Skipped empty Claude Code session {}: {}",
                        path.display(),
                        err
                    );
                    self.prune_session_after_parse_skip(
                        AiAssistant::ClaudeCode,
                        path,
                        errors_detail,
                    );
                } else {
                    self.record_index_failure(
                        AiAssistant::ClaudeCode,
                        path,
                        &err,
                        stats,
                        errors_detail,
                    );
                }
            }
        }
    }

    fn process_codex_session_file(
        &mut self,
        path: &Path,
        parser: &CodexParser,
        stats: &mut IndexingStats,
        errors_detail: &mut VecDeque<IndexingError>,
    ) {
        match self.index_codex_session_file(path, parser) {
            Ok(()) => stats.indexed += 1,
            Err(err) => {
                if is_codex_error(&err) {
                    tracing::debug!("Skipped Codex session {}: {}", path.display(), err);
                    self.prune_session_after_parse_skip(AiAssistant::Codex, path, errors_detail);
                } else {
                    self.record_index_failure(AiAssistant::Codex, path, &err, stats, errors_detail);
                }
            }
        }
    }

    fn next_vibe_session_path(
        &self,
        entry: std::io::Result<std::fs::DirEntry>,
        sessions_dir: &Path,
        errors_detail: &mut VecDeque<IndexingError>,
    ) -> Result<Option<(PathBuf, PathBuf)>> {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                tracing::warn!("Failed to read Mistral Vibe session entry: {}", err);
                push_indexing_error(
                    errors_detail,
                    AiAssistant::MistralVibe,
                    Some(sessions_dir.display().to_string()),
                    format!("Failed to read Mistral Vibe session entry: {err}"),
                );
                return Ok(None);
            }
        };

        let path = entry.path();
        let fingerprint_target = path.join("messages.jsonl");
        if !path.is_dir() || !path.join("meta.json").exists() || !fingerprint_target.exists() {
            return Ok(None);
        }

        Ok(Some((path, fingerprint_target)))
    }

    fn is_claude_session_file(path: &Path) -> bool {
        path.is_file() && path.extension().is_some_and(|ext| ext == "jsonl")
    }

    fn is_codex_session_file(path: &Path) -> bool {
        path.is_file()
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("rollout-") && name.ends_with(".jsonl"))
    }

    fn prune_sidechain_session(
        &mut self,
        assistant: AiAssistant,
        path: &Path,
        errors_detail: &mut VecDeque<IndexingError>,
    ) {
        if let Err(err) = self.remove_session_for_file(path) {
            tracing::warn!(
                "Failed to prune sidechain session {}: {}",
                path.display(),
                err
            );
            push_indexing_error(
                errors_detail,
                assistant,
                Some(path.display().to_string()),
                format!("Failed to prune sidechain session: {err}"),
            );
        }
    }

    fn record_index_failure(
        &self,
        assistant: AiAssistant,
        path: &Path,
        err: &anyhow::Error,
        stats: &mut IndexingStats,
        errors_detail: &mut VecDeque<IndexingError>,
    ) {
        tracing::warn!("Failed to index {}: {}", path.display(), err);
        push_indexing_error(
            errors_detail,
            assistant,
            Some(path.display().to_string()),
            format!("Failed to index session: {err}"),
        );
        stats.errors += 1;
    }

    fn index_session_file(&mut self, file_path: &Path, parser: &ClaudeCodeParser) -> Result<()> {
        let parsed = parser.parse(file_path)?;
        self.insert_parsed_session(&parsed, file_path)?;
        Ok(())
    }

    fn index_opencode_session_file(
        &mut self,
        file_path: &Path,
        parser: &OpenCodeParser,
    ) -> Result<()> {
        let parsed = parser.parse(file_path)?;
        self.insert_parsed_session(&parsed, file_path)?;
        Ok(())
    }

    fn index_codex_session_file(&mut self, file_path: &Path, parser: &CodexParser) -> Result<()> {
        let parsed = parser.parse(file_path)?;
        self.insert_parsed_session(&parsed, file_path)?;
        Ok(())
    }

    fn index_opencode_sqlite_sources(
        &mut self,
        db_paths: &[PathBuf],
        context: &mut OpencodeIndexContext<'_>,
    ) -> Result<()> {
        for db_path in db_paths {
            self.index_opencode_sqlite_source(db_path, context)?;
        }

        Ok(())
    }

    fn index_opencode_sqlite_source(
        &mut self,
        db_path: &Path,
        context: &mut OpencodeIndexContext<'_>,
    ) -> Result<()> {
        if context.incremental && !self.should_reindex_opencode_sqlite(db_path)? {
            context.stats.skipped += 1;
            return Ok(());
        }

        match SqliteBackend::open(db_path) {
            Ok(sqlite_backend) => {
                self.index_opencode_sqlite_backend(db_path, &sqlite_backend, context)
            }
            Err(err) => {
                tracing::warn!(
                    "Failed to open OpenCode DB {}: {} - falling back to JSON only",
                    db_path.display(),
                    err
                );
                push_indexing_error(
                    context.errors_detail,
                    AiAssistant::OpenCode,
                    Some(db_path.display().to_string()),
                    format!("Failed to open OpenCode DB: {err}"),
                );
                context.stats.errors += 1;
                Ok(())
            }
        }
    }

    fn index_opencode_sqlite_backend(
        &mut self,
        db_path: &Path,
        sqlite_backend: &SqliteBackend,
        context: &mut OpencodeIndexContext<'_>,
    ) -> Result<()> {
        match sqlite_backend.list_sessions() {
            Ok(entries) => {
                context.flags.enumeration_succeeded = true;
                context.flags.sqlite_enumerated = true;
                for entry in &entries {
                    self.index_opencode_sqlite_entry(db_path, entry, sqlite_backend, context);
                }
            }
            Err(err) => {
                tracing::warn!("Failed to list SQLite sessions: {}", err);
                push_indexing_error(
                    context.errors_detail,
                    AiAssistant::OpenCode,
                    Some(db_path.display().to_string()),
                    format!("Failed to list SQLite sessions: {err}"),
                );
                context.stats.errors += 1;
            }
        }

        Ok(())
    }

    fn index_opencode_sqlite_entry(
        &mut self,
        db_path: &Path,
        entry: &SessionEntry,
        sqlite_backend: &SqliteBackend,
        context: &mut OpencodeIndexContext<'_>,
    ) {
        match context.parser.parse_entry(entry, sqlite_backend) {
            Ok(parsed) => {
                if let Err(err) = self.insert_parsed_session_with_fingerprint(
                    &parsed,
                    db_path,
                    &Self::opencode_sqlite_fingerprint_target(db_path),
                ) {
                    tracing::warn!("Failed to insert SQLite session {}: {}", entry.id, err);
                    push_indexing_error(
                        context.errors_detail,
                        AiAssistant::OpenCode,
                        Some(db_path.display().to_string()),
                        format!("Failed to insert SQLite session {}: {}", entry.id, err),
                    );
                    context.stats.errors += 1;
                    return;
                }

                context.indexed_ids.insert(entry.id.clone());
                context.stats.indexed += 1;
            }
            Err(err) => {
                if is_opencode_error(&err) {
                    tracing::debug!("Skipped SQLite session {}: {}", entry.id, err);
                } else {
                    tracing::warn!("Failed to parse SQLite session {}: {}", entry.id, err);
                    push_indexing_error(
                        context.errors_detail,
                        AiAssistant::OpenCode,
                        Some(db_path.display().to_string()),
                        format!("Failed to parse SQLite session {}: {}", entry.id, err),
                    );
                    context.stats.errors += 1;
                }
            }
        }
    }

    fn index_opencode_json_sessions(
        &mut self,
        storage_root: &Path,
        context: &mut OpencodeIndexContext<'_>,
    ) -> Result<()> {
        let json_backend = JsonBackend::new(storage_root);
        match json_backend.list_sessions() {
            Ok(entries) => {
                context.flags.enumeration_succeeded = true;
                for entry in entries {
                    self.index_opencode_json_entry(entry, context)?;
                }
            }
            Err(err) => {
                tracing::warn!("Failed to list JSON OpenCode sessions: {}", err);
                push_indexing_error(
                    context.errors_detail,
                    AiAssistant::OpenCode,
                    Some(storage_root.display().to_string()),
                    format!("Failed to list JSON OpenCode sessions: {err}"),
                );
                context.stats.errors += 1;
            }
        }

        Ok(())
    }

    fn index_opencode_json_entry(
        &mut self,
        entry: SessionEntry,
        context: &mut OpencodeIndexContext<'_>,
    ) -> Result<()> {
        let session_id = entry.id.clone();

        if context.indexed_ids.contains(&session_id) {
            tracing::debug!(
                "Skipping JSON session {} (already indexed from SQLite)",
                session_id
            );
            return Ok(());
        }

        let path = match &entry.source {
            SessionSource::JsonFile(path) => path,
            SessionSource::SqliteRow { .. } => return Ok(()),
        };

        if context.incremental && !self.should_reindex(path)? {
            context.indexed_ids.insert(session_id);
            context.stats.skipped += 1;
            return Ok(());
        }

        match self.index_opencode_session_file(path, context.parser) {
            Ok(()) => {
                context.indexed_ids.insert(session_id);
                context.stats.indexed += 1;
            }
            Err(err) => {
                if is_opencode_error(&err) {
                    tracing::debug!("Skipped OpenCode session {}: {}", path.display(), err);
                    self.prune_session_after_parse_skip(
                        AiAssistant::OpenCode,
                        path,
                        context.errors_detail,
                    );
                } else {
                    tracing::warn!("Failed to index {}: {}", path.display(), err);
                    push_indexing_error(
                        context.errors_detail,
                        AiAssistant::OpenCode,
                        Some(path.display().to_string()),
                        format!("Failed to index session: {err}"),
                    );
                    context.stats.errors += 1;
                }
            }
        }

        Ok(())
    }

    fn insert_parsed_session(&mut self, parsed: &ParsedSession, file_path: &Path) -> Result<()> {
        self.insert_parsed_session_with_fingerprint(parsed, file_path, file_path)
    }

    fn insert_parsed_session_with_fingerprint(
        &mut self,
        parsed: &ParsedSession,
        file_path: &Path,
        fingerprint_path: &Path,
    ) -> Result<()> {
        let session = &parsed.session;
        let tx = self.db.transaction()?;
        let resolved_project_id = Self::upsert_project_tx(&tx, session.project_path.as_deref())?;
        Self::upsert_session_row_tx(&tx, parsed, file_path, resolved_project_id)?;
        Self::replace_session_contents_tx(&tx, parsed)?;
        Self::link_claude_subagents_tx(&tx, parsed)?;
        Self::upsert_fingerprint_tx(&tx, fingerprint_path)?;
        tx.commit()?;
        Ok(())
    }

    fn link_claude_subagents_tx(
        tx: &rusqlite::Transaction<'_>,
        parsed: &ParsedSession,
    ) -> Result<()> {
        if parsed.session.tool != AiAssistant::ClaudeCode {
            return Ok(());
        }

        if parsed.session.is_subagent {
            let Some(parent_session_id) = parsed.session.parent_session_id.as_deref() else {
                return Ok(());
            };

            let agent_id = parsed
                .session
                .id
                .rsplit("::")
                .next()
                .filter(|value| !value.is_empty())
                .context("Claude child session id missing agent suffix")?;

            tx.execute(
                "UPDATE subagents
                 SET child_session_id = ?1
                 WHERE session_id = ?2 AND agent_id = ?3",
                rusqlite::params![&parsed.session.id, parent_session_id, agent_id],
            )?;

            return Ok(());
        }

        for subagent in &parsed.subagents {
            let Some(agent_id) = subagent.agent_id.as_deref() else {
                continue;
            };

            let child_session_id = format!("claude-subagent::{}::{}", parsed.session.id, agent_id);

            let child_exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM sessions WHERE id = ?1)",
                [&child_session_id],
                |row| row.get(0),
            )?;

            if child_exists {
                tx.execute(
                    "UPDATE subagents
                     SET child_session_id = ?1
                     WHERE session_id = ?2 AND id = ?3",
                    rusqlite::params![child_session_id, &parsed.session.id, &subagent.id],
                )?;
            }
        }

        Ok(())
    }

    /// Derive ending status from the last tool call in the session.
    fn determine_ending_status(
        tool_calls: &[crate::models::ToolCall],
    ) -> crate::models::SessionEndingStatus {
        match tool_calls.last() {
            None => crate::models::SessionEndingStatus::Unknown,
            Some(tc) => match tc.status {
                crate::models::ToolCallStatus::Error => crate::models::SessionEndingStatus::Error,
                crate::models::ToolCallStatus::Pending | crate::models::ToolCallStatus::Running => {
                    crate::models::SessionEndingStatus::Abrupt
                }
                crate::models::ToolCallStatus::Completed => {
                    crate::models::SessionEndingStatus::Clean
                }
                crate::models::ToolCallStatus::Unknown => {
                    crate::models::SessionEndingStatus::Unknown
                }
            },
        }
    }

    fn compute_activity_counts(tool_calls: &[crate::models::ToolCall]) -> (i64, i64, i64) {
        let mut edit_count: i64 = 0;
        let mut read_count: i64 = 0;
        let mut command_count: i64 = 0;

        for tool_call in tool_calls {
            match crate::models::classify_tool_name(&tool_call.tool_name) {
                crate::models::ToolCategory::Edit => edit_count += 1,
                crate::models::ToolCategory::Command => command_count += 1,
                crate::models::ToolCategory::Read | crate::models::ToolCategory::Search => {
                    read_count += 1
                }
                crate::models::ToolCategory::Agent
                | crate::models::ToolCategory::Web
                | crate::models::ToolCategory::Other => {}
            }
        }

        (edit_count, read_count, command_count)
    }

    fn upsert_session_row_tx(
        tx: &rusqlite::Transaction<'_>,
        parsed: &ParsedSession,
        file_path: &Path,
        resolved_project_id: Option<i64>,
    ) -> Result<()> {
        let session = &parsed.session;
        let (edit_count, read_count, command_count) =
            Self::compute_activity_counts(&parsed.tool_calls);
        let ending_status = Self::determine_ending_status(&parsed.tool_calls).to_storage();

        tx.execute(
            SESSION_UPSERT_SQL,
            rusqlite::params![
                &session.id,
                session.tool.to_storage(),
                &session.project_path,
                resolved_project_id,
                session.start_time.timestamp(),
                session.message_count as i64,
                file_path.to_str(),
                session.last_updated.timestamp(),
                &session.first_prompt,
                &session.parent_session_id,
                session.is_subagent as i64,
                parsed.token_usage.as_ref().map(|u| u.input_tokens),
                parsed.token_usage.as_ref().map(|u| u.output_tokens),
                parsed
                    .token_usage
                    .as_ref()
                    .and_then(|u| u.cache_read_tokens),
                parsed
                    .token_usage
                    .as_ref()
                    .and_then(|u| u.cache_write_tokens),
                parsed.token_usage.as_ref().and_then(|u| u.reasoning_tokens),
                edit_count,
                read_count,
                command_count,
                ending_status,
            ],
        )?;

        Ok(())
    }

    fn replace_session_contents_tx(
        tx: &rusqlite::Transaction<'_>,
        parsed: &ParsedSession,
    ) -> Result<()> {
        let session_id = &parsed.session.id;
        Self::delete_session_contents_tx(tx, session_id)?;

        for msg in &parsed.messages {
            tx.execute(
                "INSERT INTO messages (session_id, message_index, role, content, timestamp, model)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    session_id,
                    msg.index as i64,
                    format!("{:?}", msg.role).to_lowercase(),
                    &msg.content,
                    msg.timestamp.timestamp(),
                    &msg.model,
                ],
            )?;
        }

        for tool_call in &parsed.tool_calls {
            crate::database::insert_tool_call(tx, tool_call, session_id)?;
        }

        for subagent in &parsed.subagents {
            crate::database::insert_subagent(tx, subagent, session_id)?;
        }

        for item in &parsed.transcript_items {
            crate::database::insert_transcript_item(tx, item, session_id)?;
        }

        for attachment in &parsed.reasoning_attachments {
            crate::database::insert_reasoning_attachment(tx, attachment, session_id)?;
        }

        Ok(())
    }

    fn delete_session_contents_tx(tx: &rusqlite::Transaction<'_>, session_id: &str) -> Result<()> {
        tx.execute("DELETE FROM messages WHERE session_id = ?1", [session_id])?;
        tx.execute(
            "DELETE FROM transcript_items WHERE session_id = ?1",
            [session_id],
        )?;
        tx.execute(
            "DELETE FROM reasoning_attachments WHERE session_id = ?1",
            [session_id],
        )?;
        tx.execute("DELETE FROM tool_calls WHERE session_id = ?1", [session_id])?;
        tx.execute("DELETE FROM subagents WHERE session_id = ?1", [session_id])?;
        Ok(())
    }

    fn upsert_project_tx(
        tx: &rusqlite::Transaction<'_>,
        raw_project_path: Option<&str>,
    ) -> Result<Option<i64>> {
        let Some(raw_project_path) = raw_project_path else {
            return Ok(None);
        };

        let resolved_path = crate::project_resolver::resolve_project_path(raw_project_path);
        let project_name = Path::new(&resolved_path)
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or(&resolved_path)
            .to_string();

        let id = tx.query_row(
            "INSERT INTO projects (path, name) VALUES (?1, ?2)
             ON CONFLICT(path) DO UPDATE SET name = excluded.name
             RETURNING id",
            rusqlite::params![&resolved_path, project_name],
            |row| row.get(0),
        )?;

        Ok(Some(id))
    }

    fn get_fingerprint(&self, file_path: &Path) -> Result<Option<(i64, i64)>> {
        let Some(file_path_str) = file_path.to_str() else {
            return Ok(None);
        };

        let mut stmt = self
            .db
            .prepare("SELECT mtime_ns, size FROM file_fingerprints WHERE file_path = ?1")?;
        let mut rows = stmt.query([file_path_str])?;

        if let Some(row) = rows.next()? {
            let mtime_ns = row.get(0)?;
            let size = row.get(1)?;
            Ok(Some((mtime_ns, size)))
        } else {
            Ok(None)
        }
    }

    fn current_fingerprint(file_path: &Path) -> Result<(i64, i64)> {
        let metadata = fs::metadata(file_path)?;
        let modified = metadata
            .modified()?
            .duration_since(UNIX_EPOCH)
            .context("file timestamp predates unix epoch")?;

        let mtime_ns = i64::try_from(modified.as_nanos()).context("mtime nanoseconds overflow")?;
        let size = i64::try_from(metadata.len()).context("file size overflow")?;
        Ok((mtime_ns, size))
    }

    fn should_reindex(&self, file_path: &Path) -> Result<bool> {
        let current = Self::current_fingerprint(file_path)?;
        match self.get_fingerprint(file_path)? {
            Some(stored) if stored == current => Ok(false),
            _ => Ok(true),
        }
    }

    fn should_reindex_opencode_sqlite(&self, db_path: &Path) -> Result<bool> {
        if self.should_reindex(db_path)? {
            return Ok(true);
        }

        let wal_path = Self::opencode_sqlite_wal_path(db_path);
        if wal_path.exists() {
            return self.should_reindex(&wal_path);
        }

        Ok(self.get_fingerprint(&wal_path)?.is_some())
    }

    fn opencode_sqlite_wal_path(db_path: &Path) -> PathBuf {
        let mut wal = db_path.as_os_str().to_os_string();
        wal.push("-wal");
        wal.into()
    }

    fn opencode_sqlite_fingerprint_target(db_path: &Path) -> PathBuf {
        let wal_path = Self::opencode_sqlite_wal_path(db_path);
        if wal_path.exists() {
            wal_path
        } else {
            db_path.to_path_buf()
        }
    }

    fn upsert_fingerprint_tx(tx: &rusqlite::Transaction<'_>, file_path: &Path) -> Result<()> {
        let Some(file_path_str) = file_path.to_str() else {
            return Ok(());
        };

        let (mtime_ns, size) = Self::current_fingerprint(file_path)?;
        tx.execute(
            "INSERT INTO file_fingerprints (file_path, mtime_ns, size)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(file_path) DO UPDATE SET
               mtime_ns = excluded.mtime_ns,
               size = excluded.size",
            rusqlite::params![file_path_str, mtime_ns, size],
        )?;
        Ok(())
    }

    #[cfg(test)]
    fn upsert_fingerprint_for_file(&mut self, file_path: &Path) -> Result<()> {
        let tx = self.db.transaction()?;
        Self::upsert_fingerprint_tx(&tx, file_path)?;
        tx.commit()?;
        Ok(())
    }

    fn prune_orphan_fingerprints(&mut self) -> Result<usize> {
        let file_paths: Vec<String> = {
            let mut stmt = self.db.prepare("SELECT file_path FROM file_fingerprints")?;
            stmt.query_map([], |row| row.get(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };

        let tx = self.db.transaction()?;
        let mut removed = 0usize;

        for file_path in file_paths {
            if !Path::new(&file_path).exists() {
                removed += tx.execute(
                    "DELETE FROM file_fingerprints WHERE file_path = ?1",
                    [file_path],
                )?;
            }
        }

        tx.commit()?;
        Ok(removed)
    }

    fn prune_stale_opencode_sessions_if_needed(
        &mut self,
        incremental: bool,
        flags: OpencodeEnumerationFlags,
        indexed_ids: &HashSet<String>,
    ) -> Result<()> {
        if incremental {
            if flags.sqlite_enumerated {
                self.prune_stale_opencode_sessions(indexed_ids)?;
            }
        } else if flags.enumeration_succeeded {
            self.prune_stale_opencode_sessions(indexed_ids)?;
        }

        Ok(())
    }

    fn prune_session_after_parse_skip(
        &mut self,
        assistant: AiAssistant,
        path: &Path,
        errors_detail: &mut VecDeque<IndexingError>,
    ) {
        if let Err(remove_err) = self.remove_session_for_file(path) {
            tracing::warn!("Failed to prune session {}: {}", path.display(), remove_err);
            push_indexing_error(
                errors_detail,
                assistant,
                Some(path.display().to_string()),
                format!("Failed to prune session: {remove_err}"),
            );
        }
    }

    fn is_prunable_claude_sidechain_file(file_path: &Path, sessions_dir: &Path) -> bool {
        let relative = match file_path.strip_prefix(sessions_dir) {
            Ok(relative) => relative,
            Err(_) => return false,
        };

        let components: Vec<_> = relative.components().collect();
        let has_agent_prefix = relative
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| stem.starts_with("agent-"));

        let nested_subagent = components.len() >= 3
            && components[components.len() - 2].as_os_str() == "subagents"
            && !components
                .first()
                .is_some_and(|component| component.as_os_str() == "subagents")
            && has_agent_prefix;

        if nested_subagent {
            return false;
        }

        if components
            .first()
            .is_some_and(|component| component.as_os_str() == "subagents")
        {
            return true;
        }

        has_agent_prefix
    }

    /// Clear all indexed sessions and messages.
    ///
    /// Note: `messages` is an FTS5 virtual table. Standard `DELETE FROM` works
    /// correctly on FTS5 tables and participates in transactions normally.
    pub fn clear_all_sessions(&mut self) -> Result<()> {
        let tx = self.db.transaction()?;
        tx.execute("DELETE FROM reasoning_attachments", [])?;
        tx.execute("DELETE FROM transcript_items", [])?;
        tx.execute("DELETE FROM tool_calls", [])?;
        tx.execute("DELETE FROM subagents", [])?;
        tx.execute("DELETE FROM messages", [])?;
        tx.execute("DELETE FROM sessions", [])?;
        tx.execute("DELETE FROM file_fingerprints", [])?;
        tx.commit()?;
        Ok(())
    }

    pub fn index_all_incremental(&mut self, sources: &SessionSources) -> Result<IndexingRunResult> {
        let mut errors_detail = VecDeque::new();

        let claude =
            self.index_claude_sessions_internal(&sources.claude_dir, true, &mut errors_detail)?;
        let opencode = self.index_opencode_sessions_internal(
            &sources.opencode_storage_root,
            &sources.opencode_db_paths,
            true,
            &mut errors_detail,
        )?;
        let codex =
            self.index_codex_sessions_internal(&sources.codex_dir, true, &mut errors_detail)?;
        let vibe =
            self.index_vibe_sessions_internal(&sources.vibe_dir, true, &mut errors_detail)?;

        Ok(Self::build_indexing_run_result(
            sources,
            claude,
            opencode,
            codex,
            vibe,
            errors_detail,
        ))
    }

    pub fn index_all_full_reindex(
        &mut self,
        sources: &SessionSources,
    ) -> Result<IndexingRunResult> {
        self.clear_all_sessions()?;

        let mut errors_detail = VecDeque::new();

        let claude =
            self.index_claude_sessions_internal(&sources.claude_dir, false, &mut errors_detail)?;
        let opencode = self.index_opencode_sessions_internal(
            &sources.opencode_storage_root,
            &sources.opencode_db_paths,
            false,
            &mut errors_detail,
        )?;
        let codex =
            self.index_codex_sessions_internal(&sources.codex_dir, false, &mut errors_detail)?;
        let vibe =
            self.index_vibe_sessions_internal(&sources.vibe_dir, false, &mut errors_detail)?;

        Ok(Self::build_indexing_run_result(
            sources,
            claude,
            opencode,
            codex,
            vibe,
            errors_detail,
        ))
    }

    fn build_indexing_run_result(
        sources: &SessionSources,
        claude: IndexingStats,
        opencode: IndexingStats,
        codex: IndexingStats,
        vibe: IndexingStats,
        errors_detail: VecDeque<IndexingError>,
    ) -> IndexingRunResult {
        let per_source = vec![
            build_per_source_result(
                AiAssistant::ClaudeCode,
                sources.claude_dir.display().to_string(),
                sources.claude_dir.exists(),
                claude,
            ),
            build_per_source_result(
                AiAssistant::OpenCode,
                opencode_display_path(&sources.opencode_storage_root, &sources.opencode_db_paths),
                opencode_source_available(
                    &sources.opencode_storage_root,
                    &sources.opencode_db_paths,
                ),
                opencode,
            ),
            build_per_source_result(
                AiAssistant::Codex,
                sources.codex_dir.display().to_string(),
                sources.codex_dir.exists(),
                codex,
            ),
            build_per_source_result(
                AiAssistant::MistralVibe,
                sources.vibe_dir.display().to_string(),
                sources.vibe_dir.exists(),
                vibe,
            ),
        ];

        let totals = per_source
            .iter()
            .fold(IndexingStats::default(), |mut acc, result| {
                acc.indexed += result.indexed;
                acc.skipped += result.skipped;
                acc.errors += result.errors;
                acc
            });

        IndexingRunResult {
            totals,
            per_source,
            errors_detail: errors_detail.into(),
        }
    }

    fn remove_session_for_file(&mut self, file_path: &Path) -> Result<()> {
        let Some(file_path_str) = file_path.to_str() else {
            tracing::warn!("Cannot prune session with non-UTF8 path: {:?}", file_path);
            return Ok(());
        };

        let tx = self.db.transaction()?;

        tx.execute(
            "DELETE FROM transcript_items WHERE session_id IN (SELECT id FROM sessions WHERE file_path = ?1)",
            [file_path_str],
        )?;
        tx.execute(
            "DELETE FROM reasoning_attachments WHERE session_id IN (SELECT id FROM sessions WHERE file_path = ?1)",
            [file_path_str],
        )?;
        tx.execute(
            "DELETE FROM tool_calls WHERE session_id IN (SELECT id FROM sessions WHERE file_path = ?1)",
            [file_path_str],
        )?;
        tx.execute(
            "DELETE FROM subagents WHERE session_id IN (SELECT id FROM sessions WHERE file_path = ?1)",
            [file_path_str],
        )?;
        tx.execute(
            "DELETE FROM messages WHERE session_id IN (SELECT id FROM sessions WHERE file_path = ?1)",
            [file_path_str],
        )?;
        tx.execute("DELETE FROM sessions WHERE file_path = ?1", [file_path_str])?;

        tx.commit()?;

        Ok(())
    }

    fn prune_stale_opencode_sessions(&mut self, indexed_ids: &HashSet<String>) -> Result<()> {
        let existing_ids: Vec<String> = {
            let mut stmt = self
                .db
                .prepare("SELECT id FROM sessions WHERE tool = 'opencode'")?;
            stmt.query_map([], |row| row.get(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };

        for id in existing_ids {
            if !indexed_ids.contains(&id) {
                self.remove_session_by_id(&id)?;
            }
        }

        Ok(())
    }

    fn remove_session_by_id(&mut self, session_id: &str) -> Result<()> {
        let tx = self.db.transaction()?;
        tx.execute(
            "DELETE FROM transcript_items WHERE session_id = ?1",
            [session_id],
        )?;
        tx.execute(
            "DELETE FROM reasoning_attachments WHERE session_id = ?1",
            [session_id],
        )?;
        tx.execute("DELETE FROM tool_calls WHERE session_id = ?1", [session_id])?;
        tx.execute("DELETE FROM subagents WHERE session_id = ?1", [session_id])?;
        tx.execute("DELETE FROM messages WHERE session_id = ?1", [session_id])?;
        tx.execute("DELETE FROM sessions WHERE id = ?1", [session_id])?;
        tx.commit()?;
        Ok(())
    }
}

/// Test-only convenience wrappers that discard `errors_detail`.
/// Production code uses `index_all_incremental` / `index_all_full_reindex`
/// which propagate errors through `IndexingRunResult`.
#[cfg(test)]
impl SessionIndexer {
    pub fn index_claude_sessions_incremental(
        &mut self,
        sessions_dir: &Path,
    ) -> Result<IndexingStats> {
        let mut errors_detail = VecDeque::new();
        self.index_claude_sessions_internal(sessions_dir, true, &mut errors_detail)
    }

    pub fn index_opencode_sessions_incremental(
        &mut self,
        storage_root: &Path,
        db_paths: &[PathBuf],
    ) -> Result<IndexingStats> {
        let mut errors_detail = VecDeque::new();
        self.index_opencode_sessions_internal(storage_root, db_paths, true, &mut errors_detail)
    }

    pub fn index_codex_sessions_incremental(
        &mut self,
        sessions_dir: &Path,
    ) -> Result<IndexingStats> {
        let mut errors_detail = VecDeque::new();
        self.index_codex_sessions_internal(sessions_dir, true, &mut errors_detail)
    }

    pub fn index_vibe_sessions_incremental(
        &mut self,
        sessions_dir: &Path,
    ) -> Result<IndexingStats> {
        let mut errors_detail = VecDeque::new();
        self.index_vibe_sessions_internal(sessions_dir, true, &mut errors_detail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    fn create_opencode_sqlite_db(db_path: &std::path::Path) -> Connection {
        let conn = Connection::open(db_path).unwrap();
        conn.pragma_update(None, "journal_mode", "WAL").unwrap();
        conn.pragma_update(None, "wal_autocheckpoint", 0).unwrap();

        conn.execute_batch(
            "
            CREATE TABLE session (
                id TEXT PRIMARY KEY,
                directory TEXT,
                title TEXT,
                parent_id TEXT,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL
            );
            CREATE TABLE message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                data TEXT NOT NULL
            );
            CREATE TABLE part (
                id TEXT PRIMARY KEY,
                message_id TEXT NOT NULL,
                data TEXT NOT NULL
            );
            ",
        )
        .unwrap();

        conn
    }

    fn insert_opencode_session(conn: &Connection, session_id: &str, ts_ms: i64) {
        let msg_id = format!("msg-{}", session_id);
        let part_id = format!("prt-{}", session_id);

        conn.execute(
            "INSERT INTO session (id, directory, title, parent_id, time_created, time_updated)
             VALUES (?1, ?2, ?3, NULL, ?4, ?5)",
            rusqlite::params![
                session_id,
                "/tmp/project",
                format!("Session {}", session_id),
                ts_ms,
                ts_ms,
            ],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO message (id, session_id, time_created, data)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![msg_id, session_id, ts_ms, r#"{"role":"user"}"#],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO part (id, message_id, data)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![
                part_id,
                format!("msg-{}", session_id),
                r#"{"type":"text","order":1,"text":"hello"}"#
            ],
        )
        .unwrap();
    }

    fn parsed_session(session_id: &str, project_path: Option<&str>) -> ParsedSession {
        let now = chrono::Utc::now();

        ParsedSession {
            session: crate::models::Session {
                id: session_id.to_string(),
                tool: crate::models::AiAssistant::ClaudeCode,
                project_path: project_path.map(str::to_string),
                project_id: None,
                start_time: now,
                message_count: 1,
                file_path: format!("/tmp/{}.jsonl", session_id),
                last_updated: now,
                pinned_at: None,
                first_prompt: Some("hello".to_string()),
                parent_session_id: None,
                is_subagent: false,
                token_usage: None,
                edit_count: 0,
                read_count: 0,
                command_count: 0,
                ending_status: crate::models::SessionEndingStatus::Unknown,
            },
            messages: vec![],
            tool_calls: vec![],
            subagents: vec![],
            transcript_items: vec![],
            reasoning_attachments: vec![],
            token_usage: None,
        }
    }

    #[test]
    fn insert_parsed_session_leaves_project_id_null_when_project_path_is_missing() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let fingerprint = NamedTempFile::new().unwrap();
        let parsed = parsed_session("no-project", None);

        indexer
            .insert_parsed_session_with_fingerprint(&parsed, fingerprint.path(), fingerprint.path())
            .unwrap();

        let project_id: Option<i64> = indexer
            .db
            .query_row(
                "SELECT project_id FROM sessions WHERE id = 'no-project'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(project_id, None);
    }

    #[test]
    fn insert_parsed_session_reuses_project_row_for_same_repo_root() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");

        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::create_dir_all(repo.join("src/generated")).unwrap();
        std::fs::create_dir(repo.join(".git")).unwrap();

        let first_file = NamedTempFile::new().unwrap();
        let second_file = NamedTempFile::new().unwrap();
        let first = parsed_session("one", Some(repo.join("src").to_str().unwrap()));
        let second = parsed_session("two", Some(repo.join("src/generated").to_str().unwrap()));

        indexer
            .insert_parsed_session_with_fingerprint(&first, first_file.path(), first_file.path())
            .unwrap();
        indexer
            .insert_parsed_session_with_fingerprint(&second, second_file.path(), second_file.path())
            .unwrap();

        let project_count: i64 = indexer
            .db
            .query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
            .unwrap();
        assert_eq!(project_count, 1);

        let project_ids: Vec<Option<i64>> = indexer
            .db
            .prepare("SELECT project_id FROM sessions WHERE id IN ('one', 'two') ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(project_ids.len(), 2);
        assert!(project_ids[0].is_some());
        assert_eq!(project_ids[0], project_ids[1]);
    }

    #[test]
    fn new_indexer_configures_wal_and_busy_timeout() {
        let temp_db = NamedTempFile::new().unwrap();
        let indexer = SessionIndexer::new(temp_db.path()).unwrap();

        let journal_mode: String = indexer
            .db
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(journal_mode.to_ascii_lowercase(), "wal");

        let busy_timeout_ms: i64 = indexer
            .db
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .unwrap();
        assert_eq!(busy_timeout_ms, 5_000);
    }

    #[test]
    fn should_reindex_uses_mtime_and_size_fingerprint() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        let session_file = temp_dir.path().join("session.jsonl");

        std::fs::write(&session_file, "{}\n").unwrap();
        assert!(indexer.should_reindex(&session_file).unwrap());

        indexer.upsert_fingerprint_for_file(&session_file).unwrap();
        assert!(!indexer.should_reindex(&session_file).unwrap());

        std::fs::write(&session_file, "{}\n{}\n").unwrap();
        assert!(indexer.should_reindex(&session_file).unwrap());
    }

    #[test]
    fn prune_orphan_fingerprints_removes_missing_paths() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        let session_file = temp_dir.path().join("session.jsonl");

        std::fs::write(&session_file, "{}\n").unwrap();
        indexer.upsert_fingerprint_for_file(&session_file).unwrap();

        std::fs::remove_file(&session_file).unwrap();

        let removed = indexer.prune_orphan_fingerprints().unwrap();
        assert_eq!(removed, 1);
    }

    #[test]
    fn claude_incremental_skips_unchanged_files() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let sessions_dir = PathBuf::from("tests/fixtures/claude_sessions");

        let first = indexer
            .index_claude_sessions_incremental(&sessions_dir)
            .unwrap();
        assert!(first.indexed > 0);

        let second = indexer
            .index_claude_sessions_incremental(&sessions_dir)
            .unwrap();
        assert_eq!(second.indexed, 0);
        assert!(second.skipped > 0);
    }

    #[test]
    fn codex_incremental_skips_unchanged_rollouts() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let sessions_dir = PathBuf::from("tests/fixtures/codex_sessions");

        let first = indexer
            .index_codex_sessions_incremental(&sessions_dir)
            .unwrap();
        assert!(first.indexed > 0);

        let second = indexer
            .index_codex_sessions_incremental(&sessions_dir)
            .unwrap();
        assert_eq!(second.indexed, 0);
        assert!(second.skipped > 0);
    }

    #[test]
    fn vibe_incremental_uses_messages_jsonl_fingerprint() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let sessions_dir = PathBuf::from("tests/fixtures/vibe_sessions");

        let first = indexer
            .index_vibe_sessions_incremental(&sessions_dir)
            .unwrap();
        assert!(first.indexed > 0);

        let second = indexer
            .index_vibe_sessions_incremental(&sessions_dir)
            .unwrap();
        assert_eq!(second.indexed, 0);
        assert!(second.skipped > 0);
    }

    #[test]
    fn clear_all_sessions_also_clears_fingerprints() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let sessions_dir = PathBuf::from("tests/fixtures/claude_sessions");

        indexer
            .index_claude_sessions_incremental(&sessions_dir)
            .unwrap();

        indexer.clear_all_sessions().unwrap();

        let fingerprint_count: i64 = indexer
            .db
            .query_row("SELECT COUNT(*) FROM file_fingerprints", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(fingerprint_count, 0);
    }

    #[test]
    fn clear_all_sessions_removes_reasoning_attachments() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();

        indexer
            .db
            .execute(
                "INSERT INTO sessions (id, tool, start_time, message_count, file_path, last_updated)
                 VALUES ('s1', 'claude_code', 0, 1, '/tmp/session.jsonl', 0)",
                [],
            )
            .unwrap();
        indexer
            .db
            .execute(
                "INSERT INTO reasoning_attachments (session_id, transcript_item_index, visible_text)
                 VALUES ('s1', 0, 'reasoning')",
                [],
            )
            .unwrap();

        indexer.clear_all_sessions().unwrap();

        let count: i64 = indexer
            .db
            .query_row("SELECT COUNT(*) FROM reasoning_attachments", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn opencode_incremental_skip_keeps_sqlite_only_sessions() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let storage_root = PathBuf::from("tests/fixtures/opencode_storage");
        let db_path = storage_root.join("opencode.db");

        let first = indexer
            .index_opencode_sessions_incremental(&storage_root, &[db_path.clone()])
            .unwrap();
        assert!(first.indexed > 0);

        let second = indexer
            .index_opencode_sessions_incremental(&storage_root, &[db_path.clone()])
            .unwrap();
        assert!(second.skipped > 0);

        let sqlite_only_exists: bool = indexer
            .db
            .query_row(
                "SELECT COUNT(*) > 0 FROM sessions WHERE id = 'session-sqlite-only'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(sqlite_only_exists);
    }

    #[test]
    fn opencode_incremental_detects_new_rows_in_sqlite_wal() {
        let temp_app_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_app_db.path()).unwrap();

        let source_root = tempfile::tempdir().unwrap();
        let source_db = source_root.path().join("opencode.db");
        let writer = create_opencode_sqlite_db(&source_db);

        // Keep the writer connection open so SQLite changes stay in -wal and the
        // main DB file mtime does not advance between incremental runs.
        insert_opencode_session(&writer, "session-wal-1", 1_700_000_000_000);

        let missing_storage_root = source_root.path().join("missing-storage");
        let first = indexer
            .index_opencode_sessions_incremental(&missing_storage_root, &[source_db.clone()])
            .unwrap();
        assert_eq!(first.indexed, 1);

        let db_mtime_before = std::fs::metadata(&source_db).unwrap().modified().unwrap();

        insert_opencode_session(&writer, "session-wal-2", 1_700_000_100_000);

        let db_mtime_after = std::fs::metadata(&source_db).unwrap().modified().unwrap();
        assert_eq!(
            db_mtime_after, db_mtime_before,
            "DB mtime should stay unchanged while new rows are only in -wal"
        );

        let second = indexer
            .index_opencode_sessions_incremental(&missing_storage_root, &[source_db.clone()])
            .unwrap();

        assert!(
            second.indexed > 0,
            "Second incremental run should re-parse SQLite when WAL changes"
        );

        let count: i64 = indexer
            .db
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE tool = 'opencode'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn is_prunable_claude_sidechain_file_detects_agent_prefix() {
        let sessions_dir = PathBuf::from("/home/user/.claude/sessions");
        let path = PathBuf::from("/home/user/.claude/sessions/agent-abc123.jsonl");
        assert!(SessionIndexer::is_prunable_claude_sidechain_file(
            &path,
            &sessions_dir
        ));
    }

    #[test]
    fn is_prunable_claude_sidechain_file_allows_nested_subagent_transcripts() {
        let sessions_dir = PathBuf::from("/home/user/.claude/sessions");
        let path = PathBuf::from(
            "/home/user/.claude/sessions/65ce34ec-2589-4f2a-aad3-f536cf8b2906/subagents/agent-a41c0fb07beb52ed6.jsonl",
        );
        assert!(!SessionIndexer::is_prunable_claude_sidechain_file(
            &path,
            &sessions_dir
        ));
    }

    #[test]
    fn is_prunable_claude_sidechain_file_allows_nested_subagent_transcripts_in_project_tree() {
        let sessions_dir = PathBuf::from("/home/user/.claude/projects");
        let path = PathBuf::from(
            "/home/user/.claude/projects/-home-user-repo/65ce34ec-2589-4f2a-aad3-f536cf8b2906/subagents/agent-a41c0fb07beb52ed6.jsonl",
        );
        assert!(!SessionIndexer::is_prunable_claude_sidechain_file(
            &path,
            &sessions_dir
        ));
    }

    #[test]
    fn is_prunable_claude_sidechain_file_detects_legacy_subagents_root_file() {
        let sessions_dir = PathBuf::from("/home/user/.claude/sessions");
        let path = PathBuf::from("/home/user/.claude/sessions/subagents/agent-abc123.jsonl");
        assert!(SessionIndexer::is_prunable_claude_sidechain_file(
            &path,
            &sessions_dir
        ));
    }

    #[test]
    fn is_prunable_claude_sidechain_file_detects_subagents_root_file_without_agent_prefix() {
        let sessions_dir = PathBuf::from("/home/user/.claude/sessions");
        let path = PathBuf::from("/home/user/.claude/sessions/subagents/some-session.jsonl");
        assert!(SessionIndexer::is_prunable_claude_sidechain_file(
            &path,
            &sessions_dir
        ));
    }

    #[test]
    fn is_prunable_claude_sidechain_file_allows_regular_sessions() {
        let sessions_dir = PathBuf::from("/home/user/.claude/sessions");
        let path = PathBuf::from("/home/user/.claude/sessions/abc123.jsonl");
        assert!(!SessionIndexer::is_prunable_claude_sidechain_file(
            &path,
            &sessions_dir
        ));
    }

    #[test]
    fn is_prunable_claude_sidechain_file_allows_agent_in_middle_of_name() {
        // "agent-" prefix is required, not just containing "agent"
        let sessions_dir = PathBuf::from("/home/user/.claude/sessions");
        let path = PathBuf::from("/home/user/.claude/sessions/my-agent-session.jsonl");
        assert!(!SessionIndexer::is_prunable_claude_sidechain_file(
            &path,
            &sessions_dir
        ));
    }

    #[test]
    fn is_prunable_claude_sidechain_file_allows_subagents_in_project_name() {
        // "subagents" in an encoded project path should not trigger filtering
        let sessions_dir = PathBuf::from("/home/user/.claude/projects");
        let path = PathBuf::from("/home/user/.claude/projects/-home-user-subagents/session.jsonl");
        assert!(!SessionIndexer::is_prunable_claude_sidechain_file(
            &path,
            &sessions_dir
        ));
    }

    #[test]
    fn opencode_indexing_indexes_all_sessions_including_subagents() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let storage_root = PathBuf::from("tests/fixtures/opencode_storage");

        let count = indexer.index_opencode_sessions(&storage_root, &[]).unwrap();
        // session-002 has parentID but no messages → NoUserMessages → pruned
        // session-tools-child has parentID and messages → indexed as is_subagent=1
        assert_eq!(count, 4);

        let all_sessions: Vec<(String, String, i64)> = indexer
            .db
            .prepare("SELECT id, tool, is_subagent FROM sessions ORDER BY id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(all_sessions.len(), 4);
        assert!(all_sessions.iter().all(|(_, tool, _)| tool == "opencode"));

        let ids: Vec<&str> = all_sessions.iter().map(|(id, _, _)| id.as_str()).collect();
        assert!(ids.contains(&"session-001"));
        assert!(ids.contains(&"session-003"));
        assert!(ids.contains(&"session-tools-001"));
        assert!(ids.contains(&"session-tools-child"));

        // Verify subagent flag
        let child = all_sessions
            .iter()
            .find(|(id, _, _)| id == "session-tools-child")
            .unwrap();
        assert_eq!(
            child.2, 1,
            "session-tools-child should be marked as subagent"
        );

        let normal = all_sessions
            .iter()
            .find(|(id, _, _)| id == "session-001")
            .unwrap();
        assert_eq!(normal.2, 0, "session-001 should not be marked as subagent");

        let msg_count: i64 = indexer
            .db
            .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
            .unwrap();
        assert!(msg_count > 0, "Should have messages for indexed sessions");
    }

    #[test]
    fn opencode_indexing_returns_zero_for_missing_storage_root() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let nonexistent_root = PathBuf::from("tests/fixtures/nonexistent_opencode_storage");

        let count = indexer
            .index_opencode_sessions(&nonexistent_root, &[])
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn opencode_dual_read_indexes_sqlite_and_json() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let storage_root = PathBuf::from("tests/fixtures/opencode_storage");
        let db_path = storage_root.join("opencode.db");

        let count = indexer
            .index_opencode_sessions(&storage_root, &[db_path.clone()])
            .unwrap();

        assert_eq!(count, 6);
    }

    #[test]
    fn index_opencode_sessions_reads_all_discovered_sqlite_dbs() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();

        let source_root = tempfile::tempdir().unwrap();
        let storage_root = source_root.path().join("opencode_storage");
        std::fs::create_dir_all(&storage_root).unwrap();
        let default_db = source_root.path().join("opencode.db");
        let dev_db = source_root.path().join("opencode-dev.db");

        let default_conn = create_opencode_sqlite_db(&default_db);
        insert_opencode_session(&default_conn, "session-default", 1_700_001_000_000);
        let dev_conn = create_opencode_sqlite_db(&dev_db);
        insert_opencode_session(&dev_conn, "session-dev", 1_700_002_000_000);

        let count = indexer
            .index_opencode_sessions(&storage_root, &[default_db.clone(), dev_db.clone()])
            .unwrap();

        assert_eq!(count, 2);

        let indexed_ids: Vec<String> = indexer
            .db
            .prepare("SELECT id FROM sessions WHERE tool = 'opencode' ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(indexed_ids, vec!["session-default", "session-dev"]);
    }

    #[test]
    fn opencode_dual_read_prefers_sqlite_over_json() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let storage_root = PathBuf::from("tests/fixtures/opencode_storage");
        let db_path = storage_root.join("opencode.db");

        indexer
            .index_opencode_sessions(&storage_root, &[db_path.clone()])
            .unwrap();

        let session: (String, Option<String>) = indexer
            .db
            .query_row(
                "SELECT id, first_prompt FROM sessions WHERE id = 'session-001'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        assert_eq!(session.0, "session-001");
        assert_eq!(
            session.1.as_deref(),
            Some("Updated title from SQLite"),
            "SQLite version should win for duplicate session-001"
        );
    }

    #[test]
    fn opencode_dual_read_discovers_sqlite_only_session() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let storage_root = PathBuf::from("tests/fixtures/opencode_storage");
        let db_path = storage_root.join("opencode.db");

        indexer
            .index_opencode_sessions(&storage_root, &[db_path.clone()])
            .unwrap();

        let exists: bool = indexer
            .db
            .query_row(
                "SELECT COUNT(*) > 0 FROM sessions WHERE id = 'session-sqlite-only'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert!(exists, "SQLite-only session should be indexed");
    }

    #[test]
    fn opencode_sqlite_only_when_no_storage_root() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let nonexistent_root = PathBuf::from("tests/fixtures/nonexistent_opencode_storage");
        let db_path = PathBuf::from("tests/fixtures/opencode_storage/opencode.db");

        let count = indexer
            .index_opencode_sessions(&nonexistent_root, &[db_path.clone()])
            .unwrap();

        assert_eq!(
            count, 3,
            "Should index SQLite sessions even when storage_root is missing"
        );

        let exists: bool = indexer
            .db
            .query_row(
                "SELECT COUNT(*) > 0 FROM sessions WHERE id = 'session-sqlite-only'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(exists, "SQLite-only session should be indexed");
    }

    #[test]
    fn opencode_prune_skipped_when_enumeration_fails() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let storage_root = PathBuf::from("tests/fixtures/opencode_storage");

        // First: index JSON sessions to populate the app DB
        indexer.index_opencode_sessions(&storage_root, &[]).unwrap();
        let initial_count: i64 = indexer
            .db
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE tool = 'opencode'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(initial_count > 0, "Should have sessions after first index");

        // Second: reindex with a bad DB path and missing storage root.
        // Both backends fail to enumerate, so pruning must not run.
        let bad_root = PathBuf::from("tests/fixtures/nonexistent_opencode_storage");
        let bad_db = PathBuf::from("tests/fixtures/nonexistent.db");
        indexer
            .index_opencode_sessions(&bad_root, &[bad_db.clone()])
            .unwrap();

        let after_count: i64 = indexer
            .db
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE tool = 'opencode'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            after_count, initial_count,
            "Existing sessions must survive when enumeration fails"
        );
    }

    #[test]
    fn opencode_json_only_fallback_when_no_db() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let storage_root = PathBuf::from("tests/fixtures/opencode_storage");

        let count = indexer.index_opencode_sessions(&storage_root, &[]).unwrap();

        assert_eq!(
            count, 4,
            "JSON-only should index the same 4 sessions as before"
        );
    }

    #[test]
    fn codex_indexing_indexes_sessions() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let sessions_dir = PathBuf::from("tests/fixtures/codex_sessions");

        let count = indexer.index_codex_sessions(&sessions_dir).unwrap();
        assert_eq!(count, 3);

        let sessions: Vec<(String, String)> = indexer
            .db
            .prepare("SELECT id, tool FROM sessions ORDER BY id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(sessions.len(), 3);
        assert!(sessions.iter().all(|(_, tool)| tool == "codex"));
    }

    #[test]
    fn codex_indexing_returns_zero_for_missing_sessions_dir() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let nonexistent_dir = PathBuf::from("tests/fixtures/nonexistent_codex_sessions");

        let count = indexer.index_codex_sessions(&nonexistent_dir).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn mistral_vibe_indexing_indexes_sessions() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let sessions_dir = PathBuf::from("tests/fixtures/vibe_sessions");

        let count = indexer.index_vibe_sessions(&sessions_dir).unwrap();
        assert_eq!(count, 2);

        let sessions: Vec<(String, String)> = indexer
            .db
            .prepare("SELECT id, tool FROM sessions ORDER BY id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(sessions.len(), 2);
        assert!(sessions.iter().all(|(_, tool)| tool == "mistral_vibe"));
    }

    #[test]
    fn mistral_vibe_indexing_returns_zero_for_missing_sessions_dir() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let nonexistent_dir = PathBuf::from("tests/fixtures/nonexistent_vibe_sessions");

        let count = indexer.index_vibe_sessions(&nonexistent_dir).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn claude_indexing_persists_token_usage() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let sessions_dir = PathBuf::from("tests/fixtures/claude_sessions");
        indexer.index_claude_sessions(&sessions_dir).unwrap();

        let (input, output): (Option<i64>, Option<i64>) = indexer
            .db
            .query_row(
                "SELECT input_tokens, output_tokens FROM sessions WHERE id = 'abc123'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(input.is_some(), "input_tokens should be populated");
        assert!(output.is_some(), "output_tokens should be populated");
    }

    #[test]
    fn session_load_roundtrip_includes_token_usage() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let sessions_dir = PathBuf::from("tests/fixtures/claude_sessions");
        indexer.index_claude_sessions(&sessions_dir).unwrap();

        let session = crate::database::load_session(temp_db.path(), "abc123")
            .unwrap()
            .expect("session should exist");
        assert!(session.token_usage.is_some(), "should have token_usage");
        let usage = session.token_usage.unwrap();
        assert!(usage.input_tokens > 0);
        assert!(usage.output_tokens > 0);
    }

    #[test]
    fn clear_all_sessions_removes_sessions_and_messages() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();

        // Seed with real fixture data
        let sessions_dir = PathBuf::from("tests/fixtures/claude_sessions");
        let count = indexer.index_claude_sessions(&sessions_dir).unwrap();
        assert!(count > 0, "Should have indexed at least one session");

        let msg_count: i64 = indexer
            .db
            .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
            .unwrap();
        assert!(msg_count > 0, "Should have messages before clear");

        // Clear everything
        indexer.clear_all_sessions().unwrap();

        let session_count: i64 = indexer
            .db
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(session_count, 0, "Sessions should be empty after clear");

        let msg_count: i64 = indexer
            .db
            .query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
            .unwrap();
        assert_eq!(msg_count, 0, "Messages should be empty after clear");
    }

    #[test]
    fn indexing_diagnostics_full_reindex_returns_per_source_results() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let sources = SessionSources::resolve(Some(std::path::Path::new("tests/fixtures")));

        let result = indexer.index_all_full_reindex(&sources).unwrap();

        assert_eq!(result.per_source.len(), 4);
        assert!(result.totals.indexed > 0);
        assert!(
            result
                .per_source
                .iter()
                .any(|r| r.assistant == AiAssistant::ClaudeCode)
        );
    }

    #[test]
    fn indexing_diagnostics_malformed_source_records_errors() {
        let temp = tempfile::tempdir().unwrap();
        let claude_root = temp.path().join("claude_sessions").join("project-a");
        std::fs::create_dir_all(&claude_root).unwrap();
        std::fs::write(claude_root.join("bad.jsonl"), b"not-json\n").unwrap();

        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let sources = SessionSources::resolve(Some(temp.path()));

        let result = indexer.index_all_incremental(&sources).unwrap();
        let claude = result
            .per_source
            .iter()
            .find(|r| r.assistant == AiAssistant::ClaudeCode)
            .unwrap();

        assert_eq!(claude.errors, 1);
        assert_eq!(
            claude.status,
            crate::models::indexing_diagnostics::SourceStatus::Failed
        );
    }

    #[test]
    fn indexing_diagnostics_empty_claude_session_does_not_record_error() {
        let temp = tempfile::tempdir().unwrap();
        let claude_root = temp.path().join("claude_sessions").join("project-a");
        std::fs::create_dir_all(&claude_root).unwrap();
        std::fs::write(claude_root.join("empty.jsonl"), b"").unwrap();

        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let sources = SessionSources::resolve(Some(temp.path()));

        let result = indexer.index_all_incremental(&sources).unwrap();
        let claude = result
            .per_source
            .iter()
            .find(|r| r.assistant == AiAssistant::ClaudeCode)
            .unwrap();

        assert_eq!(claude.indexed, 0);
        assert_eq!(claude.errors, 0);
        assert_eq!(result.errors_detail.len(), 0);
        assert_eq!(
            claude.status,
            crate::models::indexing_diagnostics::SourceStatus::Empty
        );

        let session_count: i64 = indexer
            .db
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(session_count, 0);
    }

    #[test]
    fn indexing_diagnostics_error_details_are_capped_at_fifty() {
        let temp = tempfile::tempdir().unwrap();
        let claude_root = temp.path().join("claude_sessions").join("project-a");
        std::fs::create_dir_all(&claude_root).unwrap();

        for i in 0..60 {
            std::fs::write(claude_root.join(format!("bad-{i}.jsonl")), b"not-json\n").unwrap();
        }

        let temp_db = tempfile::NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let sources = SessionSources::resolve(Some(temp.path()));

        let result = indexer.index_all_incremental(&sources).unwrap();

        assert_eq!(result.errors_detail.len(), 50);
        assert!(
            result
                .errors_detail
                .iter()
                .all(|error| error.assistant == AiAssistant::ClaudeCode)
        );
    }

    #[test]
    fn indexing_diagnostics_collects_source_level_opencode_errors() {
        use crate::models::indexing_diagnostics::SourceStatus;

        let temp = tempfile::tempdir().unwrap();
        let storage_root = temp.path().join("missing-storage");
        let sqlite_path = temp.path().join("opencode.db");
        std::fs::write(&sqlite_path, b"not-a-real-db").unwrap();

        let sources = SessionSources {
            opencode_storage_root: storage_root.clone(),
            opencode_db_paths: vec![sqlite_path.clone()],
            ..SessionSources::resolve(Some(temp.path()))
        };

        let temp_db = tempfile::NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let result = indexer.index_all_incremental(&sources).unwrap();

        let opencode = result
            .per_source
            .iter()
            .find(|source| source.assistant == AiAssistant::OpenCode)
            .unwrap();

        assert!(opencode.errors > 0);
        assert!(
            opencode.status == SourceStatus::Degraded || opencode.status == SourceStatus::Failed
        );

        assert!(result.errors_detail.iter().any(|error| {
            error.assistant == AiAssistant::OpenCode
                && error
                    .location
                    .as_deref()
                    .is_some_and(|path| path.contains("opencode.db"))
        }));
    }

    #[test]
    fn indexing_diagnostics_source_status_derives_from_source_availability() {
        use crate::models::indexing_diagnostics::SourceStatus;

        // derive_source_status(source_available, indexed, skipped, errors)
        assert_eq!(derive_source_status(false, 0, 0, 0), SourceStatus::NotFound);
        assert_eq!(derive_source_status(true, 0, 0, 0), SourceStatus::Empty);
        assert_eq!(derive_source_status(true, 5, 0, 0), SourceStatus::Indexed);
        assert_eq!(derive_source_status(true, 5, 0, 2), SourceStatus::Degraded);
        assert_eq!(derive_source_status(true, 0, 0, 3), SourceStatus::Failed);
        // Skipped-only run (incremental, nothing new) should report Indexed
        assert_eq!(derive_source_status(true, 0, 10, 0), SourceStatus::Indexed);
        // Skipped with some errors should report Degraded
        assert_eq!(derive_source_status(true, 0, 10, 2), SourceStatus::Degraded);
    }

    #[test]
    fn indexing_diagnostics_opencode_source_available_with_sqlite_only() {
        let temp = tempfile::tempdir().unwrap();
        let storage_root = temp.path().join("missing-storage");
        let sqlite_path = temp.path().join("opencode.db");
        std::fs::write(&sqlite_path, b"not-a-real-db").unwrap();

        assert!(opencode_source_available(
            &storage_root,
            &[sqlite_path.clone()]
        ));
        assert!(!opencode_source_available(&storage_root, &[]));
    }

    #[test]
    fn insert_parsed_session_computes_activity_counts_and_ending_status() {
        use crate::models::{ToolCall, ToolCallStatus};

        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let fingerprint = NamedTempFile::new().unwrap();

        let mut parsed = parsed_session("activity-test", Some("/tmp/project"));

        // 3 edits, 2 commands, 1 read — last call is Completed
        let make_tc = |id: &str, tool_name: &str, status: ToolCallStatus| ToolCall {
            id: id.to_string(),
            session_id: "activity-test".to_string(),
            subagent_id: None,
            tool_name: tool_name.to_string(),
            status,
            title: None,
            summary: None,
            input_json: None,
            output_text: None,
            error_text: None,
            started_at: None,
            ended_at: None,
            duration_ms: None,
            parser_call_id: None,
        };

        parsed.tool_calls = vec![
            make_tc("tc1", "Edit", ToolCallStatus::Completed),
            make_tc("tc2", "Write", ToolCallStatus::Completed),
            make_tc("tc3", "Edit", ToolCallStatus::Completed),
            make_tc("tc4", "Bash", ToolCallStatus::Completed),
            make_tc("tc5", "Bash", ToolCallStatus::Completed),
            make_tc("tc6", "Read", ToolCallStatus::Completed),
        ];

        indexer
            .insert_parsed_session_with_fingerprint(&parsed, fingerprint.path(), fingerprint.path())
            .unwrap();

        let (edit, read, cmd, ending): (i64, i64, i64, String) = indexer
            .db
            .query_row(
                "SELECT edit_count, read_count, command_count, ending_status FROM sessions WHERE id = 'activity-test'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();

        assert_eq!(edit, 3);
        assert_eq!(read, 1);
        assert_eq!(cmd, 2);
        assert_eq!(ending, "clean");
    }

    #[test]
    fn insert_parsed_session_sets_abrupt_when_last_tool_call_is_pending() {
        use crate::models::{ToolCall, ToolCallStatus};

        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let fingerprint = NamedTempFile::new().unwrap();

        let mut parsed = parsed_session("abrupt-test", Some("/tmp/project"));
        parsed.tool_calls = vec![ToolCall {
            id: "tc1".to_string(),
            session_id: "abrupt-test".to_string(),
            subagent_id: None,
            tool_name: "Bash".to_string(),
            status: ToolCallStatus::Pending,
            title: None,
            summary: None,
            input_json: None,
            output_text: None,
            error_text: None,
            started_at: None,
            ended_at: None,
            duration_ms: None,
            parser_call_id: None,
        }];

        indexer
            .insert_parsed_session_with_fingerprint(&parsed, fingerprint.path(), fingerprint.path())
            .unwrap();

        let ending: String = indexer
            .db
            .query_row(
                "SELECT ending_status FROM sessions WHERE id = 'abrupt-test'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(ending, "abrupt");
    }

    #[test]
    fn insert_parsed_session_sets_unknown_when_no_tool_calls() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let fingerprint = NamedTempFile::new().unwrap();

        let parsed = parsed_session("no-tools-test", Some("/tmp/project"));
        // tool_calls is empty (from parsed_session helper)

        indexer
            .insert_parsed_session_with_fingerprint(&parsed, fingerprint.path(), fingerprint.path())
            .unwrap();

        let (edit, read, cmd, ending): (i64, i64, i64, String) = indexer
            .db
            .query_row(
                "SELECT edit_count, read_count, command_count, ending_status FROM sessions WHERE id = 'no-tools-test'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();

        assert_eq!(edit, 0);
        assert_eq!(read, 0);
        assert_eq!(cmd, 0);
        assert_eq!(ending, "unknown");
    }

    #[test]
    fn reindex_upsert_preserves_existing_pinned_at() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let fingerprint = NamedTempFile::new().unwrap();

        let parsed = parsed_session("pin-preserve", Some("/tmp/project"));
        indexer
            .insert_parsed_session_with_fingerprint(&parsed, fingerprint.path(), fingerprint.path())
            .unwrap();

        indexer
            .db
            .execute(
                "UPDATE sessions SET pinned_at = 1234 WHERE id = 'pin-preserve'",
                [],
            )
            .unwrap();

        let mut reparsed = parsed_session("pin-preserve", Some("/tmp/project"));
        reparsed.session.first_prompt = Some("updated prompt".to_string());

        indexer
            .insert_parsed_session_with_fingerprint(
                &reparsed,
                fingerprint.path(),
                fingerprint.path(),
            )
            .unwrap();

        let (pinned_at, first_prompt): (Option<i64>, Option<String>) = indexer
            .db
            .query_row(
                "SELECT pinned_at, first_prompt FROM sessions WHERE id = 'pin-preserve'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        assert_eq!(pinned_at, Some(1234));
        assert_eq!(first_prompt.as_deref(), Some("updated prompt"));
    }

    #[test]
    fn v10_migration_forces_incremental_reindex_of_stale_fixture_override_db() {
        let temp_db = NamedTempFile::new().unwrap();
        let sources = SessionSources::resolve(Some(std::path::Path::new("tests/fixtures")));

        {
            let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
            let result = indexer.index_all_incremental(&sources).unwrap();
            assert!(
                result.totals.indexed > 0,
                "fixture index should populate the DB"
            );
        }

        let conn = crate::database::open_connection(temp_db.path()).unwrap();
        conn.execute_batch("PRAGMA user_version = 9").unwrap();
        conn.execute(
            "UPDATE sessions SET message_count = 0 WHERE id = 'claude-tools-session'",
            [],
        )
        .unwrap();
        conn.execute(
            "DELETE FROM transcript_items WHERE session_id = 'claude-tools-session'",
            [],
        )
        .unwrap();
        drop(conn);

        let stale_count: i64 = crate::database::open_connection(temp_db.path())
            .unwrap()
            .query_row(
                "SELECT message_count FROM sessions WHERE id = 'claude-tools-session'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            stale_count, 0,
            "fixture session should be stale before reopening"
        );

        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let result = indexer.index_all_incremental(&sources).unwrap();
        assert!(
            result.totals.indexed > 0,
            "migration should clear fingerprints so incremental indexing repairs stale rows"
        );

        let (message_count, transcript_count): (i64, i64) = indexer
            .db
            .query_row(
                "SELECT s.message_count,
                        (SELECT COUNT(*) FROM transcript_items ti WHERE ti.session_id = s.id)
                 FROM sessions s
                 WHERE s.id = 'claude-tools-session'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(message_count, 4);
        assert!(transcript_count > 0);
    }
}
