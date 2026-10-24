//! End-to-end coverage for Mistral Vibe Unified sessions.
//!
//! Every test indexes a temporary copy of `tests/fixtures` through
//! `SessionSources::resolve(Some(root))`, so the whole production
//! discovery path (legacy and Unified Vibe side by side) is exercised
//! without touching the checked-in fixtures.

use std::fs;
use std::path::{Path, PathBuf};

use sessions_chronicle::database::SessionIndexer;
use sessions_chronicle::database::analytics::load_analytics;
use sessions_chronicle::database::{load_session, search_sessions_for_filter, toggle_pin};
use sessions_chronicle::models::{
    AiAssistant, DateFilter, IndexingRunResult, PerSourceResult, ProjectFilter,
};
use sessions_chronicle::parsers::mistral_vibe_unified::is_unified_session_dir;
use sessions_chronicle::session_sources::SessionSources;
use tempfile::{NamedTempFile, TempDir};

/// R302: the richest Unified root fixture (chunks, journal, saved results).
const R302: &str = "aaaaaaaa-0000-4000-8000-000000000302";
/// CHILD_A: R302's explicitly spawned `scout` subagent.
const CHILD_A: &str = "child-ea9b00b3ad4061f4ca36fd81";

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

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

/// A temporary copy of the whole fixture tree, kept alive for the test.
struct FixtureCopy {
    _root: TempDir,
    sources: SessionSources,
}

impl FixtureCopy {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        copy_dir(&fixtures_root(), root.path());
        let sources = SessionSources::resolve(Some(root.path()));
        Self {
            _root: root,
            sources,
        }
    }
}

/// A fixture copy plus a database that has been indexed incrementally once.
struct IndexedFixtures {
    _fixtures: FixtureCopy,
    database: NamedTempFile,
}

impl IndexedFixtures {
    fn new() -> Self {
        let fixtures = FixtureCopy::new();
        let database = NamedTempFile::new().unwrap();
        let mut indexer = SessionIndexer::new(database.path()).unwrap();
        indexer
            .index_all_incremental(&fixtures.sources)
            .expect("fixture tree must index");
        Self {
            _fixtures: fixtures,
            database,
        }
    }

    fn db_path(&self) -> &Path {
        self.database.path()
    }
}

fn vibe_source(result: &IndexingRunResult) -> &PerSourceResult {
    result
        .per_source
        .iter()
        .find(|source| source.assistant == AiAssistant::MistralVibe)
        .expect("Mistral Vibe per-source result")
}

fn search(fixtures: &IndexedFixtures, query: &str, project_filter: &ProjectFilter) -> Vec<String> {
    search_sessions_for_filter(
        fixtures.db_path(),
        &[AiAssistant::MistralVibe],
        project_filter,
        query,
        &DateFilter::AnyTime,
        None,
    )
    .expect("search must succeed")
    .into_iter()
    .map(|session| session.id)
    .collect()
}

#[test]
fn unified_fixture_is_searchable() {
    let fixtures = IndexedFixtures::new();
    let all = ProjectFilter::AllSessions;

    // Non-ASCII text from pooled chunks (French accents, emoji) and the
    // accented assistant message must surface through FTS.
    for query in ["Résume", "corrigé"] {
        let hits = search(&fixtures, query, &all);
        assert!(
            hits.iter().any(|id| id == R302),
            "searching {query:?} must return R302, got {hits:?}"
        );
    }

    // "Doc ajoutée." only exists as a journal replay replacing a provisional
    // assistant message; the replayed text must be searchable.
    let hits = search(&fixtures, "Doc ajoutée", &all);
    assert!(
        hits.iter().any(|id| id == R302),
        "searching journal-only text must return R302, got {hits:?}"
    );

    // Discarded assistant output and removed/provisional reasoning must
    // never leak into the search index.
    for query in ["WRONG ANSWER", "partial thinking"] {
        let hits = search(&fixtures, query, &all);
        assert!(
            hits.is_empty(),
            "searching {query:?} must not match discarded or provisional content, got {hits:?}"
        );
    }
}

