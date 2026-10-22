//! OpenCode source-aware indexing coordinator.
//!
//! Enumerates every OpenCode source (V2 SQLite, V1 SQLite, and legacy JSON),
//! selects one winning representation per session ID (`V2 > V1 > JSON`, then
//! configured source order), and only then parses the winners. Cached owners
//! whose source is unreadable, unchanged, or simply not discovered are kept as
//! retained candidates so a partial discovery failure never prunes them, while
//! a confirmed missing source still does. Captured DB/WAL fingerprints are
//! committed per source only after every selected session in that source has
//! been processed successfully.

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::database::opencode_sources::{
    self, OpenCodeGeneration, SourceCandidate, SourceFingerprint, SourcePresence,
};
use crate::models::{AiAssistant, IndexingError};
use crate::parsers::ParsedSession;
use crate::parsers::opencode::{
    OpenCodeBackend, OpenCodeParser, ParseError as OpenCodeParseError, SessionEntry, SessionSource,
    json_backend::JsonBackend, sqlite_backend::SqliteBackend, v2::V2Reader,
};

use super::{IndexingStats, SessionIndexer, push_indexing_error};

/// Cache-side reader generation. Bumping this forces a one-time reindex of
/// every OpenCode SQLite source, because a stored fingerprint whose generation
/// does not match this value is treated as stale.
const OPENCODE_READER_GENERATION: i64 = 3;

/// Outcome of processing one selected session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionOutcome {
    /// Parsed and persisted successfully.
    Indexed,
    /// Read but excluded by policy (`NoUserMessages`); pruned from cache.
    Ineligible,
    /// Read/parse/persist failed (or the source file was unchanged); the
    /// previous cache is retained.
    RetainedFailure,
}

/// Outcome of resolving one configured source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceOutcome {
    /// Present and successfully enumerated. `unchanged` on the opened source
    /// distinguishes "enumerate only, skip parse" from a full re-read.
    Enumerated,
    /// Present-but-unreadable (or a metadata probe failed); owners retained.
    Unavailable,
    /// Confirmed absent; its owners may be pruned.
    Missing,
}

/// How a cached owner's session should be treated for pruning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OwnerDisposition {
    /// The source was enumerated; the id's fate is decided by selection/parse.
    Enumerated,
    /// Keep the cached session unless a higher-generation candidate replaces it.
    Retain,
    /// The source is confirmed gone; prune unless another source re-owns the id.
    Prunable,
}

/// A successfully opened and enumerated SQLite source, kept alive (its read
/// snapshot held) until every selected session has been parsed.
struct EnumeratedSqliteSource {
    path: PathBuf,
    fingerprint: SourceFingerprint,
    v2: Option<V2Reader>,
    v2_ids: Vec<String>,
    v1: Option<SqliteBackend>,
    v1_entries: Vec<SessionEntry>,
    /// True when the source fingerprint was unchanged this run: its IDs are
    /// still enumerated (so reservations and fallbacks hold), but parsing is
    /// skipped unless the cached owner differs.
    unchanged: bool,
    /// Cleared when any selected session in this source fails; a cleared flag
    /// prevents the captured fingerprint from being committed.
    successful: bool,
}

/// The legacy JSON source after enumeration.
struct JsonEnumeration {
    backend: JsonBackend,
    entries: BTreeMap<String, SessionEntry>,
}

// Test seam: paths forced to report `SourcePresence::Unavailable` so
// discovery-directory permission errors can be injected deterministically even
// when tests run as an elevated (root) user.
#[cfg(test)]
thread_local! {
    static FORCED_UNAVAILABLE: std::cell::RefCell<Vec<PathBuf>> = std::cell::RefCell::new(Vec::new());
}

#[cfg(test)]
pub(super) fn force_unavailable(path: &Path) {
    FORCED_UNAVAILABLE.with(|paths| paths.borrow_mut().push(path.to_path_buf()));
}

/// Classify a source path, honoring the test seam.
fn probe_presence(path: &Path) -> SourcePresence {
    #[cfg(test)]
    {
        let forced = FORCED_UNAVAILABLE.with(|paths| paths.borrow().iter().any(|p| p == path));
        if forced {
            return SourcePresence::Unavailable;
        }
    }
    opencode_sources::source_presence(path)
}

