use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use super::{IndexingStats, SessionIndexer, push_indexing_error};
use crate::models::{AiAssistant, IndexingError};
use crate::parsers::mistral_vibe_unified::{
    MistralVibeUnifiedParser, UnifiedError, is_unified_session_dir,
};
use anyhow::Result;
use rusqlite::OptionalExtension;

static UNIFIED_DEFERRALS: LazyLock<Mutex<HashMap<PathBuf, u32>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

impl SessionIndexer {
    pub(super) fn index_vibe_unified_sessions(
        &mut self,
        vibe_dir: &Path,
        incremental: bool,
        stats: &mut IndexingStats,
        errors_detail: &mut VecDeque<IndexingError>,
    ) -> Result<()> {
        let root = vibe_dir.join("unified");
        let entries = match std::fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(_) => return Ok(()),
        };
        let mut present = std::collections::HashSet::new();
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if name.starts_with('.') || name == "scratchpad" || !valid_id(name) || !path.is_dir() {
                continue;
            }
            present.insert(path.clone());
            if !path.join("CURRENT").exists() {
                if self.unified_row_exists(&path)? {
                    self.unified_failure(
                        &path,
                        &anyhow::anyhow!("CURRENT missing"),
                        stats,
                        errors_detail,
                    );
                }
                continue;
            }
            if incremental
                && let Some(sig) = self.unified_signature(&path)?
                && sig.is_current(&path)
            {
                stats.skipped += 1;
                continue;
            }
            match MistralVibeUnifiedParser.parse(&path) {
                Ok(parsed) => {
                    if self.unified_id_collision(
                        &parsed.parsed.session.id,
                        &path,
                        errors_detail,
                        stats,
                    )? {
                        continue;
                    }
                    let tx = self.db.transaction()?;
                    let project = Self::upsert_project_tx(
                        &tx,
                        parsed.parsed.session.project_path.as_deref(),
                    )?;
                    Self::upsert_session_row_tx(&tx, &parsed.parsed, &path, project)?;
                    Self::replace_session_contents_tx(&tx, &parsed.parsed)?;
                    Self::link_vibe_unified_subagents_tx(&tx, &parsed.parsed)?;
                    tx.execute("INSERT INTO vibe_unified_signatures(session_dir, signature) VALUES (?1, ?2) ON CONFLICT(session_dir) DO UPDATE SET signature=excluded.signature", rusqlite::params![path.to_string_lossy(), parsed.signature.to_storage()])?;
                    tx.commit()?;
                    UNIFIED_DEFERRALS.lock().unwrap().remove(&path);
                    stats.indexed += 1;
                }
                Err(UnifiedError::NoUserMessages) => {
                    self.prune_session_after_parse_skip(
                        AiAssistant::MistralVibe,
                        &path,
                        stats,
                        errors_detail,
                    );
                    self.delete_unified_signature(&path)?;
                }
                Err(UnifiedError::Ephemeral) => self
                    .remove_session_for_file(&path)
                    .map(|n| stats.removed += n)?,
                Err(UnifiedError::DeletingTree { child_session_ids }) => {
                    self.remove_unified_tree(&path, &child_session_ids, stats)?
                }
                Err(err @ UnifiedError::Unstable(_)) => {
                    let mut deferrals = UNIFIED_DEFERRALS.lock().unwrap();
                    let count = deferrals.entry(path.clone()).or_default();
                    *count += 1;
                    if *count == 1 {
                        tracing::debug!(
                            "Deferring unstable Mistral Vibe Unified session {}: {err}",
                            path.display()
                        );
                        stats.skipped += 1;
                    } else {
                        self.unified_failure(&path, &err.into(), stats, errors_detail);
                    }
                }
                Err(err) => self.unified_failure(&path, &err.into(), stats, errors_detail),
            }
        }
        // Prune only after successful directory enumeration; independent of entry order.
        for path in self.unified_indexed_paths(&root)? {
            let child = PathBuf::from(&path);
            if child.parent() == Some(root.as_path())
                && !present.contains(&child)
                && !child.exists()
            {
                let tx = self.db.transaction()?;
                let ids: Vec<String> = {
                    let mut stmt = tx.prepare("SELECT id FROM sessions WHERE file_path = ?1 OR (parent_session_id = (SELECT id FROM sessions WHERE file_path = ?1) AND tool='mistral_vibe')")?;
                    stmt.query_map([&path], |r| r.get(0))?
                        .collect::<rusqlite::Result<_>>()?
                };
                for id in ids {
                    tx.execute(
                        "UPDATE subagents SET child_session_id=NULL WHERE child_session_id=?1",
                        [&id],
                    )?;
                    tx.execute("DELETE FROM sessions WHERE id=?1", [&id])?;
                    tx.execute("DELETE FROM messages WHERE session_id=?1", [&id])?;
                    tx.execute("DELETE FROM tool_calls WHERE session_id=?1", [&id])?;
                    tx.execute("DELETE FROM subagents WHERE session_id=?1", [&id])?;
                    tx.execute("DELETE FROM transcript_items WHERE session_id=?1", [&id])?;
                    tx.execute(
                        "DELETE FROM reasoning_attachments WHERE session_id=?1",
                        [&id],
                    )?;
                }
                tx.execute(
                    "DELETE FROM vibe_unified_signatures WHERE session_dir=?1",
                    [&path],
                )?;
                tx.commit()?;
                UNIFIED_DEFERRALS.lock().unwrap().remove(&child);
                stats.removed += 1;
            }
        }
        Ok(())
    }

    fn unified_failure(
        &self,
        path: &Path,
        err: &anyhow::Error,
        stats: &mut IndexingStats,
        errors: &mut VecDeque<IndexingError>,
    ) {
        push_indexing_error(
            errors,
            AiAssistant::MistralVibe,
            Some(path.display().to_string()),
            format!("Failed to index session: {err}"),
        );
        stats.errors += 1;
    }
    fn unified_row_exists(&self, path: &Path) -> Result<bool> {
        Ok(self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sessions WHERE file_path=?1)",
            [path.to_string_lossy().as_ref()],
            |r| r.get::<_, bool>(0),
        )?)
    }
    fn unified_signature(
        &self,
        path: &Path,
    ) -> Result<Option<crate::parsers::mistral_vibe_unified::signature::UnifiedSignature>> {
        let raw: Option<String> = self
            .db
            .query_row(
                "SELECT signature FROM vibe_unified_signatures WHERE session_dir=?1",
                [path.to_string_lossy().as_ref()],
                |r| r.get(0),
            )
            .optional()?;
        Ok(raw.as_deref().and_then(
            crate::parsers::mistral_vibe_unified::signature::UnifiedSignature::from_storage,
        ))
    }
    fn delete_unified_signature(&self, path: &Path) -> Result<()> {
        self.db.execute(
            "DELETE FROM vibe_unified_signatures WHERE session_dir=?1",
            [path.to_string_lossy().as_ref()],
        )?;
        Ok(())
    }
    fn unified_id_collision(
        &self,
        id: &str,
        path: &Path,
        errors: &mut VecDeque<IndexingError>,
        stats: &mut IndexingStats,
    ) -> Result<bool> {
        let row: Option<String> = self
            .db
            .query_row("SELECT file_path FROM sessions WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        if let Some(existing) = row
            && existing != path.to_string_lossy()
            && Path::new(&existing).exists()
        {
            push_indexing_error(
                errors,
                AiAssistant::MistralVibe,
                Some(path.display().to_string()),
                format!("Mistral Vibe session id {id} is already indexed from {existing}"),
            );
            stats.errors += 1;
            return Ok(true);
        }
        Ok(false)
    }
    fn unified_indexed_paths(&self, root: &Path) -> Result<Vec<String>> {
        let prefix = format!("{}/", root.display());
        let upper = format!("{}0", root.display());
        let mut stmt=self.db.prepare("SELECT file_path FROM sessions WHERE file_path>=?1 AND file_path<?2 AND tool='mistral_vibe'")?;
        Ok(stmt
            .query_map([prefix, upper], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }
    fn remove_unified_tree(
        &mut self,
        path: &Path,
        listed: &[String],
        stats: &mut IndexingStats,
    ) -> Result<()> {
        let tx = self.db.transaction()?;
        let root_id: Option<String> = tx
            .query_row(
                "SELECT id FROM sessions WHERE file_path=?1",
                [path.to_string_lossy().as_ref()],
                |r| r.get(0),
            )
            .optional()?;
        let mut ids = listed.to_vec();
        if let Some(root_id) = root_id {
            ids.push(root_id);
        }
        if let Some(root) = ids.last().cloned() {
            let mut stmt = tx.prepare(
                "SELECT id FROM sessions WHERE parent_session_id=?1 AND tool='mistral_vibe'",
            )?;
            ids.extend(
                stmt.query_map([root], |r| r.get(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?,
            );
        }
        for id in &ids {
            tx.execute(
                "UPDATE subagents SET child_session_id=NULL WHERE child_session_id=?1",
                [id],
            )?;
            tx.execute("DELETE FROM sessions WHERE id=?1", [id])?;
            tx.execute("DELETE FROM messages WHERE session_id=?1", [id])?;
            tx.execute("DELETE FROM tool_calls WHERE session_id=?1", [id])?;
            tx.execute("DELETE FROM subagents WHERE session_id=?1", [id])?;
            tx.execute("DELETE FROM transcript_items WHERE session_id=?1", [id])?;
            tx.execute(
                "DELETE FROM reasoning_attachments WHERE session_id=?1",
                [id],
            )?;
        }
        tx.execute(
            "DELETE FROM vibe_unified_signatures WHERE session_dir=?1",
            [path.to_string_lossy().as_ref()],
        )?;
        tx.commit()?;
        stats.removed += ids.len();
        Ok(())
    }
    fn link_vibe_unified_subagents_tx(
        tx: &rusqlite::Transaction<'_>,
        parsed: &crate::parsers::ParsedSession,
    ) -> Result<()> {
        if !is_unified_session_dir(Path::new(&parsed.session.file_path)) {
            return Ok(());
        }
        if parsed.session.is_subagent {
            if let Some(parent) = parsed.session.parent_session_id.as_deref() {
                tx.execute("UPDATE subagents SET child_session_id=?1 WHERE session_id=?2 AND agent_id=?1 AND ?2 IN (SELECT id FROM sessions WHERE is_subagent=0)",rusqlite::params![parsed.session.id,parent])?;
            }
        } else {
            tx.execute("UPDATE subagents SET child_session_id=agent_id WHERE session_id=?1 AND agent_id IN (SELECT id FROM sessions WHERE tool='mistral_vibe' AND is_subagent=1 AND parent_session_id=?1)",[&parsed.session.id])?;
        }
        Ok(())
    }
}

fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty()
        && b.len() <= 128
        && b[0].is_ascii_alphanumeric()
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-')
}
