//! OpenCode source-generation arbitration and cache-ownership metadata.
//!
//! This module is the *pure* source-selection layer: it never opens a source
//! database, parses a session, or mutates the local cache. The coordinator
//! (Task 5, `database/indexer/opencode.rs`) builds [`SourceCandidate`]s from
//! discovery and cached ownership, calls [`select_candidates`] once before any
//! parsing, and treats the returned map as the single owner per session ID.

// Consumed by the OpenCode coordinator (Task 5) but not referenced by
// production code in this commit.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::Connection;

/// Which OpenCode storage generation produced a session's indexed
/// representation.
///
/// Priority is exactly the variant order: `V2` > `V1` > `Json`.
/// [`OpenCodeGeneration::priority`] encodes it explicitly (higher value =
/// higher priority). Storage strings match `opencode_session_sources.generation`
/// (Task 1's v19 migration).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum OpenCodeGeneration {
    V2,
    V1,
    Json,
}

impl OpenCodeGeneration {
    pub(crate) fn to_storage(self) -> &'static str {
        match self {
            Self::V2 => "v2",
            Self::V1 => "v1",
            Self::Json => "json",
        }
    }

    pub(crate) fn from_storage(value: &str) -> Option<Self> {
        match value {
            "v2" => Some(Self::V2),
            "v1" => Some(Self::V1),
            "json" => Some(Self::Json),
            _ => None,
        }
    }

    /// Selection priority, highest first: `V2` = 2, `V1` = 1, `Json` = 0.
    pub(crate) fn priority(self) -> u8 {
        match self {
            Self::V2 => 2,
            Self::V1 => 1,
            Self::Json => 0,
        }
    }
}

/// One discovered or retained representation of an OpenCode session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceCandidate {
    pub session_id: String,
    pub path: PathBuf,
    pub generation: OpenCodeGeneration,
    /// Rank of the configured source that discovered this candidate, starting
    /// at 1 (`Sources.opencode_db_paths` order). `0` is reserved for a retained
    /// owner whose original configured position cannot be established.
    pub source_order: usize,
    /// True when this candidate is a cached owner preserved for availability
    /// (an unreadable or undiscovered source) rather than freshly enumerated.
    pub retained: bool,
}

/// Select the single winning representation per session ID.
///
/// Generation priority wins first (`V2` > `V1` > `Json`), then configured
/// source order (lower rank first; retained rank `0` beats discovered
/// equal-generation candidates), and finally a deterministic path tie-break.
/// Traversal order and modification time never influence the result.
pub(crate) fn select_candidates(
    candidates: Vec<SourceCandidate>,
) -> BTreeMap<String, SourceCandidate> {
    let mut winners: BTreeMap<String, SourceCandidate> = BTreeMap::new();
    for candidate in candidates {
        match winners.get(&candidate.session_id) {
            Some(current) => {
                if beats(&candidate, current) {
                    winners.insert(candidate.session_id.clone(), candidate);
                }
            }
            None => {
                winners.insert(candidate.session_id.clone(), candidate);
            }
        }
    }
    winners
}

/// True when `candidate` should replace `current` as the winner for an ID:
/// higher generation first, then lower configured rank, then a deterministic
/// path tie-break.
fn beats(candidate: &SourceCandidate, current: &SourceCandidate) -> bool {
    use std::cmp::Ordering;
    match candidate
        .generation
        .priority()
        .cmp(&current.generation.priority())
    {
        Ordering::Greater => true,
        Ordering::Less => false,
        Ordering::Equal => match candidate.source_order.cmp(&current.source_order) {
            Ordering::Less => true,
            Ordering::Greater => false,
            Ordering::Equal => candidate.path < current.path,
        },
    }
}

/// Load cached OpenCode owners from the local cache: session id, owning path,
/// and the generation recorded by the v19 migration / indexing. Limited to
/// OpenCode sessions only.
pub(crate) fn load_cached_owners(
    db: &Connection,
) -> Result<Vec<(String, PathBuf, OpenCodeGeneration)>> {
    let mut stmt = db
        .prepare(
            "SELECT s.id, s.file_path, oss.generation
             FROM sessions s
             JOIN opencode_session_sources oss ON oss.session_id = s.id
             WHERE s.tool = 'opencode'
             ORDER BY s.id",
        )
        .context("Failed to prepare cached OpenCode owner query")?;

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;

    let mut owners = Vec::new();
    for row in rows {
        let (id, file_path, generation_str) = row?;
        match OpenCodeGeneration::from_storage(&generation_str) {
            Some(generation) => owners.push((id, PathBuf::from(file_path), generation)),
            None => tracing::warn!(
                "Unknown OpenCode generation '{}' for cached session {}",
                generation_str,
                id
            ),
        }
    }
    Ok(owners)
}