fn is_no_user_messages(err: &anyhow::Error) -> bool {
    matches!(
        err.downcast_ref::<OpenCodeParseError>(),
        Some(OpenCodeParseError::NoUserMessages)
    )
}

fn record_source_error(
    path: &Path,
    message: impl Into<String>,
    errors_detail: &mut VecDeque<IndexingError>,
    stats: &mut IndexingStats,
) {
    let message = message.into();
    tracing::warn!(
        "Failed to read OpenCode source {}: {}",
        path.display(),
        message
    );
    push_indexing_error(
        errors_detail,
        AiAssistant::OpenCode,
        Some(path.display().to_string()),
        format!("Failed to read OpenCode source: {message}"),
    );
    stats.errors += 1;
}

fn record_session_error(
    path: &Path,
    id: &str,
    err: &anyhow::Error,
    errors_detail: &mut VecDeque<IndexingError>,
    stats: &mut IndexingStats,
) {
    tracing::warn!(
        "Failed to index OpenCode session {id} from {}: {err:#}",
        path.display()
    );
    push_indexing_error(
        errors_detail,
        AiAssistant::OpenCode,
        Some(path.display().to_string()),
        format!("Failed to index session {id}: {err:#}"),
    );
    stats.errors += 1;
}

/// Open a short-lived read-only connection and probe which generations the
/// source database can serve.
fn probe_source_generations(db_path: &Path) -> Result<Vec<OpenCodeGeneration>> {
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("Failed to open OpenCode DB: {}", db_path.display()))?;
    opencode_sources::probe_generations(&conn)
}

impl SessionIndexer {
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

