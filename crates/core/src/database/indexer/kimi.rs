use super::{IndexingStats, SessionIndexer, push_indexing_error};
use crate::database::source_state::{
    ScopeScan, SourceObservation, SourceScope, path_prefix_bounds, reconcile_scope,
    record_observation_tx,
};
use crate::models::{AiAssistant, IndexingError, SourceKind};
use crate::parsers::kimi_code::{
    KimiCodeParser, KimiParsedBundle, ParseError, validate_bundle_path,
};
use anyhow::{Result, bail};
use rusqlite::OptionalExtension;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
struct KimiCandidate {
    session_dir: PathBuf,
    main_session_id: String,
    required_paths: RequiredPaths,
}

#[derive(Debug)]
struct KimiDiscovery {
    candidates: Vec<KimiCandidate>,
    discovered_dirs: HashSet<PathBuf>,
    /// Subtrees this scan could not enumerate. They keep whatever state they
    /// already have instead of failing reconciliation for the whole home.
    protected_dirs: HashSet<PathBuf>,
    enumeration_complete: bool,
    errors: usize,
}

#[derive(Debug)]
enum RequiredPaths {
    Ready,
    Incomplete,
    Invalid { path: PathBuf, message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PathFingerprint {
    path: PathBuf,
    mtime_ns: i64,
    size: i64,
}

struct KimiReplaceOutcome {
    removed: usize,
    source_state_changes: usize,
}

#[allow(clippy::large_enum_variant)] // Keep the task-defined stable parse interface unboxed.
enum StableParse {
    Bundle(KimiParsedBundle, Vec<PathFingerprint>),
    NoUserMessages(Vec<PathFingerprint>),
    Incomplete,
}

fn discover_kimi_sessions(
    kimi_home: &Path,
    errors: &mut VecDeque<IndexingError>,
) -> Result<KimiDiscovery> {
    let sessions_dir = kimi_home.join("sessions");
    let mut discovery = KimiDiscovery {
        candidates: Vec::new(),
        discovered_dirs: HashSet::new(),
        protected_dirs: HashSet::new(),
        enumeration_complete: true,
        errors: 0,
    };
    if !trusted_kimi_directory(kimi_home, true, &mut discovery, errors)
        || !trusted_kimi_directory(&sessions_dir, true, &mut discovery, errors)
    {
        return Ok(discovery);
    }
    let workspaces = sorted_dirs(&sessions_dir, None, &mut discovery, errors);
    for workspace in workspaces {
        if !workspace
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("wd_"))
        {
            continue;
        }
        for session_dir in sorted_dirs(&workspace, Some(&workspace), &mut discovery, errors) {
            let Some(session_id) = session_dir
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
            else {
                continue;
            };
            if !session_id.starts_with("session_") {
                continue;
            }
            discovery.discovered_dirs.insert(session_dir.clone());
            discovery.candidates.push(KimiCandidate {
                required_paths: classify_required_kimi_paths(&session_dir),
                session_dir,
                main_session_id: session_id,
            });
        }
    }
    Ok(discovery)
}

fn trusted_kimi_directory(
    path: &Path,
    missing_is_ok: bool,
    discovery: &mut KimiDiscovery,
    errors: &mut VecDeque<IndexingError>,
) -> bool {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => true,
        Err(error) if missing_is_ok && error.kind() == std::io::ErrorKind::NotFound => false,
        result => {
            let message = match result {
                Ok(_) => "Kimi session directory is not a trusted directory",
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    "Kimi home directory was not found"
                }
                Err(_) => "Failed to inspect Kimi session directory",
            };
            record_discovery_error(discovery, errors, path, message);
            false
        }
    }
}

fn record_discovery_error(
    discovery: &mut KimiDiscovery,
    errors: &mut VecDeque<IndexingError>,
    path: &Path,
    message: impl Into<String>,
) {
    discovery.enumeration_complete = false;
    discovery.errors += 1;
    push_indexing_error(
        errors,
        AiAssistant::KimiCode,
        Some(path.display().to_string()),
        message,
    );
}

/// Report a discovery failure, excluding only `protect` when the failure is
/// scoped to one subtree and failing the whole enumeration otherwise.
fn record_scoped_discovery_error(
    discovery: &mut KimiDiscovery,
    errors: &mut VecDeque<IndexingError>,
    path: &Path,
    message: impl Into<String>,
    protect: Option<&Path>,
) {
    match protect {
        Some(protected) => {
            discovery.protected_dirs.insert(protected.to_path_buf());
            discovery.errors += 1;
            push_indexing_error(
                errors,
                AiAssistant::KimiCode,
                Some(path.display().to_string()),
                message,
            );
        }
        None => record_discovery_error(discovery, errors, path, message),
    }
}

fn classify_required_kimi_paths(session_dir: &Path) -> RequiredPaths {
    let mut missing = false;
    for path in [
        session_dir.join("state.json"),
        session_dir.join("agents/main/wire.jsonl"),
    ] {
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
            Ok(_) => {
                return RequiredPaths::Invalid {
                    path,
                    message: "Required Kimi session path is not a regular file".to_string(),
                };
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => missing = true,
            Err(_) => {
                return RequiredPaths::Invalid {
                    path,
                    message: "Failed to inspect required Kimi session path".to_string(),
                };
            }
        }
    }
    if missing {
        RequiredPaths::Incomplete
    } else {
        RequiredPaths::Ready
    }
}

