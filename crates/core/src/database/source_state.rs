//! Source availability bookkeeping, independent of transcript parsing.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::models::{AiAssistant, SourceKind};

#[derive(Debug, Clone)]
pub(crate) enum SourceScope {
    PathRoot {
        assistant: AiAssistant,
        root: PathBuf,
    },
    Database {
        path: PathBuf,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct SourceObservation {
    pub assistant: AiAssistant,
    pub id: String,
    pub locator: PathBuf,
    pub kind: SourceKind,
    pub scope: Option<PathBuf>,
    /// Nanosecond mtime and byte size of one authoritative transcript file.
    pub fingerprint: Option<(i64, i64)>,
}

#[derive(Debug)]
pub(crate) struct ScopeScan {
    pub scope: SourceScope,
    pub complete: bool,
    pub discovered_ids: HashSet<String>,
    pub protected_locators: HashSet<PathBuf>,
    pub observations: Vec<SourceObservation>,
}

/// A binary half-open SQL range for descendants, including the separator.
/// Paths remain lexical: disappeared sources cannot reliably be canonicalized.
pub(crate) fn path_prefix_bounds(root: &Path) -> Option<(String, String)> {
    if !root.is_absolute() {
        return None;
    }
    let root = root.to_str()?.trim_end_matches('/');
    Some((format!("{root}/"), format!("{root}0")))
}

fn sql_path(path: &Path) -> Result<&str> {
    ensure!(
        path.is_absolute(),
        "Source locator must be absolute: {path:?}"
    );
    path.to_str().context("Source locator is not valid UTF-8")
}

/// Record evidence only for the row currently owned by this source.
///
/// The caller must upsert the new locator before recording a successful reindex,
/// and commit this transaction together with content and fingerprint changes.
/// The returned transition count is provisional until that commit succeeds.
pub(crate) fn record_observation_tx(
    tx: &Transaction<'_>,
    observation: &SourceObservation,
    now: i64,
    reindexed: bool,
) -> Result<usize> {
    let locator = sql_path(&observation.locator)?;
    let scope = observation.scope.as_deref().map(sql_path).transpose()?;
    ensure!(
        match observation.kind {
            SourceKind::DatabaseRecord => {
                observation.assistant == AiAssistant::OpenCode && scope.is_some()
            }
            _ => scope.is_none(),
        },
        "Source kind and storage scope do not match"
    );

    let owner: Option<(Option<String>, bool)> = tx
        .query_row(
            "SELECT source_scope, source_missing FROM sessions
             WHERE id = ?1 AND file_path = ?2 AND tool = ?3",
            params![observation.id, locator, observation.assistant.to_storage()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((stored_scope, missing)) = owner else {
        return Ok(0);
    };
    // Legacy database rows may be adopted only by their exact database locator.
    let legacy_database = stored_scope.is_none()
        && observation.kind == SourceKind::DatabaseRecord
        && scope == Some(locator);
    if stored_scope.as_deref() != scope && !legacy_database {
        return Ok(0);
    }
    let fingerprint = if observation.kind == SourceKind::TranscriptFile {
        observation.fingerprint
    } else {
        None
    };
    tx.execute(
        "UPDATE sessions SET
             source_last_seen_at = ?1, source_kind = ?2,
             source_mtime_ns = ?3, source_size = ?4, source_scope = ?5,
             source_missing = CASE WHEN ?6 THEN 0 ELSE source_missing END,
             source_missing_detected_at = CASE WHEN ?6 THEN NULL ELSE source_missing_detected_at END
         WHERE id = ?7 AND file_path = ?8 AND tool = ?9 AND source_scope IS ?10",
        params![
            now,
            observation.kind.to_storage(),
            fingerprint.map(|(mtime, _)| mtime),
            fingerprint.map(|(_, size)| size),
            scope,
            reindexed,
            observation.id,
            locator,
            observation.assistant.to_storage(),
            stored_scope,
        ],
    )?;
    Ok(usize::from(reindexed && missing))
}

impl SourceScope {
    fn contains(&self, observation: &SourceObservation) -> Result<bool> {
        let locator = sql_path(&observation.locator)?;
        Ok(match self {
            Self::PathRoot { assistant, root } => {
                let (lower, upper) = path_prefix_bounds(root)
                    .context("Source root must be an absolute UTF-8 path")?;
                *assistant == observation.assistant
                    && observation.scope.is_none()
                    && locator >= lower.as_str()
                    && locator < upper.as_str()
            }
            Self::Database { path } => {
                sql_path(path)?;
                observation.assistant == AiAssistant::OpenCode
                    && observation.kind == SourceKind::DatabaseRecord
                    && observation.scope.as_deref() == Some(path.as_path())
            }
        })
    }
}

/// Apply observations and infer absence only after a complete enumeration.
/// Candidate selection and all state changes commit or roll back together.
pub(crate) fn reconcile_scope(conn: &mut Connection, scan: &ScopeScan, now: i64) -> Result<usize> {
    let tx = conn.transaction()?;
    for observation in &scan.observations {
        if scan.scope.contains(observation)? {
            record_observation_tx(&tx, observation, now, false)?;
        }
    }
    if !scan.complete {
        tx.commit()?;
        return Ok(0);
    }

    let (assistant, scope, candidates) = match &scan.scope {
        SourceScope::PathRoot { assistant, root } => {
            let (lower, upper) =
                path_prefix_bounds(root).context("Source root must be an absolute UTF-8 path")?;
            let mut stmt = tx.prepare(
                "SELECT id, file_path FROM sessions
                 WHERE tool = ?1 AND source_scope IS NULL
                   AND file_path >= ?2 COLLATE BINARY AND file_path < ?3 COLLATE BINARY",
            )?;
            let candidates = stmt
                .query_map(params![assistant.to_storage(), lower, upper], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            (*assistant, None, candidates)
        }
        SourceScope::Database { path } => {
            let path = sql_path(path)?;
            // Only complete enumeration proves which database owns legacy rows.
            // Adoption never invents positive last-seen or fingerprint evidence.
            tx.execute(
                "UPDATE sessions SET source_scope = ?1
                 WHERE tool = 'opencode' AND source_scope IS NULL AND file_path = ?1",
                [path],
            )?;
            let mut stmt = tx.prepare(
                "SELECT id, file_path FROM sessions WHERE tool = 'opencode' AND source_scope = ?1",
            )?;
            let candidates = stmt
                .query_map([path], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            (AiAssistant::OpenCode, Some(path), candidates)
        }
    };

    let mut transitions = 0;
    for (id, locator) in candidates {
        if scan.discovered_ids.contains(&id)
            || scan
                .protected_locators
                .iter()
                .any(|protected| Path::new(&locator).starts_with(protected))
        {
            continue;
        }
        transitions += tx.execute(
            "UPDATE sessions SET source_missing = 1,
                 source_missing_detected_at = COALESCE(source_missing_detected_at, ?1)
             WHERE id = ?2 AND file_path = ?3 AND tool = ?4
               AND source_scope IS ?5 AND source_missing = 0",
            params![now, id, locator, assistant.to_storage(), scope],
        )?;
    }
    tx.commit()?;
    Ok(transitions)
}

#[cfg(test)]
mod tests;