    pub(super) fn index_opencode_sessions_internal(
        &mut self,
        storage_root: &Path,
        db_paths: &[PathBuf],
        incremental: bool,
        errors_detail: &mut VecDeque<IndexingError>,
    ) -> Result<IndexingStats> {
        let parser = OpenCodeParser::new(storage_root);
        let mut stats = IndexingStats::default();

        // Preload cached owners before any availability check, so an
        // inaccessible or merely undiscovered source still contributes retained
        // candidates instead of silently disappearing.
        let cached_owners = opencode_sources::load_cached_owners(&self.db)?;
        let cached_owner_map: BTreeMap<String, (PathBuf, OpenCodeGeneration)> = cached_owners
            .iter()
            .map(|(id, path, generation)| (id.clone(), (path.clone(), *generation)))
            .collect();

        let json_order = db_paths.len() + 1;

        let mut enumerated: Vec<EnumeratedSqliteSource> = Vec::new();
        let mut enumerated_paths: HashSet<PathBuf> = HashSet::new();
        let mut retained_paths: HashSet<PathBuf> = HashSet::new();
        let mut missing_paths: HashSet<PathBuf> = HashSet::new();
        let mut candidates: Vec<SourceCandidate> = Vec::new();

        for (index, db_path) in db_paths.iter().enumerate() {
            let order = index + 1;
            let (outcome, opened) =
                self.resolve_sqlite_source(db_path, incremental, errors_detail, &mut stats)?;
            match outcome {
                SourceOutcome::Missing => {
                    missing_paths.insert(db_path.clone());
                }
                SourceOutcome::Unavailable => {
                    retained_paths.insert(db_path.clone());
                }
                SourceOutcome::Enumerated => {
                    let source = opened.expect("enumerated source is present");
                    for id in &source.v2_ids {
                        candidates.push(SourceCandidate {
                            session_id: id.clone(),
                            path: db_path.clone(),
                            generation: OpenCodeGeneration::V2,
                            source_order: order,
                            retained: false,
                        });
                    }
                    for entry in &source.v1_entries {
                        candidates.push(SourceCandidate {
                            session_id: entry.id.clone(),
                            path: db_path.clone(),
                            generation: OpenCodeGeneration::V1,
                            source_order: order,
                            retained: false,
                        });
                    }
                    enumerated_paths.insert(db_path.clone());
                    enumerated.push(source);
                }
            }
        }

        // Enumerate the legacy JSON source.
        let json = self.enumerate_json(
            storage_root,
            json_order,
            &mut candidates,
            errors_detail,
            &mut stats,
        );

        // Emit retained candidates for cached owners not covered by discovery.
        for (id, path, generation) in &cached_owners {
            let disposition = owner_disposition(
                id,
                path,
                *generation,
                &enumerated_paths,
                &retained_paths,
                &missing_paths,
                &json,
            );
            if disposition == OwnerDisposition::Retain {
                candidates.push(SourceCandidate {
                    session_id: id.clone(),
                    path: path.clone(),
                    generation: *generation,
                    source_order: 0,
                    retained: true,
                });
            }
        }

        // Select one winner per id, then parse only the readable ones.
        let winners = opencode_sources::select_candidates(candidates);

        let source_index: BTreeMap<PathBuf, usize> = enumerated
            .iter()
            .enumerate()
            .map(|(index, source)| (source.path.clone(), index))
            .collect();

        let mut indexed_ids: HashSet<String> = HashSet::new();
        let mut retained_ids: HashSet<String> = HashSet::new();

        for (id, winner) in &winners {
            if winner.retained {
                retained_ids.insert(id.clone());
                continue;
            }

            // An unchanged source is still enumerated, so its IDs reserve their
            // priority and its readers stay open. Decide per-winner whether
            // parsing can be skipped.
            let is_unchanged = source_index
                .get(&winner.path)
                .map(|&index| enumerated[index].unchanged)
                .unwrap_or(false);
            if is_unchanged {
                match cached_owner_map.get(id) {
                    Some((path, generation))
                        if path == &winner.path && *generation == winner.generation =>
                    {
                        // The cache already holds this exact owner: retain
                        // without parsing.
                        retained_ids.insert(id.clone());
                        stats.skipped += 1;
                        continue;
                    }
                    None => {
                        // No cached owner: an ineligible ID from the previous
                        // run reserves priority. Skip the parse and let it be
                        // pruned rather than reintroducing a lower-priority
                        // representation.
                        continue;
                    }
                    Some(_) => {
                        // Cached owner differs: this is the lower-priority
                        // fallback / promotion path. The source's readers are
                        // open because it was enumerated, so parse normally.
                    }
                }
            }

            let outcome = self.parse_selected(
                &parser,
                winner,
                &source_index,
                &mut enumerated,
                &json,
                incremental,
                errors_detail,
                &mut stats,
                &cached_owner_map,
            )?;
            match outcome {
                SessionOutcome::Indexed => {
                    indexed_ids.insert(id.clone());
                }
                SessionOutcome::Ineligible => {
                    // Reserved priority but excluded by policy; pruned below.
                }
                SessionOutcome::RetainedFailure => {
                    retained_ids.insert(id.clone());
                }
            }
        }

        // Prune only ids whose owners were successfully enumerated or confirmed
        // missing: everything not freshly indexed and not retained.
        let kept: HashSet<String> = indexed_ids.union(&retained_ids).cloned().collect();

        // Prune and drop orphan fingerprints before committing any fingerprint
        // (Task 5 Step 4): a cleanup failure must prevent this run's fingerprint
        // commit, but must not abort the other assistants' indexing.
        let cleanup = (|| -> Result<()> {
            stats.removed += self.prune_stale_opencode_sessions(&kept)?;
            self.prune_orphan_fingerprints()?;
            Ok(())
        })();

        match cleanup {
            Ok(()) => {
                // Commit captured fingerprints only for fully successful sources
                // and only after cleanup succeeded.
                for source in &enumerated {
                    if source.successful
                        && let Err(err) =
                            self.commit_opencode_fingerprint(&source.path, &source.fingerprint)
                    {
                        tracing::warn!(
                            "Failed to commit OpenCode fingerprint for {}: {err:#}",
                            source.path.display()
                        );
                        push_indexing_error(
                            errors_detail,
                            AiAssistant::OpenCode,
                            Some(source.path.display().to_string()),
                            format!("Failed to commit OpenCode source fingerprint: {err:#}"),
                        );
                        stats.errors += 1;
                    }
                }
            }
            Err(err) => {
                tracing::warn!("Failed to clean up OpenCode cache: {err:#}");
                push_indexing_error(
                    errors_detail,
                    AiAssistant::OpenCode,
                    None,
                    format!("Failed to clean up OpenCode cache: {err:#}"),
                );
                stats.errors += 1;
            }
        }

        Ok(stats)
    }