/// List the directories directly under `path`.
///
/// `protect` names the subtree to exclude from reconciliation when this level
/// cannot be read: a workspace that could not be enumerated proves nothing
/// about its bundles, while a failure to read `sessions/` itself leaves the
/// whole home unreconcilable.
fn sorted_dirs(
    path: &Path,
    protect: Option<&Path>,
    discovery: &mut KimiDiscovery,
    errors: &mut VecDeque<IndexingError>,
) -> Vec<PathBuf> {
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(err) => {
            record_scoped_discovery_error(
                discovery,
                errors,
                path,
                format!("Failed to list Kimi session directory: {err}"),
                protect,
            );
            return Vec::new();
        }
    };
    let mut dirs = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => handle_directory_file_type(
                entry.path(),
                entry.file_type(),
                &mut dirs,
                discovery,
                errors,
            ),
            Err(err) => {
                record_scoped_discovery_error(
                    discovery,
                    errors,
                    path,
                    format!("Failed to read Kimi session directory entry: {err}"),
                    protect,
                );
            }
        }
    }
    dirs.sort();
    dirs
}

fn handle_directory_file_type(
    entry_path: PathBuf,
    file_type: std::io::Result<fs::FileType>,
    dirs: &mut Vec<PathBuf>,
    discovery: &mut KimiDiscovery,
    errors: &mut VecDeque<IndexingError>,
) {
    match file_type {
        Ok(file_type) if file_type.is_dir() => dirs.push(entry_path),
        Ok(_) => {}
        Err(err) => {
            record_discovery_error(
                discovery,
                errors,
                &entry_path,
                format!("Failed to read Kimi session directory entry file type: {err}"),
            );
        }
    }
}

fn snapshot_dependencies(session_dir: &Path, paths: &[PathBuf]) -> Result<Vec<PathFingerprint>> {
    let mut paths = paths.to_vec();
    paths.sort();
    paths.dedup();
    paths
        .into_iter()
        .map(|path| {
            validate_bundle_path(session_dir, session_dir, &path)?;
            let (mtime_ns, size) = SessionIndexer::current_fingerprint(&path)?;
            Ok(PathFingerprint {
                path,
                mtime_ns,
                size,
            })
        })
        .collect()
}

// Use path_prefix_bounds from source_state for consistent ownership queries.

impl SessionIndexer {
    fn stored_kimi_fingerprints(&self, session_dir: &Path) -> Result<HashMap<PathBuf, (i64, i64)>> {
        let Some((lower, upper)) = path_prefix_bounds(session_dir) else {
            return Ok(HashMap::new());
        };
        let mut stmt = self.db.prepare(
            "SELECT file_path, mtime_ns, size FROM file_fingerprints
             WHERE file_path >= ?1 COLLATE BINARY AND file_path < ?2 COLLATE BINARY",
        )?;
        Ok(stmt
            .query_map(rusqlite::params![lower, upper], |row| {
                Ok((
                    PathBuf::from(row.get::<_, String>(0)?),
                    (row.get(1)?, row.get(2)?),
                ))
            })?
            .collect::<std::result::Result<_, _>>()?)
    }

