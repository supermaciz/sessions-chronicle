use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

use super::{IndexingStats, SessionIndexer, push_indexing_error};
use crate::models::{AiAssistant, IndexingError};
use crate::parsers::mistral_vibe_unified::{
    MistralVibeUnifiedParser, UnifiedError, UnifiedParse, UnifiedSignature, child_session_id,
    is_unified_session_dir,
};
use anyhow::Result;
use rusqlite::OptionalExtension;

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
        self.index_unified_candidates(
            &root,
            entries.filter_map(|entry| entry.ok().map(|entry| entry.path())),
            incremental,
            stats,
            errors_detail,
        )
    }

    fn index_unified_candidates(
        &mut self,
        root: &Path,
        entries: impl Iterator<Item = PathBuf>,
        incremental: bool,
        stats: &mut IndexingStats,
        errors_detail: &mut VecDeque<IndexingError>,
    ) -> Result<()> {
        let mut present = HashSet::new();
        let mut children = Vec::new();
        let mut deleting = HashSet::new();
        for path in entries {
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if name.starts_with('.') || name == "scratchpad" || !valid_id(name) || !path.is_dir() {
                continue;
            }
            present.insert(path.clone());
            if !path.join("CURRENT").exists() {
                if self.unified_row_exists(&path)? {
                    // The observed incomplete-store diagnostic breaks any
                    // pending unstable sequence for this path.
                    self.runtime_state.break_unified_sequence(&path);
                    self.unified_failure(
                        &path,
                        &UnifiedError::Incomplete("CURRENT missing".into()).into(),
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
                // A successful current-signature skip observes a healthy
                // session and breaks any pending unstable sequence for it.
                self.runtime_state.break_unified_sequence(&path);
                stats.skipped += 1;
                continue;
            }
            let outcome = MistralVibeUnifiedParser.parse(&path);
            // Publish children only after all lifecycle outcomes are known. A root
            // enumerated before OR after its children must prevent reinsertion.
            match outcome {
                Ok(parsed) if parsed.parsed.session.is_subagent => {
                    children.push((path, parsed));
                }
                outcome => {
                    if let Err(UnifiedError::DeletingTree { child_session_ids }) = &outcome {
                        deleting.insert(name.to_string());
                        deleting.extend(child_session_ids.iter().cloned());
                    }
                    self.handle_unified_outcome(&path, outcome, stats, errors_detail)?;
                }
            }
        }
        // An orphan may be a legitimate first-level child: absence of its
        // parent is not evidence of nesting. Use only verified candidates and
        // persisted Unified rows owned by this source to establish ancestry.
        let persisted = self.owned_unified_subagents(root)?;
        let mut subagent_ids: HashSet<String> =
            persisted.iter().map(|(id, _, _)| id.clone()).collect();
        for (path, parsed) in &children {
            let existing: Option<String> = self
                .db
                .query_row(
                    "SELECT file_path FROM sessions WHERE id=?1",
                    [&parsed.parsed.session.id],
                    |r| r.get(0),
                )
                .optional()?;
            if existing.as_ref().is_none_or(|owner| {
                owner == path.to_string_lossy().as_ref() || !Path::new(owner).exists()
            }) {
                subagent_ids.insert(parsed.parsed.session.id.clone());
            }
        }
        let mut rejected = HashSet::new();
        for (path, parsed) in children {
            if deleting.contains(&parsed.parsed.session.id)
                || parsed
                    .parsed
                    .session
                    .parent_session_id
                    .as_ref()
                    .is_some_and(|parent| deleting.contains(parent))
            {
                continue;
            }
            if parsed
                .parsed
                .session
                .parent_session_id
                .as_ref()
                .is_some_and(|parent| subagent_ids.contains(parent))
            {
                self.remove_unified_paths(std::slice::from_ref(&path), stats)?;
                rejected.insert(path.clone());
                self.unified_failure(
                    &path,
                    &UnifiedError::Corrupt("deeper subagent nesting is not supported".into())
                        .into(),
                    stats,
                    errors_detail,
                );
                continue;
            }
            self.handle_unified_outcome(&path, Ok(parsed), stats, errors_detail)?;
        }
        // A previously accepted orphan can become provably invalid when its
        // parent is first revealed as a subagent (even if the orphan was skipped
        // incrementally). Cleanup is path-owned, never ID-only or legacy-wide.
        for (_, parent, path) in persisted {
            if parent
                .as_ref()
                .is_some_and(|parent| subagent_ids.contains(parent))
                && !rejected.contains(&path)
            {
                self.remove_unified_paths(std::slice::from_ref(&path), stats)?;
                self.unified_failure(
                    &path,
                    &UnifiedError::Corrupt("deeper subagent nesting is not supported".into())
                        .into(),
                    stats,
                    errors_detail,
                );
            }
        }
        // Recheck accessibility: a mount/source may disappear after read_dir
        // succeeded. Its absence is never evidence that its sessions were deleted.
        if std::fs::read_dir(root).is_err() {
            return Ok(());
        }
        // Only a missing direct directory is physical deletion evidence, not lineage.
        for path in self.unified_indexed_paths(root)? {
            let child = PathBuf::from(&path);
            if child.parent() == Some(root)
                && !present.contains(&child)
                && !child.try_exists().unwrap_or(true)
            {
                self.remove_unified_paths(&[child], stats)?;
            }
        }
        Ok(())
    }

    fn handle_unified_outcome(
        &mut self,
        path: &Path,
        outcome: std::result::Result<UnifiedParse, UnifiedError>,
        stats: &mut IndexingStats,
        errors_detail: &mut VecDeque<IndexingError>,
    ) -> Result<()> {
        match outcome {
            Ok(parsed) => {
                let tx = self.db.transaction()?;
                if Self::unified_id_collision(
                    &tx,
                    &parsed.parsed.session.id,
                    path,
                    errors_detail,
                    stats,
                )? {
                    // The collision diagnostic is an observed non-unstable
                    // outcome for this path; it breaks any pending sequence.
                    self.runtime_state.break_unified_sequence(path);
                    return Ok(());
                }
                let project =
                    Self::upsert_project_tx(&tx, parsed.parsed.session.project_path.as_deref())?;
                Self::upsert_session_row_tx(&tx, &parsed.parsed, path, project)?;
                Self::replace_session_contents_tx(&tx, &parsed.parsed)?;
                Self::link_vibe_unified_subagents_tx(&tx, &parsed.parsed)?;
                tx.execute("INSERT INTO vibe_unified_signatures(session_dir, signature) VALUES (?1, ?2) ON CONFLICT(session_dir) DO UPDATE SET signature=excluded.signature", rusqlite::params![path.to_string_lossy(), parsed.signature.to_storage()])?;
                tx.commit()?;
                self.runtime_state.break_unified_sequence(path);
                stats.indexed += 1;
            }
            Err(UnifiedError::NoUserMessages) => {
                self.remove_unified_paths(&[path.to_path_buf()], stats)?;
            }
            Err(UnifiedError::Ephemeral) => {
                self.remove_unified_paths(&[path.to_path_buf()], stats)?
            }
            Err(UnifiedError::DeletingTree { child_session_ids }) => {
                self.remove_unified_tree(path, &child_session_ids, stats)?
            }
            Err(err @ UnifiedError::Unstable(_)) => {
                let count = self.runtime_state.note_unified_deferral(path);
                if count == 1 {
                    tracing::debug!(
                        "Deferring unstable Mistral Vibe Unified session {}: {err}",
                        path.display()
                    );
                    stats.skipped += 1;
                } else {
                    // Still the same consecutive sequence: keep reporting
                    // while it lasts, and keep the counter so a later
                    // unstable pass is not silently deferred again.
                    self.unified_failure(path, &err.into(), stats, errors_detail);
                }
            }
            Err(err) => {
                // Any other parser outcome is an observed non-unstable
                // result: it breaks the consecutive sequence, so the next
                // unstable read starts a fresh first pass.
                self.runtime_state.break_unified_sequence(path);
                self.unified_failure(path, &err.into(), stats, errors_detail);
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

    fn owned_unified_subagents(
        &self,
        root: &Path,
    ) -> Result<Vec<(String, Option<String>, PathBuf)>> {
        let mut stmt = self.db.prepare(
            "SELECT s.id,s.parent_session_id,s.file_path FROM sessions s
             JOIN vibe_unified_signatures sig ON sig.session_dir=s.file_path
             WHERE s.tool='mistral_vibe' AND s.is_subagent=1",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                PathBuf::from(r.get::<_, String>(2)?),
            ))
        })?;
        Ok(rows
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .filter(|(_, _, path)| path.parent() == Some(root))
            .collect())
    }
    fn unified_signature(&self, path: &Path) -> Result<Option<UnifiedSignature>> {
        let raw: Option<String> = self
            .db
            .query_row(
                "SELECT signature FROM vibe_unified_signatures WHERE session_dir=?1",
                [path.to_string_lossy().as_ref()],
                |r| r.get(0),
            )
            .optional()?;
        Ok(raw.as_deref().and_then(UnifiedSignature::from_storage))
    }
    fn unified_id_collision(
        tx: &rusqlite::Transaction<'_>,
        id: &str,
        path: &Path,
        errors: &mut VecDeque<IndexingError>,
        stats: &mut IndexingStats,
    ) -> Result<bool> {
        let row: Option<String> = tx
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
        // Unified Mistral Vibe sessions only: other assistants indexed
        // under the same root stay owned by their own source.
        self.indexed_paths_under(&root.display().to_string(), Some(&AiAssistant::MistralVibe))
    }
    fn remove_unified_tree(
        &mut self,
        path: &Path,
        listed: &[String],
        stats: &mut IndexingStats,
    ) -> Result<()> {
        let root_id = path.file_name().unwrap().to_string_lossy();
        let root = path.parent().unwrap();
        let mut paths = HashSet::from([path.to_path_buf()]);
        let tx = self.db.transaction()?;
        let mut stmt = tx.prepare(
            "SELECT file_path FROM sessions
             WHERE tool='mistral_vibe' AND (parent_session_id=?1 OR id=?1)",
        )?;
        let rows = stmt.query_map([root_id.as_ref()], |r| r.get::<_, String>(0))?;
        for row in rows {
            let candidate = PathBuf::from(row?);
            if candidate.parent() == Some(root) {
                paths.insert(candidate);
            }
        }
        drop(stmt);
        for id in listed.iter().filter(|id| valid_id(id)) {
            paths.insert(root.join(id));
        }
        let paths = paths.into_iter().collect::<Vec<_>>();
        let removed = Self::remove_unified_paths_tx(&tx, &paths)?;
        tx.commit()?;
        self.record_unified_removal(&paths, removed, stats);
        Ok(())
    }

    fn remove_unified_paths(&mut self, paths: &[PathBuf], stats: &mut IndexingStats) -> Result<()> {
        let tx = self.db.transaction()?;
        let removed = Self::remove_unified_paths_tx(&tx, paths)?;
        tx.commit()?;
        self.record_unified_removal(paths, removed, stats);
        Ok(())
    }

    fn remove_unified_paths_tx(tx: &rusqlite::Transaction<'_>, paths: &[PathBuf]) -> Result<usize> {
        let mut removed = 0;
        for path in paths {
            let id: Option<String> = tx
                .query_row(
                    "SELECT id FROM sessions WHERE file_path=?1 AND tool='mistral_vibe'",
                    [path.to_string_lossy().as_ref()],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(id) = id {
                tx.execute(
                    "UPDATE subagents SET child_session_id=NULL WHERE child_session_id=?1",
                    [&id],
                )?;
                removed += Self::delete_session_by_id_tx(tx, &id)?;
            }
            tx.execute(
                "DELETE FROM vibe_unified_signatures WHERE session_dir=?1",
                [path.to_string_lossy().as_ref()],
            )?;
        }
        Ok(removed)
    }

    /// Records a committed Unified removal: the confirmed deletion breaks
    /// every removed path's pending unstable sequence.
    fn record_unified_removal(&self, paths: &[PathBuf], removed: usize, stats: &mut IndexingStats) {
        self.runtime_state.note_unified_removals(paths);
        stats.removed += removed;
    }
    fn link_vibe_unified_subagents_tx(
        tx: &rusqlite::Transaction<'_>,
        parsed: &crate::parsers::ParsedSession,
    ) -> Result<()> {
        if !is_unified_session_dir(Path::new(&parsed.session.file_path)) {
            return Ok(());
        }
        let parent = if parsed.session.is_subagent {
            let Some(parent) = parsed.session.parent_session_id.as_deref() else {
                return Ok(());
            };
            parent
        } else {
            &parsed.session.id
        };
        let mut stmt = tx.prepare("SELECT subagents.id, subagents.agent_id, subagents.parser_ref, child.file_path FROM subagents JOIN sessions parent ON parent.id=subagents.session_id JOIN sessions child ON child.id=subagents.agent_id WHERE parent.id=?1 AND parent.is_subagent=0 AND parent.tool='mistral_vibe' AND child.tool='mistral_vibe' AND child.is_subagent=1 AND child.parent_session_id=parent.id")?;
        let rows = stmt.query_map([parent], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        for row in rows {
            let (effect, child, action, file_path) = row?;
            if action.is_some_and(|action| child_session_id(parent, &action) == child)
                && is_unified_session_dir(Path::new(&file_path))
            {
                tx.execute(
                    "UPDATE subagents SET child_session_id=?1 WHERE id=?2",
                    rusqlite::params![child, effect],
                )?;
            }
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

#[cfg(test)]
mod tests;