    /// Resolve one configured SQLite source into a [`SourceOutcome`] and, when
    /// it is enumerable, the opened source with its snapshot held.
    fn resolve_sqlite_source(
        &self,
        db_path: &Path,
        incremental: bool,
        errors_detail: &mut VecDeque<IndexingError>,
        stats: &mut IndexingStats,
    ) -> Result<(SourceOutcome, Option<EnumeratedSqliteSource>)> {
        match probe_presence(db_path) {
            SourcePresence::Missing => Ok((SourceOutcome::Missing, None)),
            SourcePresence::Unavailable => {
                record_source_error(
                    db_path,
                    "source metadata is unavailable",
                    errors_detail,
                    stats,
                );
                Ok((SourceOutcome::Unavailable, None))
            }
            SourcePresence::Present => {
                // Capture the fingerprint before opening the read snapshot, so
                // the committed state is the pre-read state.
                let fingerprint = match opencode_sources::capture_source_fingerprint(db_path) {
                    Ok(fingerprint) => fingerprint,
                    Err(err) => {
                        record_source_error(db_path, err.to_string(), errors_detail, stats);
                        return Ok((SourceOutcome::Unavailable, None));
                    }
                };

                // An unchanged source still enumerates its IDs (so reservations
                // and lower-priority fallbacks hold on later runs); only parsing
                // is skipped.
                let unchanged =
                    incremental && self.opencode_source_unchanged(db_path, &fingerprint)?;

                let generations = match probe_source_generations(db_path) {
                    Ok(generations) => generations,
                    Err(err) => {
                        record_source_error(db_path, err.to_string(), errors_detail, stats);
                        return Ok((SourceOutcome::Unavailable, None));
                    }
                };

                match self.open_sqlite_source(
                    db_path,
                    fingerprint,
                    &generations,
                    errors_detail,
                    stats,
                )? {
                    Some(mut source) => {
                        source.unchanged = unchanged;
                        Ok((SourceOutcome::Enumerated, Some(source)))
                    }
                    None => Ok((SourceOutcome::Unavailable, None)),
                }
            }
        }
    }

    /// Open and enumerate one present SQLite source, returning `None` (after
    /// recording a diagnostic) if the reader cannot be opened or enumerated.
    fn open_sqlite_source(
        &self,
        db_path: &Path,
        fingerprint: SourceFingerprint,
        generations: &[OpenCodeGeneration],
        errors_detail: &mut VecDeque<IndexingError>,
        stats: &mut IndexingStats,
    ) -> Result<Option<EnumeratedSqliteSource>> {
        let mut source = EnumeratedSqliteSource {
            path: db_path.to_path_buf(),
            fingerprint,
            v2: None,
            v2_ids: Vec::new(),
            v1: None,
            v1_entries: Vec::new(),
            unchanged: false,
            successful: true,
        };

        if generations.contains(&OpenCodeGeneration::V2) {
            match V2Reader::open(db_path).and_then(|reader| {
                let ids = reader.list_session_ids()?;
                Ok((reader, ids))
            }) {
                Ok((reader, ids)) => {
                    source.v2_ids = ids;
                    source.v2 = Some(reader);
                }
                Err(err) => {
                    record_source_error(db_path, err.to_string(), errors_detail, stats);
                    return Ok(None);
                }
            }
        }

        if generations.contains(&OpenCodeGeneration::V1) {
            match SqliteBackend::open(db_path).and_then(|backend| {
                backend.begin_read_snapshot()?;
                let entries = backend.list_sessions()?;
                Ok((backend, entries))
            }) {
                Ok((backend, entries)) => {
                    source.v1_entries = entries;
                    source.v1 = Some(backend);
                }
                Err(err) => {
                    record_source_error(db_path, err.to_string(), errors_detail, stats);
                    return Ok(None);
                }
            }
        }

        Ok(Some(source))
    }