    #[cfg(test)]
    fn upsert_kimi_fingerprints(&mut self, snapshot: &[PathFingerprint]) -> Result<()> {
        let tx = self.db.transaction()?;
        for fingerprint in snapshot {
            Self::upsert_fingerprint_values_tx(
                &tx,
                &fingerprint.path,
                fingerprint.mtime_ns,
                fingerprint.size,
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn should_reindex_kimi_bundle(
        &self,
        session_dir: &Path,
        snapshot: &[PathFingerprint],
    ) -> Result<bool> {
        let stored = self.stored_kimi_fingerprints(session_dir)?;
        let current = snapshot
            .iter()
            .map(|fingerprint| {
                (
                    fingerprint.path.clone(),
                    (fingerprint.mtime_ns, fingerprint.size),
                )
            })
            .collect::<HashMap<_, _>>();
        Ok(stored != current)
    }

    pub(super) fn index_kimi_sessions_internal(
        &mut self,
        kimi_home: &Path,
        incremental: bool,
        errors_detail: &mut VecDeque<IndexingError>,
    ) -> Result<IndexingStats> {
        // Source evidence is stored and compared as absolute paths, so every
        // locator this scan produces must already be rooted.
        let kimi_home = &Self::absolute_source_path(kimi_home)?;
        let discovery = discover_kimi_sessions(kimi_home, errors_detail)?;
        let parser = KimiCodeParser::new(kimi_home);
        self.process_kimi_discovery(kimi_home, incremental, discovery, &parser, errors_detail)
    }

    pub fn index_kimi_sessions(&mut self, kimi_home: &Path) -> Result<usize> {
        let mut errors_detail = VecDeque::new();
        Ok(self
            .index_kimi_sessions_internal(kimi_home, false, &mut errors_detail)?
            .indexed)
    }

    fn process_kimi_discovery(
        &mut self,
        kimi_home: &Path,
        incremental: bool,
        discovery: KimiDiscovery,
        parser: &KimiCodeParser,
        errors_detail: &mut VecDeque<IndexingError>,
    ) -> Result<IndexingStats> {
        let sessions_dir = kimi_home.join("sessions");
        let mut stats = IndexingStats {
            errors: discovery.errors,
            ..IndexingStats::default()
        };
        let mut scan = ScopeScan {
            scope: SourceScope::PathRoot {
                assistant: AiAssistant::KimiCode,
                root: sessions_dir.clone(),
            },
            complete: discovery.enumeration_complete && kimi_home.is_dir() && sessions_dir.is_dir(),
            discovered_ids: HashSet::new(),
            protected_locators: discovery.protected_dirs,
            observations: Vec::new(),
        };
        // Only a bundle this scan actually read proves what it currently owns.
        // Every other discovered directory keeps the state it already has.
        let mut observed_dirs: HashSet<PathBuf> = HashSet::new();

        for candidate in discovery.candidates {
            match candidate.required_paths {
                RequiredPaths::Ready => {}
                RequiredPaths::Incomplete => {
                    self.report_incomplete_kimi_bundle(
                        &candidate.session_dir,
                        &mut stats,
                        errors_detail,
                    )?;
                    stats.skipped += 1;
                    continue;
                }
                RequiredPaths::Invalid { path, message } => {
                    push_indexing_error(
                        errors_detail,
                        AiAssistant::KimiCode,
                        Some(path.display().to_string()),
                        message,
                    );
                    stats.errors += 1;
                    continue;
                }
            }
            let snapshot = match snapshot_kimi_bundle(parser, &candidate.session_dir) {
                Ok(Some(snapshot)) => snapshot,
                // A required file vanished after discovery: the bundle is not
                // proven absent, so its retained data stays untouched.
                Ok(None) => {
                    stats.skipped += 1;
                    continue;
                }
                Err(err) => {
                    let error_path = err
                        .downcast_ref::<ParseError>()
                        .and_then(ParseError::invalid_path)
                        .unwrap_or(&candidate.session_dir);
                    self.record_index_failure(
                        AiAssistant::KimiCode,
                        error_path,
                        &err,
                        &mut stats,
                        errors_detail,
                    );
                    continue;
                }
            };
            // An unchanged bundle may skip content only while none of the rows
            // it owns is still marked missing: a returned bundle keeps every
            // dependency fingerprint, so nothing else would force the reindex.
            if incremental
                && !self.kimi_bundle_has_missing_rows(&candidate.session_dir)?
                && !self.should_reindex_kimi_bundle(&candidate.session_dir, &snapshot)?
            {
                let owned = self.kimi_owned_rows(&candidate.session_dir)?;
                Self::observe_kimi_rows(&mut scan, owned);
                observed_dirs.insert(candidate.session_dir.clone());
                stats.skipped += 1;
                continue;
            }

            match parse_stable_bundle(parser, &candidate.session_dir, snapshot) {
                Ok(StableParse::Bundle(bundle, snapshot)) => {
                    match self.replace_kimi_bundle(&bundle, &snapshot, &candidate.session_dir) {
                        // Children that `state.json` no longer declares are deleted
                        // by the replace, so they count as removals even though the
                        // bundle itself was reindexed.
                        Ok(outcome) => {
                            stats.removed += outcome.removed;
                            stats.indexed += 1;
                            stats.source_state_changes += outcome.source_state_changes;
                            for session_id in &bundle.session_ids {
                                scan.discovered_ids.insert(session_id.clone());
                            }
                            observed_dirs.insert(candidate.session_dir.clone());
                        }
                        Err(err) => self.record_index_failure(
                            AiAssistant::KimiCode,
                            &candidate.session_dir,
                            &err,
                            &mut stats,
                            errors_detail,
                        ),
                    }
                }
                Ok(StableParse::NoUserMessages(_snapshot)) => {
                    match self.prune_kimi_no_user_bundle(
                        &candidate.session_dir,
                        &candidate.main_session_id,
                    ) {
                        Ok(removed) => stats.removed += removed,
                        Err(err) => self.record_index_failure(
                            AiAssistant::KimiCode,
                            &candidate.session_dir,
                            &err,
                            &mut stats,
                            errors_detail,
                        ),
                    }
                }
                Ok(StableParse::Incomplete) => stats.skipped += 1,
                Err(err) => self.record_index_failure(
                    AiAssistant::KimiCode,
                    err.downcast_ref::<ParseError>()
                        .and_then(ParseError::invalid_path)
                        .unwrap_or(&candidate.session_dir),
                    &err,
                    &mut stats,
                    errors_detail,
                ),
            }
        }

        for session_dir in &discovery.discovered_dirs {
            if !observed_dirs.contains(session_dir) {
                scan.protected_locators.insert(session_dir.clone());
            }
        }
        stats.source_state_changes +=
            reconcile_scope(&mut self.db, &scan, chrono::Utc::now().timestamp())?;
        Ok(stats)
    }

    /// Report a surviving bundle whose required files are absent, but only once
    /// it has actually been indexed: a candidate that was never readable is not
    /// a regression worth a diagnostic.
    fn report_incomplete_kimi_bundle(
        &self,
        session_dir: &Path,
        stats: &mut IndexingStats,
        errors_detail: &mut VecDeque<IndexingError>,
    ) -> Result<()> {
        if self.kimi_owned_rows(session_dir)?.is_empty() {
            return Ok(());
        }
        push_indexing_error(
            errors_detail,
            AiAssistant::KimiCode,
            Some(session_dir.display().to_string()),
            "Kimi session bundle is missing a required file".to_string(),
        );
        stats.errors += 1;
        Ok(())
    }

    /// The sessions currently stored at or under a bundle directory.
    fn kimi_owned_rows(&self, session_dir: &Path) -> Result<Vec<(String, PathBuf)>> {
        let Some(locator) = session_dir.to_str() else {
            return Ok(Vec::new());
        };
        let Some((lower, upper)) = path_prefix_bounds(session_dir) else {
            return Ok(Vec::new());
        };
        let mut statement = self.db.prepare(
            "SELECT id, file_path FROM sessions
             WHERE tool = 'kimi_code'
               AND (file_path = ?1
                    OR (file_path >= ?2 COLLATE BINARY AND file_path < ?3 COLLATE BINARY))",
        )?;
        Ok(statement
            .query_map(rusqlite::params![locator, lower, upper], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    PathBuf::from(row.get::<_, String>(1)?),
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn kimi_bundle_has_missing_rows(&self, session_dir: &Path) -> Result<bool> {
        let Some(locator) = session_dir.to_str() else {
            return Ok(false);
        };
        let Some((lower, upper)) = path_prefix_bounds(session_dir) else {
            return Ok(false);
        };
        Ok(self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sessions
             WHERE tool = 'kimi_code'
               AND (file_path = ?1
                    OR (file_path >= ?2 COLLATE BINARY AND file_path < ?3 COLLATE BINARY))
               AND source_missing = 1)",
            rusqlite::params![locator, lower, upper],
            |row| row.get::<_, i64>(0),
        )? != 0)
    }

    fn observe_kimi_rows(scan: &mut ScopeScan, rows: Vec<(String, PathBuf)>) {
        for (id, locator) in rows {
            scan.discovered_ids.insert(id.clone());
            scan.observations.push(SourceObservation {
                assistant: AiAssistant::KimiCode,
                id,
                locator,
                kind: SourceKind::SessionBundle,
                scope: None,
                fingerprint: None,
            });
        }
    }

    fn replace_kimi_bundle(
        &mut self,
        bundle: &KimiParsedBundle,
        snapshot: &[PathFingerprint],
        session_dir: &Path,
    ) -> Result<KimiReplaceOutcome> {
        let main_id = &bundle.main.session.id;
        let child_prefix = format!("kimi-subagent::{main_id}::");
        let tx = self.db.transaction()?;

        for session_id in &bundle.session_ids {
            if let Some(tool) = tx
                .query_row(
                    "SELECT tool FROM sessions WHERE id = ?1",
                    [session_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                && tool != "kimi_code"
            {
                bail!("Kimi session id {session_id} is already owned by {tool}");
            }
        }
        // Only children this bundle still owns and still has on disk may be
        // dropped: an identity prefix alone would reach into another home, and
        // a retained missing row is not evidence of an undeclared child.
        let old_children: Vec<String> = Self::present_kimi_rows_owned_by_tx(&tx, session_dir)?
            .into_iter()
            .filter(|id| id.starts_with(&child_prefix))
            .collect();

        let main_project =
            Self::upsert_project_tx(&tx, bundle.main.session.project_path.as_deref())?;
        Self::upsert_session_row_tx(
            &tx,
            &bundle.main,
            Path::new(&bundle.main.session.file_path),
            main_project,
        )?;
        for child in &bundle.children {
            let project = Self::upsert_project_tx(&tx, child.session.project_path.as_deref())?;
            Self::upsert_session_row_tx(&tx, child, Path::new(&child.session.file_path), project)?;
        }
        Self::replace_session_contents_preserving_links_tx(&tx, &bundle.main)?;
        for child in &bundle.children {
            Self::replace_session_contents_preserving_links_tx(&tx, child)?;
        }
        let now = chrono::Utc::now().timestamp();
        let mut source_state_changes = 0;
        for parsed in std::iter::once(&bundle.main).chain(bundle.children.iter()) {
            source_state_changes += record_observation_tx(
                &tx,
                &SourceObservation {
                    assistant: AiAssistant::KimiCode,
                    id: parsed.session.id.clone(),
                    locator: PathBuf::from(&parsed.session.file_path),
                    kind: SourceKind::SessionBundle,
                    scope: None,
                    fingerprint: None,
                },
                now,
                true,
            )?;
        }
        let mut removed = 0;
        for child_id in old_children {
            if !bundle.session_ids.contains(&child_id) {
                removed += Self::delete_session_by_id_tx(&tx, &child_id)?;
            }
        }
        if let Some((lower, upper)) = path_prefix_bounds(Path::new(&bundle.main.session.file_path))
        {
            tx.execute(
                "DELETE FROM file_fingerprints WHERE file_path >= ?1 COLLATE BINARY AND file_path < ?2 COLLATE BINARY",
                rusqlite::params![lower, upper],
            )?;
        }
        for fingerprint in snapshot {
            Self::upsert_fingerprint_values_tx(
                &tx,
                &fingerprint.path,
                fingerprint.mtime_ns,
                fingerprint.size,
            )?;
        }
        tx.commit()?;
        Ok(KimiReplaceOutcome {
            removed,
            source_state_changes,
        })
    }

    /// The available session ids stored at or under a bundle directory.
    fn present_kimi_rows_owned_by_tx(
        tx: &rusqlite::Transaction<'_>,
        session_dir: &Path,
    ) -> Result<Vec<String>> {
        let Some(locator) = session_dir.to_str() else {
            return Ok(Vec::new());
        };
        let Some((lower, upper)) = path_prefix_bounds(session_dir) else {
            return Ok(Vec::new());
        };
        let mut statement = tx.prepare(
            "SELECT id FROM sessions
             WHERE tool = 'kimi_code' AND source_missing = 0
               AND (file_path = ?1
                    OR (file_path >= ?2 COLLATE BINARY AND file_path < ?3 COLLATE BINARY))",
        )?;
        Ok(statement
            .query_map(rusqlite::params![locator, lower, upper], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Remove a present, fully read bundle that carries no user message.
    ///
    /// Ineligibility is proven only for the rows this bundle currently owns and
    /// that are actually available; a retained missing row is never removed by
    /// a scan that could not read the source it came from.
    fn prune_kimi_no_user_bundle(&mut self, session_dir: &Path, main_id: &str) -> Result<usize> {
        let child_prefix = format!("kimi-subagent::{main_id}::");
        let tx = self.db.transaction()?;
        let ids: Vec<String> = Self::present_kimi_rows_owned_by_tx(&tx, session_dir)?
            .into_iter()
            .filter(|id| id == main_id || id.starts_with(&child_prefix))
            .collect();
        let mut removed = 0;
        for id in ids {
            removed += Self::delete_session_by_id_tx(&tx, &id)?;
        }
        // Only clear fingerprints if we actually removed rows. Never delete
        // fingerprints for a bundle with retained-missing content.
        if removed > 0
            && let Some((lower, upper)) = path_prefix_bounds(session_dir)
        {
            tx.execute(
                "DELETE FROM file_fingerprints WHERE file_path >= ?1 COLLATE BINARY AND file_path < ?2 COLLATE BINARY",
                rusqlite::params![lower, upper],
            )?;
        }
        tx.commit()?;
        Ok(removed)
    }
}

#[cfg(test)]
impl SessionIndexer {
    pub fn index_kimi_sessions_incremental(&mut self, kimi_home: &Path) -> Result<IndexingStats> {
        let mut errors_detail = VecDeque::new();
        self.index_kimi_sessions_internal(kimi_home, true, &mut errors_detail)
    }
}

fn required_kimi_files_missing(session_dir: &Path) -> bool {
    [
        session_dir.join("state.json"),
        session_dir.join("agents/main/wire.jsonl"),
    ]
    .iter()
    .any(|path| {
        fs::symlink_metadata(path).is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    })
}

fn snapshot_kimi_bundle(
    parser: &KimiCodeParser,
    session_dir: &Path,
) -> Result<Option<Vec<PathFingerprint>>> {
    let paths = match parser.dependency_paths(session_dir) {
        Ok(paths) => paths,
        Err(_) if required_kimi_files_missing(session_dir) => return Ok(None),
        Err(error) => return Err(error),
    };
    match snapshot_dependencies(session_dir, &paths) {
        Ok(snapshot) => Ok(Some(snapshot)),
        Err(_) if required_kimi_files_missing(session_dir) => Ok(None),
        Err(error) => Err(error),
    }
}

fn parse_stable_bundle(
    parser: &KimiCodeParser,
    session_dir: &Path,
    mut before: Vec<PathFingerprint>,
) -> Result<StableParse> {
    for _ in 0..2 {
        let parsed = match parser.parse_session_dir(session_dir) {
            Ok(bundle) => Ok(bundle),
            Err(_) if required_kimi_files_missing(session_dir) => {
                return Ok(StableParse::Incomplete);
            }
            Err(error) => Err(error),
        };
        let Some(after) = snapshot_kimi_bundle(parser, session_dir)? else {
            return Ok(StableParse::Incomplete);
        };
        if before != after {
            before = after;
            continue;
        }
        return match parsed {
            Ok(bundle) => Ok(StableParse::Bundle(bundle, after)),
            Err(err)
                if matches!(
                    err.downcast_ref::<ParseError>(),
                    Some(ParseError::NoUserMessages)
                ) =>
            {
                Ok(StableParse::NoUserMessages(after))
            }
            Err(err) => Err(err),
        };
    }
    bail!("Kimi session changed while being parsed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::fs;
    use std::io::Write;
    use std::path::Path;

    fn copy_dir(source: &Path, destination: &Path) {
        fs::create_dir_all(destination).unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let target = destination.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_dir(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), target).unwrap();
            }
        }
    }

    fn fixture_home() -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        copy_dir(&crate::fixture_path("kimi_home"), temp.path());
        temp
    }

    fn primary_dir(home: &Path) -> PathBuf {
        home.join("sessions/wd_primary_aaaaaaaaaaaa/session_00000000-0000-4000-8000-000000000001")
    }

    #[test]
    fn incremental_kimi_bundle_reindexes_each_changed_dependency() {
        let home = fixture_home();
        let db = tempfile::NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(db.path()).unwrap();
        let dir = primary_dir(home.path());

        let first = indexer
            .index_kimi_sessions_incremental(home.path())
            .unwrap();
        assert_eq!(first.indexed, 7);
        let unchanged = indexer
            .index_kimi_sessions_incremental(home.path())
            .unwrap();
        assert_eq!(unchanged.indexed, 0);
        assert_eq!(unchanged.skipped, 7);

        let mut state: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join("state.json")).unwrap()).unwrap();
        state["title"] = serde_json::json!("replacement title");
        fs::write(dir.join("state.json"), serde_json::to_vec(&state).unwrap()).unwrap();
        assert_eq!(
            indexer
                .index_kimi_sessions_incremental(home.path())
                .unwrap()
                .indexed,
            1
        );

        fs::OpenOptions::new()
            .append(true)
            .open(dir.join("agents/main/wire.jsonl"))
            .unwrap()
            .write_all(b"\nnot-json\n")
            .unwrap();
        assert_eq!(
            indexer
                .index_kimi_sessions_incremental(home.path())
                .unwrap()
                .indexed,
            1
        );

        let child_content = "Persisted child append";
        writeln!(
            fs::OpenOptions::new()
                .append(true)
                .open(dir.join("agents/agent-0/wire.jsonl"))
                .unwrap(),
            r#"{{"type":"context.append_loop_event","time":1785320010000,"event":{{"type":"content.part","stepUuid":"child-step","part":{{"type":"text","text":"{child_content}"}}}}}}"#
        )
        .unwrap();
        assert_eq!(
            indexer
                .index_kimi_sessions_incremental(home.path())
                .unwrap()
                .indexed,
            1
        );
        assert_eq!(
            indexer
                .db
                .query_row(
                    "SELECT COUNT(*) FROM messages WHERE session_id = ?1 AND content = ?2",
                    rusqlite::params![
                        "kimi-subagent::session_00000000-0000-4000-8000-000000000001::agent-0",
                        child_content
                    ],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
    }

    #[test]
    fn dropping_a_child_from_state_counts_as_a_removal() {
        let home = fixture_home();
        let db = tempfile::NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(db.path()).unwrap();
        let dir = primary_dir(home.path());
        let child_id = "kimi-subagent::session_00000000-0000-4000-8000-000000000001::agent-1";

        let child_rows = |indexer: &SessionIndexer| {
            indexer
                .db
                .query_row(
                    "SELECT COUNT(*) FROM sessions WHERE id = ?1",
                    [child_id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap()
        };

        indexer
            .index_kimi_sessions_incremental(home.path())
            .unwrap();
        assert_eq!(child_rows(&indexer), 1);

        let mut state: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join("state.json")).unwrap()).unwrap();
        state["agents"].as_object_mut().unwrap().remove("agent-1");
        fs::write(dir.join("state.json"), serde_json::to_vec(&state).unwrap()).unwrap();

        let stats = indexer
            .index_kimi_sessions_incremental(home.path())
            .unwrap();

        assert_eq!(stats.indexed, 1);
        assert_eq!(stats.removed, 1);
        assert_eq!(stats.errors, 0);
        assert_eq!(child_rows(&indexer), 0);
    }

    #[test]
    fn discovery_only_returns_direct_workspace_session_bundles() {
        let temp = fixture_home();
        let sessions = temp.path().join("sessions");
        fs::create_dir_all(temp.path().join("user-history")).unwrap();
        fs::create_dir_all(temp.path().join("credentials")).unwrap();
        fs::create_dir_all(temp.path().join("logs")).unwrap();
        fs::create_dir_all(sessions.join("not-a-workspace")).unwrap();
        fs::create_dir_all(sessions.join("wd_valid/not-a-session")).unwrap();

        let mut errors = VecDeque::new();
        let discovery = discover_kimi_sessions(temp.path(), &mut errors).unwrap();

        assert!(discovery.enumeration_complete);
        assert_eq!(discovery.candidates.len(), 8);
        assert!(errors.is_empty());
        assert!(discovery.candidates.iter().all(|candidate| {
            candidate
                .session_dir
                .parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("wd_")
        }));
        assert!(
            discovery
                .candidates
                .iter()
                .all(|candidate| candidate.main_session_id.starts_with("session_"))
        );
        assert!(
            discovery
                .candidates
                .iter()
                .all(|candidate| matches!(candidate.required_paths, RequiredPaths::Ready))
        );
    }

    #[test]
    fn discovery_marks_file_type_failures_incomplete_and_reports_them() {
        let temp = tempfile::tempdir().unwrap();
        let mut discovery = KimiDiscovery {
            candidates: Vec::new(),
            discovered_dirs: HashSet::new(),
            protected_dirs: HashSet::new(),
            enumeration_complete: true,
            errors: 0,
        };
        let mut errors = VecDeque::new();

        handle_directory_file_type(
            temp.path().to_path_buf(),
            Err(std::io::Error::other("simulated file type failure")),
            &mut Vec::new(),
            &mut discovery,
            &mut errors,
        );

        assert!(!discovery.enumeration_complete);
        assert_eq!(discovery.errors, 1);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].assistant, AiAssistant::KimiCode);
        assert!(errors[0].message.contains("file type"));
        let source_result = super::super::build_per_source_result(
            AiAssistant::KimiCode,
            temp.path().display().to_string(),
            true,
            IndexingStats {
                errors: discovery.errors,
                ..IndexingStats::default()
            },
        );
        assert_eq!(source_result.errors, 1);
        assert_eq!(source_result.status, crate::models::SourceStatus::Failed);
    }

    #[test]
    fn production_incremental_skips_initially_incomplete_candidate_without_diagnostic() {
        for required_path in ["state.json", "agents/main/wire.jsonl"] {
            let temp = fixture_home();
            fs::remove_file(primary_dir(temp.path()).join(required_path)).unwrap();
            let db = tempfile::NamedTempFile::new().unwrap();
            let mut indexer = SessionIndexer::new(db.path()).unwrap();
            let mut errors = VecDeque::new();

            let stats = indexer
                .index_kimi_sessions_internal(temp.path(), true, &mut errors)
                .unwrap();

            assert_eq!(stats.indexed, 6);
            assert_eq!(stats.skipped, 1);
            assert_eq!(stats.errors, 0);
            assert!(errors.is_empty());
        }
    }

    #[test]
    fn production_incremental_preserves_bundle_when_main_file_disappears_after_discovery() {
        let temp = fixture_home();
        let session_dir = primary_dir(temp.path());
        let db = tempfile::NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(db.path()).unwrap();
        indexer.index_kimi_sessions(temp.path()).unwrap();
        let before_fingerprints = indexer.stored_kimi_fingerprints(&session_dir).unwrap();
        let before_messages: i64 = indexer
            .db
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE session_id = ?1 OR session_id LIKE ?2",
                rusqlite::params![
                    "session_00000000-0000-4000-8000-000000000001",
                    "kimi-subagent::session_00000000-0000-4000-8000-000000000001::%"
                ],
                |row| row.get(0),
            )
            .unwrap();
        let mut errors = VecDeque::new();
        let discovery = discover_kimi_sessions(temp.path(), &mut errors).unwrap();
        fs::remove_file(session_dir.join("agents/main/wire.jsonl")).unwrap();
        let parser = KimiCodeParser::new(temp.path());

        let stats = indexer
            .process_kimi_discovery(temp.path(), true, discovery, &parser, &mut errors)
            .unwrap();

        assert_eq!(stats.errors, 0);
        assert!(errors.is_empty());
        assert_eq!(
            indexer.stored_kimi_fingerprints(&session_dir).unwrap(),
            before_fingerprints
        );
        assert_eq!(
            indexer
                .db
                .query_row(
                    "SELECT COUNT(*) FROM messages WHERE session_id = ?1 OR session_id LIKE ?2",
                    rusqlite::params![
                        "session_00000000-0000-4000-8000-000000000001",
                        "kimi-subagent::session_00000000-0000-4000-8000-000000000001::%"
                    ],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            before_messages
        );
    }

    #[test]
    fn production_incremental_reports_unrelated_state_error() {
        let temp = fixture_home();
        let session_dir = primary_dir(temp.path());
        let db = tempfile::NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(db.path()).unwrap();
        indexer.index_kimi_sessions(temp.path()).unwrap();
        let mut errors = VecDeque::new();
        let discovery = discover_kimi_sessions(temp.path(), &mut errors).unwrap();
        fs::write(session_dir.join("state.json"), "not-json").unwrap();
        let parser = KimiCodeParser::new(temp.path());

        let stats = indexer
            .process_kimi_discovery(temp.path(), true, discovery, &parser, &mut errors)
            .unwrap();

        assert_eq!(stats.errors, 1);
        assert_eq!(errors.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn production_incremental_reports_dangling_required_symlink_and_preserves_bundle() {
        use std::os::unix::fs::symlink;

        let temp = fixture_home();
        let session_dir = primary_dir(temp.path());
        let db = tempfile::NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(db.path()).unwrap();
        indexer.index_kimi_sessions(temp.path()).unwrap();
        let before_fingerprints = indexer.stored_kimi_fingerprints(&session_dir).unwrap();
        fs::remove_file(session_dir.join("state.json")).unwrap();
        symlink("missing-state.json", session_dir.join("state.json")).unwrap();
        let mut errors = VecDeque::new();

        let stats = indexer
            .index_kimi_sessions_internal(temp.path(), true, &mut errors)
            .unwrap();

        assert_eq!(stats.errors, 1);
        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors[0].location.as_deref(),
            session_dir.join("state.json").to_str()
        );
        assert_eq!(
            indexer.stored_kimi_fingerprints(&session_dir).unwrap(),
            before_fingerprints
        );
    }

    #[test]
    fn production_incremental_reports_non_file_required_path_and_preserves_bundle() {
        let temp = fixture_home();
        let session_dir = primary_dir(temp.path());
        let db = tempfile::NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(db.path()).unwrap();
        indexer.index_kimi_sessions(temp.path()).unwrap();
        let before_fingerprints = indexer.stored_kimi_fingerprints(&session_dir).unwrap();
        fs::remove_file(session_dir.join("agents/main/wire.jsonl")).unwrap();
        fs::create_dir(session_dir.join("agents/main/wire.jsonl")).unwrap();
        let mut errors = VecDeque::new();

        let stats = indexer
            .index_kimi_sessions_internal(temp.path(), true, &mut errors)
            .unwrap();

        assert_eq!(stats.errors, 1);
        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors[0].location.as_deref(),
            session_dir.join("agents/main/wire.jsonl").to_str()
        );
        assert_eq!(
            indexer.stored_kimi_fingerprints(&session_dir).unwrap(),
            before_fingerprints
        );
    }

    #[test]
    fn bundle_fingerprints_detect_changes_additions_and_missing_dependencies() {
        let temp = fixture_home();
        let session_dir = temp
            .path()
            .join("sessions/wd_primary_aaaaaaaaaaaa/session_00000000-0000-4000-8000-000000000001");
        let parser = KimiCodeParser::new(temp.path());
        let bundle = parser.parse_session_dir(&session_dir).unwrap();
        let snapshot = snapshot_dependencies(
            &session_dir,
            &parser.dependency_paths(&session_dir).unwrap(),
        )
        .unwrap();
        let db = tempfile::NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(db.path()).unwrap();

        indexer
            .insert_parsed_session(&bundle.main, &session_dir)
            .unwrap();
        indexer.upsert_kimi_fingerprints(&snapshot).unwrap();
        assert!(
            !indexer
                .should_reindex_kimi_bundle(&session_dir, &snapshot)
                .unwrap()
        );

        let mut state: serde_json::Value =
            serde_json::from_slice(&fs::read(session_dir.join("state.json")).unwrap()).unwrap();
        state["title"] = serde_json::json!("Changed title");
        fs::write(
            session_dir.join("state.json"),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();
        let changed_state = snapshot_dependencies(
            &session_dir,
            &parser.dependency_paths(&session_dir).unwrap(),
        )
        .unwrap();
        assert!(
            indexer
                .should_reindex_kimi_bundle(&session_dir, &changed_state)
                .unwrap()
        );
        indexer.upsert_kimi_fingerprints(&changed_state).unwrap();

        fs::write(session_dir.join("agents/main/wire.jsonl"), "main append\n").unwrap();
        let changed_main = snapshot_dependencies(
            &session_dir,
            &parser.dependency_paths(&session_dir).unwrap(),
        )
        .unwrap();
        assert!(
            indexer
                .should_reindex_kimi_bundle(&session_dir, &changed_main)
                .unwrap()
        );
        indexer.upsert_kimi_fingerprints(&changed_main).unwrap();

        let child = session_dir.join("agents/agent-0/wire.jsonl");
        fs::write(&child, "child append\n").unwrap();
        let changed_child = snapshot_dependencies(
            &session_dir,
            &parser.dependency_paths(&session_dir).unwrap(),
        )
        .unwrap();
        assert!(
            indexer
                .should_reindex_kimi_bundle(&session_dir, &changed_child)
                .unwrap()
        );
        indexer.upsert_kimi_fingerprints(&changed_child).unwrap();

        fs::create_dir_all(session_dir.join("agents/agent-new")).unwrap();
        fs::write(session_dir.join("agents/agent-new/wire.jsonl"), "new\n").unwrap();
        state["agents"]["agent-new"] =
            serde_json::json!({"type": "agent", "parentAgentId": "main"});
        fs::write(
            session_dir.join("state.json"),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();
        let expanded = snapshot_dependencies(
            &session_dir,
            &parser.dependency_paths(&session_dir).unwrap(),
        )
        .unwrap();
        assert!(
            indexer
                .should_reindex_kimi_bundle(&session_dir, &expanded)
                .unwrap()
        );
        indexer.upsert_kimi_fingerprints(&expanded).unwrap();

        fs::remove_file(&child).unwrap();
        assert_eq!(indexer.prune_orphan_fingerprints().unwrap(), 0);
        assert!(
            indexer
                .stored_kimi_fingerprints(&session_dir)
                .unwrap()
                .contains_key(&child)
        );
        let current_without_deleted: Vec<_> = expanded
            .iter()
            .filter(|fingerprint| fingerprint.path != child)
            .cloned()
            .collect();
        assert!(
            indexer
                .should_reindex_kimi_bundle(&session_dir, &current_without_deleted)
                .unwrap()
        );
    }

    #[test]
    fn stored_fingerprints_use_boundary_safe_prefix_ranges() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("percent%_root");
        let bundle = root.join("session_one");
        let sibling = root.join("session_one_extra");
        fs::create_dir_all(&bundle).unwrap();
        fs::create_dir_all(&sibling).unwrap();
        let bundle_file = bundle.join("wire.jsonl");
        let sibling_file = sibling.join("wire.jsonl");
        fs::write(&bundle_file, "one\n").unwrap();
        fs::write(&sibling_file, "extra\n").unwrap();

        let db = tempfile::NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(db.path()).unwrap();
        indexer
            .upsert_kimi_fingerprints(
                &snapshot_dependencies(&bundle, &[bundle_file.clone()]).unwrap(),
            )
            .unwrap();
        indexer
            .upsert_kimi_fingerprints(
                &snapshot_dependencies(&sibling, &[sibling_file.clone()]).unwrap(),
            )
            .unwrap();

        let stored = indexer.stored_kimi_fingerprints(&bundle).unwrap();
        assert_eq!(stored.len(), 1);
        assert!(stored.contains_key(&bundle_file));
        assert!(!stored.contains_key(&sibling_file));
    }

    #[test]
    fn no_user_bundle_does_not_clear_fingerprints_when_no_rows_removed() {
        let home = fixture_home();
        let db = tempfile::NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(db.path()).unwrap();
        let dir = primary_dir(home.path());

        // Index the bundle normally (with user messages).
        indexer
            .index_kimi_sessions_incremental(home.path())
            .unwrap();
        let stored_before = indexer.stored_kimi_fingerprints(&dir).unwrap();
        assert!(!stored_before.is_empty());

        // Make the bundle disappear so it becomes retained-missing.
        let saved = home.path().parent().unwrap().join("saved_bundle");
        fs::rename(&dir, &saved).unwrap();
        indexer
            .index_kimi_sessions_incremental(home.path())
            .unwrap();
        assert!(
            indexer
                .db
                .query_row(
                    "SELECT source_missing FROM sessions WHERE file_path = ?1 LIMIT 1",
                    rusqlite::params![dir.to_str().unwrap()],
                    |row| row.get::<_, i32>(0),
                )
                .ok()
                .map(|m| m != 0)
                .unwrap_or(false)
        );

        // Restore the bundle but rewrite main wire.jsonl to have only injection messages.
        fs::rename(&saved, &dir).unwrap();
        let main_wire = dir.join("agents/main/wire.jsonl");
        fs::write(
            &main_wire,
            r#"{"id":"injection-1","role":"user","origin":"injection"}
{"id":"injection-2","role":"assistant","origin":"injection"}
"#,
        )
        .unwrap();

        // Rescan: parse_stable_bundle returns NoUserMessages, and prune_kimi_no_user_bundle
        // should remove 0 rows (the row is still marked missing from the previous scan)
        // and must NOT clear fingerprints.
        indexer
            .index_kimi_sessions_incremental(home.path())
            .unwrap();

        // Fingerprints should still exist because we removed 0 rows.
        let stored_after = indexer.stored_kimi_fingerprints(&dir).unwrap();
        assert!(
            !stored_after.is_empty(),
            "Fingerprints should not be cleared when removing 0 rows"
        );
    }
}