/// Whether a cached owner's source is still present.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourcePresence {
    Present,
    Missing,
    Unavailable,
}

/// Classify a source path. Only `NotFound` confirms deletion; every other
/// metadata error (permission denied, I/O) retains ownership.
pub(crate) fn source_presence(path: &Path) -> SourcePresence {
    match std::fs::metadata(path) {
        Ok(_) => SourcePresence::Present,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => SourcePresence::Missing,
        Err(_) => SourcePresence::Unavailable,
    }
}

/// Determine which storage generations a source database can serve, highest
/// priority first. A present `session_v2` table whose `session_message` table
/// is missing is a failed V2 source (an error), never V1 absence.
pub(crate) fn probe_generations(conn: &Connection) -> Result<Vec<OpenCodeGeneration>> {
    if table_exists(conn, "session_v2")? {
        if !table_exists(conn, "session_message")? {
            anyhow::bail!(
                "OpenCode database has session_v2 but is missing session_message (partial V2 source)"
            );
        }

        let mut generations = vec![OpenCodeGeneration::V2];
        // A mixed database can serve both generations; the coordinator
        // enumerates V2 before V1 and ignores V1 rows for V2-present IDs.
        if table_exists(conn, "session")? && table_exists(conn, "message")? {
            generations.push(OpenCodeGeneration::V1);
        }
        return Ok(generations);
    }

    if table_exists(conn, "session")? && table_exists(conn, "message")? {
        return Ok(vec![OpenCodeGeneration::V1]);
    }

    Ok(Vec::new())
}