    /// Enumerate the legacy JSON source. Never treats an absent or unreadable
    /// storage root as an error that could prune cached JSON sessions; the
    /// per-file presence check in [`owner_disposition`] decides retention.
    fn enumerate_json(
        &self,
        storage_root: &Path,
        order: usize,
        candidates: &mut Vec<SourceCandidate>,
        errors_detail: &mut VecDeque<IndexingError>,
        stats: &mut IndexingStats,
    ) -> JsonEnumeration {
        let backend = JsonBackend::new(storage_root);
        match probe_presence(storage_root) {
            SourcePresence::Missing => JsonEnumeration {
                backend,
                entries: BTreeMap::new(),
            },
            SourcePresence::Unavailable => {
                record_source_error(
                    storage_root,
                    "storage directory is unavailable",
                    errors_detail,
                    stats,
                );
                JsonEnumeration {
                    backend,
                    entries: BTreeMap::new(),
                }
            }
            SourcePresence::Present => match backend.list_sessions() {
                Ok(entries) => {
                    let mut by_id = BTreeMap::new();
                    for entry in entries {
                        candidates.push(SourceCandidate {
                            session_id: entry.id.clone(),
                            path: json_file_path(&entry),
                            generation: OpenCodeGeneration::Json,
                            source_order: order,
                            retained: false,
                        });
                        by_id.insert(entry.id.clone(), entry);
                    }
                    JsonEnumeration {
                        backend,
                        entries: by_id,
                    }
                }
                Err(err) => {
                    record_source_error(storage_root, err.to_string(), errors_detail, stats);
                    JsonEnumeration {
                        backend,
                        entries: BTreeMap::new(),
                    }
                }
            },
        }
    }

    /// True when the stored fingerprint matches the captured DB/WAL state and
    /// the reader generation is current, so the source can be skipped.
    fn opencode_source_unchanged(
        &self,
        db_path: &Path,
        captured: &SourceFingerprint,
    ) -> Result<bool> {
        let Some(stored) = opencode_sources::load_source_fingerprint(&self.db, db_path)? else {
            return Ok(false);
        };
        Ok(stored.reader_generation == OPENCODE_READER_GENERATION
            && stored.fingerprint == *captured)
    }

