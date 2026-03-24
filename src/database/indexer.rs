use anyhow::{Context, Result};
use rusqlite::Connection;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::models::indexing_diagnostics::{IndexingRunResult, PerSourceResult, SourceStatus};
use crate::models::session::AiAssistant;
use crate::parsers::ParsedSession;
use crate::parsers::claude_code::ClaudeCodeParser;
use crate::parsers::codex::{CodexParser, ParseError as CodexParseError};
use crate::parsers::mistral_vibe::{MistralVibeParser, ParseError as MistralVibeParseError};
use crate::parsers::opencode::{
    OpenCodeBackend, OpenCodeParser, ParseError as OpenCodeParseError, SessionSource,
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

pub(crate) fn opencode_source_available(storage_root: &Path, db_path: Option<&Path>) -> bool {
    storage_root.exists() || db_path.is_some_and(|path| path.exists())
}

fn opencode_display_path(storage_root: &Path, db_path: Option<&Path>) -> String {
    if storage_root.exists() {
        storage_root.display().to_string()
    } else if let Some(db) = db_path {
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

fn is_opencode_error(err: &anyhow::Error) -> bool {
    err.downcast_ref::<OpenCodeParseError>().is_some()
}

fn is_codex_error(err: &anyhow::Error) -> bool {
    err.downcast_ref::<CodexParseError>().is_some()
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
        Ok(self
            .index_claude_sessions_internal(sessions_dir, false)?
            .indexed)
    }

    pub fn index_claude_sessions_incremental(
        &mut self,
        sessions_dir: &Path,
    ) -> Result<IndexingStats> {
        self.index_claude_sessions_internal(sessions_dir, true)
    }

    fn index_claude_sessions_internal(
        &mut self,
        sessions_dir: &Path,
        incremental: bool,
    ) -> Result<IndexingStats> {
        let parser = ClaudeCodeParser;
        let mut stats = IndexingStats::default();

        for entry in walkdir::WalkDir::new(sessions_dir)
            .max_depth(5)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if entry.file_type().is_file()
                && let Some(ext) = path.extension()
                && ext == "jsonl"
            {
                if Self::is_sidechain_file(path, sessions_dir) {
                    if let Err(err) = self.remove_session_for_file(path) {
                        tracing::warn!(
                            "Failed to prune sidechain session {}: {}",
                            path.display(),
                            err
                        );
                    }
                    continue;
                }

                if incremental && !self.should_reindex(path)? {
                    stats.skipped += 1;
                    continue;
                }

                if let Err(e) = self.index_session_file(path, &parser) {
                    tracing::warn!("Failed to index {}: {}", path.display(), e);
                    stats.errors += 1;
                } else {
                    stats.indexed += 1;
                }
            }
        }

        self.prune_orphan_fingerprints()?;
        Ok(stats)
    }

    #[allow(dead_code)]
    pub fn index_opencode_sessions(
        &mut self,
        storage_root: &Path,
        db_path: Option<&Path>,
    ) -> Result<usize> {
        Ok(self
            .index_opencode_sessions_internal(storage_root, db_path, false)?
            .indexed)
    }

    pub fn index_opencode_sessions_incremental(
        &mut self,
        storage_root: &Path,
        db_path: Option<&Path>,
    ) -> Result<IndexingStats> {
        self.index_opencode_sessions_internal(storage_root, db_path, true)
    }

    fn index_opencode_sessions_internal(
        &mut self,
        storage_root: &Path,
        db_path: Option<&Path>,
        incremental: bool,
    ) -> Result<IndexingStats> {
        let has_storage_root = storage_root.exists();
        let has_db = db_path.is_some_and(|p| p.exists());

        if !has_storage_root && !has_db {
            return Ok(IndexingStats::default());
        }

        let parser = OpenCodeParser::new(storage_root);
        let mut indexed_ids: HashSet<String> = HashSet::new();
        let mut stats = IndexingStats::default();
        let mut enumeration_succeeded = false;
        let mut sqlite_enumerated = false;

        if let Some(db_path) = db_path {
            if incremental && !self.should_reindex_opencode_sqlite(db_path)? {
                stats.skipped += 1;
            } else {
                match SqliteBackend::open(db_path) {
                    Ok(sqlite_backend) => match sqlite_backend.list_sessions() {
                        Ok(entries) => {
                            enumeration_succeeded = true;
                            sqlite_enumerated = true;
                            for entry in &entries {
                                match parser.parse_entry(entry, &sqlite_backend) {
                                    Ok(parsed) => {
                                        if let Err(err) = self
                                            .insert_parsed_session_with_fingerprint(
                                                &parsed,
                                                db_path,
                                                &Self::opencode_sqlite_fingerprint_target(db_path),
                                            )
                                        {
                                            tracing::warn!(
                                                "Failed to insert SQLite session {}: {}",
                                                entry.id,
                                                err
                                            );
                                            stats.errors += 1;
                                            continue;
                                        }
                                        indexed_ids.insert(entry.id.clone());
                                        stats.indexed += 1;
                                    }
                                    Err(err) => {
                                        if is_opencode_error(&err) {
                                            tracing::debug!(
                                                "Skipped SQLite session {}: {}",
                                                entry.id,
                                                err
                                            );
                                        } else {
                                            tracing::warn!(
                                                "Failed to parse SQLite session {}: {}",
                                                entry.id,
                                                err
                                            );
                                            stats.errors += 1;
                                        }
                                    }
                                }
                            }
                        }
                        Err(err) => {
                            tracing::warn!("Failed to list SQLite sessions: {}", err);
                        }
                    },
                    Err(err) => {
                        tracing::warn!(
                            "Failed to open OpenCode DB {}: {} - falling back to JSON only",
                            db_path.display(),
                            err
                        );
                    }
                }
            }
        }

        if has_storage_root {
            let json_backend = JsonBackend::new(storage_root);
            match json_backend.list_sessions() {
                Ok(entries) => {
                    enumeration_succeeded = true;
                    for entry in entries {
                        if indexed_ids.contains(&entry.id) {
                            tracing::debug!(
                                "Skipping JSON session {} (already indexed from SQLite)",
                                entry.id
                            );
                            continue;
                        }

                        let path = match &entry.source {
                            SessionSource::JsonFile(path) => path,
                            SessionSource::SqliteRow { .. } => continue,
                        };

                        if incremental && !self.should_reindex(path)? {
                            indexed_ids.insert(entry.id.clone());
                            stats.skipped += 1;
                            continue;
                        }

                        match self.index_opencode_session_file(path, &parser) {
                            Ok(()) => {
                                indexed_ids.insert(entry.id);
                                stats.indexed += 1;
                            }
                            Err(err) => {
                                if is_opencode_error(&err) {
                                    tracing::debug!(
                                        "Skipped OpenCode session {}: {}",
                                        path.display(),
                                        err
                                    );
                                    if let Err(remove_err) = self.remove_session_for_file(path) {
                                        tracing::warn!(
                                            "Failed to prune session {}: {}",
                                            path.display(),
                                            remove_err
                                        );
                                    }
                                } else {
                                    tracing::warn!("Failed to index {}: {}", path.display(), err);
                                    stats.errors += 1;
                                }
                            }
                        }
                    }
                }
                Err(err) => {
                    tracing::warn!("Failed to list JSON OpenCode sessions: {}", err);
                }
            }
        }

        if incremental {
            if sqlite_enumerated {
                self.prune_stale_opencode_sessions(&indexed_ids)?;
            }
        } else if enumeration_succeeded {
            self.prune_stale_opencode_sessions(&indexed_ids)?;
        }

        self.prune_orphan_fingerprints()?;

        Ok(stats)
    }

    #[allow(dead_code)]
    pub fn index_codex_sessions(&mut self, sessions_dir: &Path) -> Result<usize> {
        Ok(self
            .index_codex_sessions_internal(sessions_dir, false)?
            .indexed)
    }

    pub fn index_codex_sessions_incremental(
        &mut self,
        sessions_dir: &Path,
    ) -> Result<IndexingStats> {
        self.index_codex_sessions_internal(sessions_dir, true)
    }

    fn index_codex_sessions_internal(
        &mut self,
        sessions_dir: &Path,
        incremental: bool,
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
            if entry.file_type().is_file()
                && let Some(file_name) = path.file_name().and_then(|name| name.to_str())
                && file_name.starts_with("rollout-")
                && file_name.ends_with(".jsonl")
            {
                if incremental && !self.should_reindex(path)? {
                    stats.skipped += 1;
                    continue;
                }

                match self.index_codex_session_file(path, &parser) {
                    Ok(()) => {
                        stats.indexed += 1;
                    }
                    Err(err) => {
                        if is_codex_error(&err) {
                            tracing::warn!("Skipped Codex session {}: {}", path.display(), err);
                            if let Err(remove_err) = self.remove_session_for_file(path) {
                                tracing::warn!(
                                    "Failed to prune session {}: {}",
                                    path.display(),
                                    remove_err
                                );
                            }
                        } else {
                            tracing::warn!("Failed to index {}: {}", path.display(), err);
                            stats.errors += 1;
                        }
                    }
                }
            }
        }

        self.prune_orphan_fingerprints()?;
        Ok(stats)
    }

    #[allow(dead_code)]
    pub fn index_vibe_sessions(&mut self, sessions_dir: &Path) -> Result<usize> {
        Ok(self
            .index_vibe_sessions_internal(sessions_dir, false)?
            .indexed)
    }

    pub fn index_vibe_sessions_incremental(
        &mut self,
        sessions_dir: &Path,
    ) -> Result<IndexingStats> {
        self.index_vibe_sessions_internal(sessions_dir, true)
    }

    fn index_vibe_sessions_internal(
        &mut self,
        sessions_dir: &Path,
        incremental: bool,
    ) -> Result<IndexingStats> {
        if !sessions_dir.exists() {
            return Ok(IndexingStats::default());
        }

        let parser = MistralVibeParser;
        let mut stats = IndexingStats::default();

        let entries = std::fs::read_dir(sessions_dir)
            .with_context(|| format!("Failed to read {}", sessions_dir.display()))?;

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    tracing::warn!("Failed to read Mistral Vibe session entry: {}", err);
                    continue;
                }
            };

            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let fingerprint_target = path.join("messages.jsonl");
            if !path.join("meta.json").exists() || !fingerprint_target.exists() {
                continue;
            }

            if incremental && !self.should_reindex(&fingerprint_target)? {
                stats.skipped += 1;
                continue;
            }

            match parser.parse(&path) {
                Ok(parsed) => {
                    self.insert_parsed_session_with_fingerprint(
                        &parsed,
                        &path,
                        &fingerprint_target,
                    )?;
                    stats.indexed += 1;
                }
                Err(err) => {
                    if matches!(
                        err.downcast_ref::<MistralVibeParseError>(),
                        Some(MistralVibeParseError::NoUserMessages)
                    ) {
                        if let Err(remove_err) = self.remove_session_for_file(&path) {
                            tracing::warn!(
                                "Failed to prune session {}: {}",
                                path.display(),
                                remove_err
                            );
                        }
                    } else {
                        tracing::warn!("Failed to index {}: {}", path.display(), err);
                        stats.errors += 1;
                    }
                }
            }
        }

        self.prune_orphan_fingerprints()?;
        Ok(stats)
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

        tx.execute(
            "INSERT OR REPLACE INTO sessions
             (id, tool, project_path, project_id, start_time, message_count, file_path, last_updated,
              first_prompt, parent_session_id, is_subagent,
              input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
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
            ],
        )?;

        tx.execute("DELETE FROM messages WHERE session_id = ?1", [&session.id])?;
        tx.execute(
            "DELETE FROM transcript_items WHERE session_id = ?1",
            [&session.id],
        )?;
        tx.execute(
            "DELETE FROM tool_calls WHERE session_id = ?1",
            [&session.id],
        )?;
        tx.execute("DELETE FROM subagents WHERE session_id = ?1", [&session.id])?;

        for msg in &parsed.messages {
            tx.execute(
                "INSERT INTO messages (session_id, message_index, role, content, timestamp, model)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    &session.id,
                    msg.index as i64,
                    format!("{:?}", msg.role).to_lowercase(),
                    &msg.content,
                    msg.timestamp.timestamp(),
                    &msg.model,
                ],
            )?;
        }

        for tc in &parsed.tool_calls {
            crate::database::insert_tool_call(&tx, tc, &session.id)?;
        }

        for sa in &parsed.subagents {
            crate::database::insert_subagent(&tx, sa, &session.id)?;
        }

        for item in &parsed.transcript_items {
            crate::database::insert_transcript_item(&tx, item, &session.id)?;
        }

        Self::upsert_fingerprint_tx(&tx, fingerprint_path)?;

        tx.commit()?;

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

    fn is_sidechain_file(file_path: &Path, sessions_dir: &Path) -> bool {
        let is_agent_file = file_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| stem.starts_with("agent-"));

        // Check if path is under sessions_dir/subagents/
        let is_subagent = file_path
            .strip_prefix(sessions_dir)
            .ok()
            .and_then(|rel| rel.components().next())
            .is_some_and(|first| first.as_os_str() == "subagents");

        is_agent_file || is_subagent
    }

    /// Clear all indexed sessions and messages.
    ///
    /// Note: `messages` is an FTS5 virtual table. Standard `DELETE FROM` works
    /// correctly on FTS5 tables and participates in transactions normally.
    pub fn clear_all_sessions(&mut self) -> Result<()> {
        let tx = self.db.transaction()?;
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
        let claude = self.index_claude_sessions_incremental(&sources.claude_dir)?;
        let opencode = self.index_opencode_sessions_incremental(
            &sources.opencode_storage_root,
            sources.opencode_db_path.as_deref(),
        )?;
        let codex = self.index_codex_sessions_incremental(&sources.codex_dir)?;
        let vibe = self.index_vibe_sessions_incremental(&sources.vibe_dir)?;

        let per_source = vec![
            build_per_source_result(
                AiAssistant::ClaudeCode,
                sources.claude_dir.display().to_string(),
                sources.claude_dir.exists(),
                claude,
            ),
            build_per_source_result(
                AiAssistant::OpenCode,
                opencode_display_path(
                    &sources.opencode_storage_root,
                    sources.opencode_db_path.as_deref(),
                ),
                opencode_source_available(
                    &sources.opencode_storage_root,
                    sources.opencode_db_path.as_deref(),
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
            .fold(IndexingStats::default(), |mut acc, r| {
                acc.indexed += r.indexed;
                acc.skipped += r.skipped;
                acc.errors += r.errors;
                acc
            });

        Ok(IndexingRunResult { totals, per_source })
    }

    pub fn index_all_full_reindex(
        &mut self,
        sources: &SessionSources,
    ) -> Result<IndexingRunResult> {
        self.clear_all_sessions()?;

        let claude = self.index_claude_sessions_internal(&sources.claude_dir, false)?;
        let opencode = self.index_opencode_sessions_internal(
            &sources.opencode_storage_root,
            sources.opencode_db_path.as_deref(),
            false,
        )?;
        let codex = self.index_codex_sessions_internal(&sources.codex_dir, false)?;
        let vibe = self.index_vibe_sessions_internal(&sources.vibe_dir, false)?;

        let per_source = vec![
            build_per_source_result(
                AiAssistant::ClaudeCode,
                sources.claude_dir.display().to_string(),
                sources.claude_dir.exists(),
                claude,
            ),
            build_per_source_result(
                AiAssistant::OpenCode,
                opencode_display_path(
                    &sources.opencode_storage_root,
                    sources.opencode_db_path.as_deref(),
                ),
                opencode_source_available(
                    &sources.opencode_storage_root,
                    sources.opencode_db_path.as_deref(),
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
            .fold(IndexingStats::default(), |mut acc, r| {
                acc.indexed += r.indexed;
                acc.skipped += r.skipped;
                acc.errors += r.errors;
                acc
            });

        Ok(IndexingRunResult { totals, per_source })
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
        tx.execute("DELETE FROM tool_calls WHERE session_id = ?1", [session_id])?;
        tx.execute("DELETE FROM subagents WHERE session_id = ?1", [session_id])?;
        tx.execute("DELETE FROM messages WHERE session_id = ?1", [session_id])?;
        tx.execute("DELETE FROM sessions WHERE id = ?1", [session_id])?;
        tx.commit()?;
        Ok(())
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
                first_prompt: Some("hello".to_string()),
                parent_session_id: None,
                is_subagent: false,
                token_usage: None,
            },
            messages: vec![],
            tool_calls: vec![],
            subagents: vec![],
            transcript_items: vec![],
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
    fn opencode_incremental_skip_keeps_sqlite_only_sessions() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let storage_root = PathBuf::from("tests/fixtures/opencode_storage");
        let db_path = storage_root.join("opencode.db");

        let first = indexer
            .index_opencode_sessions_incremental(&storage_root, Some(&db_path))
            .unwrap();
        assert!(first.indexed > 0);

        let second = indexer
            .index_opencode_sessions_incremental(&storage_root, Some(&db_path))
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
            .index_opencode_sessions_incremental(&missing_storage_root, Some(&source_db))
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
            .index_opencode_sessions_incremental(&missing_storage_root, Some(&source_db))
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
    fn is_sidechain_file_detects_agent_prefix() {
        let sessions_dir = PathBuf::from("/home/user/.claude/sessions");
        let path = PathBuf::from("/home/user/.claude/sessions/agent-abc123.jsonl");
        assert!(SessionIndexer::is_sidechain_file(&path, &sessions_dir));
    }

    #[test]
    fn is_sidechain_file_detects_subagents_directory() {
        let sessions_dir = PathBuf::from("/home/user/.claude/sessions");
        let path = PathBuf::from("/home/user/.claude/sessions/subagents/some-session.jsonl");
        assert!(SessionIndexer::is_sidechain_file(&path, &sessions_dir));
    }

    #[test]
    fn is_sidechain_file_allows_regular_sessions() {
        let sessions_dir = PathBuf::from("/home/user/.claude/sessions");
        let path = PathBuf::from("/home/user/.claude/sessions/abc123.jsonl");
        assert!(!SessionIndexer::is_sidechain_file(&path, &sessions_dir));
    }

    #[test]
    fn is_sidechain_file_allows_agent_in_middle_of_name() {
        // "agent-" prefix is required, not just containing "agent"
        let sessions_dir = PathBuf::from("/home/user/.claude/sessions");
        let path = PathBuf::from("/home/user/.claude/sessions/my-agent-session.jsonl");
        assert!(!SessionIndexer::is_sidechain_file(&path, &sessions_dir));
    }

    #[test]
    fn is_sidechain_file_allows_subagents_in_project_name() {
        // "subagents" in an encoded project path should not trigger filtering
        let sessions_dir = PathBuf::from("/home/user/.claude/projects");
        let path = PathBuf::from("/home/user/.claude/projects/-home-user-subagents/session.jsonl");
        assert!(!SessionIndexer::is_sidechain_file(&path, &sessions_dir));
    }

    #[test]
    fn opencode_indexing_indexes_all_sessions_including_subagents() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let storage_root = PathBuf::from("tests/fixtures/opencode_storage");

        let count = indexer
            .index_opencode_sessions(&storage_root, None)
            .unwrap();
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
            .index_opencode_sessions(&nonexistent_root, None)
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
            .index_opencode_sessions(&storage_root, Some(&db_path))
            .unwrap();

        assert_eq!(count, 6);
    }

    #[test]
    fn opencode_dual_read_prefers_sqlite_over_json() {
        let temp_db = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(temp_db.path()).unwrap();
        let storage_root = PathBuf::from("tests/fixtures/opencode_storage");
        let db_path = storage_root.join("opencode.db");

        indexer
            .index_opencode_sessions(&storage_root, Some(&db_path))
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
            .index_opencode_sessions(&storage_root, Some(&db_path))
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
            .index_opencode_sessions(&nonexistent_root, Some(&db_path))
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
        indexer
            .index_opencode_sessions(&storage_root, None)
            .unwrap();
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
            .index_opencode_sessions(&bad_root, Some(&bad_db))
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

        let count = indexer
            .index_opencode_sessions(&storage_root, None)
            .unwrap();

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

        assert!(opencode_source_available(&storage_root, Some(&sqlite_path)));
        assert!(!opencode_source_available(&storage_root, None));
    }
}