#[test]
fn analytics_and_filters_include_unified() {
    let fixtures = IndexedFixtures::new();

    // All seven Unified sessions are indexed: four top-level sessions
    // (three roots plus the F301 fork) and three subagent children.
    let connection = rusqlite::Connection::open(fixtures.db_path()).unwrap();
    let rows: Vec<(String, i64)> = connection
        .prepare("SELECT file_path, is_subagent FROM sessions WHERE tool = 'mistral_vibe'")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    let unified: Vec<&(String, i64)> = rows
        .iter()
        .filter(|(file_path, _)| is_unified_session_dir(Path::new(file_path)))
        .collect();
    assert_eq!(rows.len(), 10, "3 legacy + 7 Unified Vibe sessions");
    assert_eq!(unified.len(), 7, "7 Unified sessions: 4 roots + 3 children");
    assert_eq!(
        unified
            .iter()
            .filter(|(_, subagent)| *subagent == 0)
            .count(),
        4,
        "four Unified sessions are top-level"
    );
    assert_eq!(
        unified
            .iter()
            .filter(|(_, subagent)| *subagent == 1)
            .count(),
        3,
        "three Unified sessions are subagent children"
    );

    // Analytics per-tool counts include the Unified sessions. The four
    // top-level Unified sessions are counted; the three children follow the
    // existing `is_subagent = 0` analytics rule shared by every assistant.
    let analytics = load_analytics(fixtures.db_path()).unwrap();
    let vibe = analytics
        .sessions_by_tool
        .iter()
        .find(|row| row.tool == "Mistral Vibe")
        .expect("Mistral Vibe analytics row");
    assert_eq!(
        vibe.session_count, 7,
        "3 legacy + 4 Unified top-level sessions"
    );

    // The tool filter returns the Unified sessions alongside legacy ones.
    let hits = search(&fixtures, "", &ProjectFilter::AllSessions);
    assert_eq!(hits.len(), 7);
    assert!(hits.iter().any(|id| id == R302));
    let unified_in_filter: usize = connection
        .prepare("SELECT file_path FROM sessions WHERE tool = 'mistral_vibe' AND is_subagent = 0")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
        .into_iter()
        .filter(|file_path| is_unified_session_dir(Path::new(file_path)))
        .count();
    assert_eq!(
        unified_in_filter, 4,
        "all four Unified top-level sessions pass the filter"
    );

    // Favorites keep working on Unified sessions through the pinned filter.
    assert!(toggle_pin(fixtures.db_path(), R302).unwrap());
    let pinned = search(&fixtures, "", &ProjectFilter::Pinned);
    assert_eq!(pinned, vec![R302.to_string()]);
}

#[test]
fn unified_root_resume_and_child_resume_flags() {
    let fixtures = IndexedFixtures::new();

    let root = load_session(fixtures.db_path(), R302)
        .unwrap()
        .expect("R302 must be indexed");
    assert!(!root.is_subagent);
    assert!(is_unified_session_dir(Path::new(&root.file_path)));
    assert!(root.can_resume(), "Unified roots must stay resumable");

    let child = load_session(fixtures.db_path(), CHILD_A)
        .unwrap()
        .expect("CHILD_A must be indexed");
    assert!(child.is_subagent);
    assert_eq!(child.parent_session_id.as_deref(), Some(R302));
    assert!(is_unified_session_dir(Path::new(&child.file_path)));
    assert!(
        !child.can_resume(),
        "Unified subagents are resumed through their parent"
    );
}

#[test]
fn full_reindex_then_incremental_is_stable() {
    let fixtures = FixtureCopy::new();
    let database = NamedTempFile::new().unwrap();
    let mut indexer = SessionIndexer::new(database.path()).unwrap();

    let full = indexer
        .index_all_full_reindex(&fixtures.sources)
        .expect("full reindex must succeed");
    assert_eq!(
        vibe_source(&full).indexed,
        10,
        "3 legacy + 7 Unified sessions"
    );
    assert_eq!(vibe_source(&full).errors, 0);

    let incremental = indexer
        .index_all_incremental(&fixtures.sources)
        .expect("incremental pass must succeed");
    let vibe = vibe_source(&incremental);
    assert_eq!(
        vibe.indexed, 0,
        "unchanged Unified sessions must not reindex"
    );
    assert_eq!(vibe.skipped, 10, "all Vibe sessions are up to date");
    assert_eq!(vibe.removed, 0);
    assert_eq!(vibe.errors, 0);
}