    /// Parse one selected (non-retained) winner and persist it.
    #[allow(clippy::too_many_arguments)]
    fn parse_selected(
        &mut self,
        parser: &OpenCodeParser,
        winner: &SourceCandidate,
        source_index: &BTreeMap<PathBuf, usize>,
        enumerated: &mut [EnumeratedSqliteSource],
        json: &JsonEnumeration,
        incremental: bool,
        errors_detail: &mut VecDeque<IndexingError>,
        stats: &mut IndexingStats,
        cached_owner_map: &BTreeMap<String, (PathBuf, OpenCodeGeneration)>,
    ) -> Result<SessionOutcome> {
        match winner.generation {
            OpenCodeGeneration::V2 => {
                let index = *source_index
                    .get(&winner.path)
                    .context("V2 source not enumerated")?;
                let parsed = {
                    let reader = enumerated[index]
                        .v2
                        .as_ref()
                        .context("V2 reader missing for enumerated source")?;
                    reader.parse_session(&winner.session_id)
                };
                match parsed {
                    Ok(parsed) => match self.insert_opencode_session(&parsed, winner) {
                        Ok(()) => {
                            stats.indexed += 1;
                            Ok(SessionOutcome::Indexed)
                        }
                        Err(err) => {
                            enumerated[index].successful = false;
                            record_session_error(
                                &winner.path,
                                &winner.session_id,
                                &err,
                                errors_detail,
                                stats,
                            );
                            Ok(SessionOutcome::RetainedFailure)
                        }
                    },
                    Err(err) if is_no_user_messages(&err) => Ok(SessionOutcome::Ineligible),
                    Err(err) => {
                        enumerated[index].successful = false;
                        record_session_error(
                            &winner.path,
                            &winner.session_id,
                            &err,
                            errors_detail,
                            stats,
                        );
                        Ok(SessionOutcome::RetainedFailure)
                    }
                }
            }
            OpenCodeGeneration::V1 => {
                let index = *source_index
                    .get(&winner.path)
                    .context("V1 source not enumerated")?;
                let parsed = {
                    let backend = enumerated[index]
                        .v1
                        .as_ref()
                        .context("V1 backend missing for enumerated source")?;
                    let entry = enumerated[index]
                        .v1_entries
                        .iter()
                        .find(|entry| entry.id == winner.session_id)
                        .context("V1 entry not enumerated")?;
                    parser.parse_entry(entry, backend)
                };
                match parsed {
                    Ok(parsed) => match self.insert_opencode_session(&parsed, winner) {
                        Ok(()) => {
                            stats.indexed += 1;
                            Ok(SessionOutcome::Indexed)
                        }
                        Err(err) => {
                            enumerated[index].successful = false;
                            record_session_error(
                                &winner.path,
                                &winner.session_id,
                                &err,
                                errors_detail,
                                stats,
                            );
                            Ok(SessionOutcome::RetainedFailure)
                        }
                    },
                    Err(err) if is_no_user_messages(&err) => Ok(SessionOutcome::Ineligible),
                    Err(err) => {
                        enumerated[index].successful = false;
                        record_session_error(
                            &winner.path,
                            &winner.session_id,
                            &err,
                            errors_detail,
                            stats,
                        );
                        Ok(SessionOutcome::RetainedFailure)
                    }
                }
            }
            OpenCodeGeneration::Json => {
                let entry = json
                    .entries
                    .get(&winner.session_id)
                    .context("JSON entry not enumerated")?;
                let path = json_file_path(entry);
                // Skip only when the cache already holds this exact JSON
                // representation and its file is unchanged. If the cached owner
                // is a V1/V2 row elsewhere (or a different JSON path), the
                // JSON candidate must be parsed so a confirmed-missing SQLite
                // owner can fall back to it.
                let cached_is_same_json = cached_owner_map.get(&winner.session_id).is_some_and(
                    |(cached_path, generation)| {
                        *generation == OpenCodeGeneration::Json && cached_path == &path
                    },
                );
                if incremental && cached_is_same_json && !self.should_reindex(&path)? {
                    stats.skipped += 1;
                    return Ok(SessionOutcome::RetainedFailure);
                }
                let parsed = parser.parse_entry(entry, &json.backend);
                match parsed {
                    Ok(parsed) => match self.insert_opencode_json_session(&parsed, &path) {
                        Ok(()) => {
                            stats.indexed += 1;
                            Ok(SessionOutcome::Indexed)
                        }
                        Err(err) => {
                            record_session_error(
                                &path,
                                &winner.session_id,
                                &err,
                                errors_detail,
                                stats,
                            );
                            Ok(SessionOutcome::RetainedFailure)
                        }
                    },
                    Err(err) if is_no_user_messages(&err) => Ok(SessionOutcome::Ineligible),
                    Err(err) => {
                        record_session_error(&path, &winner.session_id, &err, errors_detail, stats);
                        Ok(SessionOutcome::RetainedFailure)
                    }
                }
            }
        }
    }