fn table_exists(conn: &Connection, name: &str) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [name],
            |row| row.get::<_, i64>(0),
        )
        .context("Failed to inspect source table names")?
        > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn cand(
        id: &str,
        path: &str,
        generation: OpenCodeGeneration,
        source_order: usize,
        retained: bool,
    ) -> SourceCandidate {
        SourceCandidate {
            session_id: id.to_string(),
            path: PathBuf::from(path),
            generation,
            source_order,
            retained,
        }
    }

    fn permutations<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
        if items.len() <= 1 {
            return vec![items.to_vec()];
        }
        let mut result = Vec::new();
        for i in 0..items.len() {
            let mut rest = items.to_vec();
            let picked = rest.remove(i);
            for mut perm in permutations(&rest) {
                perm.insert(0, picked.clone());
                result.push(perm);
            }
        }
        result
    }

    #[test]
    fn opencode_candidates_ignore_traversal_order() {
        let v2_path = PathBuf::from("/data/opencode-v2.db");
        let base = vec![
            cand(
                "ses_shared",
                "/data/opencode-v2.db",
                OpenCodeGeneration::V2,
                1,
                false,
            ),
            cand(
                "ses_shared",
                "/data/opencode.db",
                OpenCodeGeneration::V1,
                2,
                false,
            ),
            cand(
                "ses_shared",
                "/json/ses_shared.json",
                OpenCodeGeneration::Json,
                3,
                false,
            ),
            cand(
                "ses_other",
                "/data/opencode.db",
                OpenCodeGeneration::V1,
                2,
                false,
            ),
        ];

        let mut results = Vec::new();
        for perm in permutations(&base) {
            let chosen = select_candidates(perm);
            assert_eq!(chosen["ses_shared"].generation, OpenCodeGeneration::V2);
            assert_eq!(chosen["ses_shared"].path, v2_path);
            assert_eq!(chosen["ses_other"].generation, OpenCodeGeneration::V1);
            results.push(chosen);
        }

        // Every permutation of the same configured-source order must yield the
        // identical owner map.
        for chosen in &results[1..] {
            assert_eq!(*chosen, results[0]);
        }
    }

    #[test]
    fn opencode_retained_owner_can_be_promoted() {
        // A retained V1 owner must not block promotion to a higher generation.
        let chosen = select_candidates(vec![
            cand(
                "ses_shared",
                "/data/opencode.db",
                OpenCodeGeneration::V1,
                0,
                true,
            ),
            cand(
                "ses_shared",
                "/data/opencode-v2.db",
                OpenCodeGeneration::V2,
                1,
                false,
            ),
        ]);
        assert_eq!(chosen["ses_shared"].generation, OpenCodeGeneration::V2);
        assert_eq!(
            chosen["ses_shared"].path,
            PathBuf::from("/data/opencode-v2.db")
        );

        // Retention still protects against a lower-generation source.
        let retained_wins = select_candidates(vec![
            cand(
                "ses_shared",
                "/data/opencode.db",
                OpenCodeGeneration::V1,
                0,
                true,
            ),
            cand(
                "ses_shared",
                "/json/ses_shared.json",
                OpenCodeGeneration::Json,
                3,
                false,
            ),
        ]);
        assert_eq!(
            retained_wins["ses_shared"].generation,
            OpenCodeGeneration::V1
        );
        assert!(retained_wins["ses_shared"].retained);
    }

    #[test]
    fn opencode_same_generation_uses_source_order() {
        // Discovered same-generation candidates: lower configured rank wins.
        let chosen = select_candidates(vec![
            cand(
                "ses_shared",
                "/data/opencode-b.db",
                OpenCodeGeneration::V1,
                2,
                false,
            ),
            cand(
                "ses_shared",
                "/data/opencode-a.db",
                OpenCodeGeneration::V1,
                1,
                false,
            ),
        ]);
        assert_eq!(
            chosen["ses_shared"].path,
            PathBuf::from("/data/opencode-a.db")
        );
        assert_eq!(chosen["ses_shared"].source_order, 1);

        // A retained equal-generation owner (rank 0) outranks discovered
        // candidates of the same generation.
        let equal_generation = select_candidates(vec![
            cand(
                "ses_shared",
                "/data/opencode-b.db",
                OpenCodeGeneration::V1,
                1,
                false,
            ),
            cand(
                "ses_shared",
                "/data/opencode-retained.db",
                OpenCodeGeneration::V1,
                0,
                true,
            ),
        ]);
        assert_eq!(equal_generation["ses_shared"].source_order, 0);
        assert!(equal_generation["ses_shared"].retained);
        assert_eq!(
            equal_generation["ses_shared"].path,
            PathBuf::from("/data/opencode-retained.db")
        );
    }

    #[test]
    fn opencode_mixed_v2_suppresses_same_id_v1() {
        // A mixed database exposes both V2 and V1 rows for the same ID.
        // Selection reserves the V2 representation and suppresses the V1 row,
        // even though the V2 session may later prove ineligible during parsing
        // (that reservation is the coordinator's job, not selection's).
        let chosen = select_candidates(vec![
            cand(
                "ses_shared",
                "/data/mixed.db",
                OpenCodeGeneration::V2,
                1,
                false,
            ),
            cand(
                "ses_shared",
                "/data/mixed.db",
                OpenCodeGeneration::V1,
                1,
                false,
            ),
        ]);
        assert_eq!(chosen.len(), 1);
        assert_eq!(chosen["ses_shared"].generation, OpenCodeGeneration::V2);
        assert_eq!(chosen["ses_shared"].path, PathBuf::from("/data/mixed.db"));
    }

    #[test]
    fn opencode_probe_partial_v2_is_failure() {
        // session_v2 present but session_message missing: failed V2, not V1
        // absence permitting a V1 fallback.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE session_v2 (id TEXT PRIMARY KEY);")
            .unwrap();
        assert!(probe_generations(&conn).is_err());

        // Complete V2 -> [V2].
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE session_v2 (id TEXT PRIMARY KEY);
             CREATE TABLE session_message (id TEXT PRIMARY KEY);",
        )
        .unwrap();
        assert_eq!(
            probe_generations(&conn).unwrap(),
            vec![OpenCodeGeneration::V2]
        );

        // V1-only -> [V1].
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE session (id TEXT PRIMARY KEY);
             CREATE TABLE message (id TEXT PRIMARY KEY);",
        )
        .unwrap();
        assert_eq!(
            probe_generations(&conn).unwrap(),
            vec![OpenCodeGeneration::V1]
        );

        // Mixed DB -> [V2, V1], V2 first.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE session_v2 (id TEXT PRIMARY KEY);
             CREATE TABLE session_message (id TEXT PRIMARY KEY);
             CREATE TABLE session (id TEXT PRIMARY KEY);
             CREATE TABLE message (id TEXT PRIMARY KEY);",
        )
        .unwrap();
        assert_eq!(
            probe_generations(&conn).unwrap(),
            vec![OpenCodeGeneration::V2, OpenCodeGeneration::V1]
        );

        // No recognized tables -> [].
        let conn = Connection::open_in_memory().unwrap();
        assert!(probe_generations(&conn).unwrap().is_empty());
    }

    #[test]
    fn source_presence_confirms_only_not_found_as_missing() {
        let dir = tempfile::tempdir().unwrap();

        let present = dir.path().join("present.db");
        std::fs::write(&present, b"").unwrap();
        assert_eq!(source_presence(&present), SourcePresence::Present);

        let missing = dir.path().join("missing.db");
        assert_eq!(source_presence(&missing), SourcePresence::Missing);

        // A metadata error that is not NotFound retains ownership: a path whose
        // parent is a regular file yields NotADirectory, not NotFound.
        let file = dir.path().join("afile");
        std::fs::write(&file, b"").unwrap();
        let child = file.join("sub.db");
        assert_eq!(source_presence(&child), SourcePresence::Unavailable);
    }

    #[test]
    fn load_cached_owners_reads_generation_limited_to_opencode() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (
                id TEXT PRIMARY KEY, tool TEXT NOT NULL, file_path TEXT NOT NULL
             );
             CREATE TABLE opencode_session_sources (
                session_id TEXT PRIMARY KEY, generation TEXT NOT NULL
             );
             INSERT INTO sessions (id, tool, file_path) VALUES
                ('oc_v2', 'opencode', '/data/opencode-v2.db'),
                ('oc_v1', 'opencode', '/data/opencode.db'),
                ('oc_json', 'opencode', '/json/oc.json'),
                ('claude', 'claude_code', '/data/claude.jsonl'),
                ('oc_unknown', 'opencode', '/data/opencode-future.db');
             INSERT INTO opencode_session_sources (session_id, generation) VALUES
                ('oc_v2', 'v2'),
                ('oc_v1', 'v1'),
                ('oc_json', 'json'),
                ('claude', 'v1'),
                ('oc_unknown', 'v3');",
        )
        .unwrap();

        let owners = load_cached_owners(&conn).unwrap();
        // The claude_code session is excluded by the tool filter, and the
        // unknown-generation OpenCode owner is skipped defensively.
        assert_eq!(owners.len(), 3);

        let map: BTreeMap<&str, (&str, OpenCodeGeneration)> = owners
            .iter()
            .map(|(id, path, generation)| (id.as_str(), (path.to_str().unwrap(), *generation)))
            .collect();
        assert_eq!(
            map["oc_v2"],
            ("/data/opencode-v2.db", OpenCodeGeneration::V2)
        );
        assert_eq!(map["oc_v1"], ("/data/opencode.db", OpenCodeGeneration::V1));
        assert_eq!(map["oc_json"], ("/json/oc.json", OpenCodeGeneration::Json));
    }

    #[test]
    fn generation_storage_strings_and_priority_are_stable() {
        assert_eq!(OpenCodeGeneration::V2.to_storage(), "v2");
        assert_eq!(OpenCodeGeneration::V1.to_storage(), "v1");
        assert_eq!(OpenCodeGeneration::Json.to_storage(), "json");
        assert_eq!(
            OpenCodeGeneration::from_storage("v2"),
            Some(OpenCodeGeneration::V2)
        );
        assert_eq!(
            OpenCodeGeneration::from_storage("v1"),
            Some(OpenCodeGeneration::V1)
        );
        assert_eq!(
            OpenCodeGeneration::from_storage("json"),
            Some(OpenCodeGeneration::Json)
        );
        assert_eq!(OpenCodeGeneration::from_storage("bogus"), None);

        assert!(OpenCodeGeneration::V2.priority() > OpenCodeGeneration::V1.priority());
        assert!(OpenCodeGeneration::V1.priority() > OpenCodeGeneration::Json.priority());
    }
}