    /// Atomically persist a parsed session (session row, contents, and its
    /// generation) without writing a source fingerprint.
    fn insert_opencode_session(
        &mut self,
        parsed: &ParsedSession,
        owner: &SourceCandidate,
    ) -> Result<()> {
        let tx = self.db.transaction()?;
        let resolved_project_id =
            Self::upsert_project_tx(&tx, parsed.session.project_path.as_deref())?;
        Self::upsert_session_row_tx(&tx, parsed, &owner.path, resolved_project_id)?;
        Self::replace_session_contents_tx(&tx, parsed)?;
        tx.execute(
            "INSERT INTO opencode_session_sources (session_id, generation) VALUES (?1, ?2)
             ON CONFLICT(session_id) DO UPDATE SET generation = excluded.generation",
            rusqlite::params![&parsed.session.id, owner.generation.to_storage()],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Atomically persist a legacy JSON session with its generation and the
    /// per-file `file_fingerprints` row used for incremental skip detection.
    fn insert_opencode_json_session(
        &mut self,
        parsed: &ParsedSession,
        file_path: &Path,
    ) -> Result<()> {
        let tx = self.db.transaction()?;
        let resolved_project_id =
            Self::upsert_project_tx(&tx, parsed.session.project_path.as_deref())?;
        Self::upsert_session_row_tx(&tx, parsed, file_path, resolved_project_id)?;
        Self::replace_session_contents_tx(&tx, parsed)?;
        tx.execute(
            "INSERT INTO opencode_session_sources (session_id, generation) VALUES (?1, 'json')
             ON CONFLICT(session_id) DO UPDATE SET generation = excluded.generation",
            [&parsed.session.id],
        )?;
        Self::upsert_fingerprint_tx(&tx, file_path)?;
        tx.commit()?;
        Ok(())
    }

    /// Commit a captured source fingerprint with the current reader generation.
    fn commit_opencode_fingerprint(
        &mut self,
        path: &Path,
        captured: &SourceFingerprint,
    ) -> Result<()> {
        let Some(path_str) = path.to_str() else {
            return Ok(());
        };
        let tx = self.db.transaction()?;
        tx.execute(
            "INSERT INTO opencode_source_fingerprints
                (db_path, db_mtime_ns, db_size, wal_mtime_ns, wal_size, reader_generation)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(db_path) DO UPDATE SET
                db_mtime_ns = excluded.db_mtime_ns,
                db_size = excluded.db_size,
                wal_mtime_ns = excluded.wal_mtime_ns,
                wal_size = excluded.wal_size,
                reader_generation = excluded.reader_generation",
            rusqlite::params![
                path_str,
                captured.db.mtime_ns,
                captured.db.size,
                captured.wal.map(|wal| wal.mtime_ns),
                captured.wal.map(|wal| wal.size),
                OPENCODE_READER_GENERATION,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }
}

/// Test-only convenience wrapper that discards `errors_detail`.
#[cfg(test)]
impl SessionIndexer {
    pub fn index_opencode_sessions_incremental(
        &mut self,
        storage_root: &Path,
        db_paths: &[PathBuf],
    ) -> Result<IndexingStats> {
        let mut errors_detail = VecDeque::new();
        self.index_opencode_sessions_internal(storage_root, db_paths, true, &mut errors_detail)
    }
}

fn json_file_path(entry: &SessionEntry) -> PathBuf {
    match &entry.source {
        SessionSource::JsonFile(path) => path.clone(),
        SessionSource::SqliteRow { db_path, .. } => db_path.clone(),
    }
}

fn owner_disposition(
    id: &str,
    path: &Path,
    generation: OpenCodeGeneration,
    enumerated_paths: &HashSet<PathBuf>,
    retained_paths: &HashSet<PathBuf>,
    missing_paths: &HashSet<PathBuf>,
    json: &JsonEnumeration,
) -> OwnerDisposition {
    match generation {
        OpenCodeGeneration::Json => {
            if json.entries.contains_key(id) {
                OwnerDisposition::Enumerated
            } else {
                match probe_presence(path) {
                    SourcePresence::Missing => OwnerDisposition::Prunable,
                    _ => OwnerDisposition::Retain,
                }
            }
        }
        OpenCodeGeneration::V1 | OpenCodeGeneration::V2 => {
            if enumerated_paths.contains(path) {
                OwnerDisposition::Enumerated
            } else if retained_paths.contains(path) {
                OwnerDisposition::Retain
            } else if missing_paths.contains(path) {
                OwnerDisposition::Prunable
            } else {
                // Not discovered this run: absence is not proof of deletion.
                match probe_presence(path) {
                    SourcePresence::Missing => OwnerDisposition::Prunable,
                    _ => OwnerDisposition::Retain,
                }
            }
        }
    }
}
