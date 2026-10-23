use super::*;
use crate::parsers::mistral_vibe_unified::store::test_support::{copy_tree, rewrite_document};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use tempfile::TempDir;

const R302: &str = "aaaaaaaa-0000-4000-8000-000000000302";
const F301: &str = "aaaaaaaa-0000-4000-8000-000000000301";
const MIG: &str = "aaaaaaaa-0000-4000-8000-000000000004";
const LEG: &str = "bbbbbbbb-0000-4000-8000-000000000004";
const CHILD_A: &str = "child-ea9b00b3ad4061f4ca36fd81";
const CHILD_B: &str = "child-600cf6121578955c8405618d";
const CHILD_C: &str = "child-4c966d1f1f932580b090cfb5";
const GROW: &str = "cccccccc-0000-4000-8000-000000000004";
const DELETE: &str = "cccccccc-0000-4000-8000-000000000002";
const DELETE_CHILD: &str = "child-a56c92821fcc4b3521ae913d";

struct Harness {
    temp: TempDir,
    indexer: SessionIndexer,
}

impl Harness {
    fn empty() -> Self {
        let temp = TempDir::new().unwrap();
        let indexer = SessionIndexer::new(&temp.path().join("index.db")).unwrap();
        fs::create_dir(temp.path().join("vibe")).unwrap();
        Self { temp, indexer }
    }

    fn fixtures() -> Self {
        let h = Self::empty();
        h.copy("vibe_sessions");
        h
    }

    fn root(&self) -> PathBuf {
        self.temp.path().join("vibe")
    }
    fn dir(&self, id: &str) -> PathBuf {
        self.root().join("unified").join(id)
    }
    fn copy(&self, fixture: &str) {
        copy_tree(&crate::fixture_path(fixture), &self.root());
    }
    fn add(&self, id: &str) {
        copy_tree(
            &crate::fixture_path(&format!("vibe_sessions/unified/{id}")),
            &self.dir(id),
        );
    }
    fn run(&mut self, incremental: bool) -> (IndexingStats, VecDeque<IndexingError>) {
        let mut errors = VecDeque::new();
        let stats = self
            .indexer
            .index_vibe_sessions_internal(&self.root(), incremental, &mut errors)
            .unwrap();
        (stats, errors)
    }
    fn count(&self, table: &str) -> i64 {
        self.indexer
            .db
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }
    fn exists(&self, id: &str) -> bool {
        self.indexer
            .db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1)",
                [id],
                |r| r.get(0),
            )
            .unwrap()
    }
    fn messages(&self, id: &str) -> Vec<String> {
        self.indexer
            .db
            .prepare("SELECT content FROM messages WHERE session_id=?1 ORDER BY message_index")
            .unwrap()
            .query_map([id], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    fn row(&self, id: &str) -> Vec<rusqlite::types::Value> {
        self.indexer
            .db
            .query_row("SELECT * FROM sessions WHERE id=?1", [id], |row| {
                (0..row.as_ref().column_count())
                    .map(|column| row.get(column))
                    .collect()
            })
            .unwrap()
    }
    fn links(&self, id: &str) -> Vec<Option<String>> {
        self.indexer
            .db
            .prepare("SELECT child_session_id FROM subagents WHERE session_id=?1 ORDER BY rowid")
            .unwrap()
            .query_map([id], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }
    fn signature(&self, id: &str) -> Option<String> {
        self.indexer
            .db
            .query_row(
                "SELECT signature FROM vibe_unified_signatures WHERE session_dir=?1",
                [self.dir(id).to_string_lossy().as_ref()],
                |r| r.get(0),
            )
            .optional()
            .unwrap()
    }
    fn append(&self) {
        fs::OpenOptions::new()
            .append(true)
            .open(self.dir(GROW).join("journal/0000000000000002.jsonl"))
            .unwrap()
            .write_all(
                &fs::read(crate::fixture_path(
                    "vibe_unified_variants/growing/append.jsonl",
                ))
                .unwrap(),
            )
            .unwrap();
    }
    fn assert_no_contents(&self, id: &str) {
        for table in [
            "sessions",
            "messages",
            "tool_calls",
            "subagents",
            "transcript_items",
            "reasoning_attachments",
            "context_entries",
        ] {
            let key = if table == "sessions" {
                "id"
            } else {
                "session_id"
            };
            let count: i64 = self
                .indexer
                .db
                .query_row(
                    &format!("SELECT count(*) FROM {table} WHERE {key}=?1"),
                    [id],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(count, 0, "{table} retained {id}");
        }
        assert!(self.signature(id).is_none());
    }
}

fn corrupt_document(dir: &Path) -> (PathBuf, Vec<u8>) {
    // CHILD_B has inline projection entries, R302 has pooled chunks.
    let path = if dir.join("chunks").is_dir() {
        fs::read_dir(dir.join("chunks"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path()
    } else {
        dir.join("generations/0000000000000001/projection-state.json")
    };
    let original = fs::read(&path).unwrap();
    let mut bytes = original.clone();
    bytes[0] ^= 1;
    fs::write(&path, bytes).unwrap();
    (path, original)
}

fn pool_child_projection(dir: &Path) {
    // The checked-in CHILD_B fixture is inline. Pool its entries in the temp
    // copy so this regression exercises chunk integrity, not JSON syntax.
    let generation = dir.join("generations/0000000000000001");
    let read = |path: &Path| {
        serde_json::from_slice::<serde_json::Value>(&fs::read(path).unwrap()).unwrap()
    };
    let write = |path: &Path, value: &serde_json::Value| {
        let body = serde_json::to_vec(value).unwrap();
        let digest = format!("{:x}", Sha256::digest(&body));
        let mut stored = body;
        stored.push(b'\n');
        fs::write(path, stored).unwrap();
        digest
    };
    let mut projection = read(&generation.join("projection-state.json"));
    let entries = projection["snapshot"]["history"]["entries"].take();
    let chunk = serde_json::to_vec(&entries).unwrap();
    let digest = format!("{:x}", Sha256::digest(&chunk));
    fs::create_dir(dir.join("chunks")).unwrap();
    write(&dir.join("chunks").join(format!("{digest}.json")), &entries);
    projection["snapshot"]["history"]["entries"] = serde_json::json!([]);
    let projection_digest = write(&generation.join("projection-state.json"), &projection);
    let mut manifest = read(&generation.join("manifest.json"));
    manifest["projection_state"]["sha256"] = projection_digest.into();
    manifest["projection_state"]["chunks"] = serde_json::json!([digest]);
    let manifest_digest = write(&generation.join("manifest.json"), &manifest);
    let mut pointer = read(&dir.join("CURRENT"));
    pointer["manifest_sha256"] = manifest_digest.into();
    write(&dir.join("CURRENT"), &pointer);
    MistralVibeUnifiedParser.parse(dir).unwrap();
}

#[test]
fn runtime_identity_shapes_and_conflicts_preserve_prior_state() {
    use serde_json::json;
    for field in ["root_session_id", "parent_session_id"] {
        let mut h = Harness::empty();
        h.add(CHILD_B);
        assert!(h.run(false).1.is_empty());
        let old = (h.messages(CHILD_B), h.signature(CHILD_B));
        rewrite_document(&h.dir(CHILD_B), "runtime-state.json", |doc| {
            doc["session_metadata"][field] = json!("other");
        });
        let result = MistralVibeUnifiedParser.parse(&h.dir(CHILD_B));
        assert!(
            matches!(result, Err(UnifiedError::IdentityConflict(_))),
            "{field}: {result:?}"
        );
        assert_eq!(h.run(true).0.errors, 1);
        assert_eq!(old, (h.messages(CHILD_B), h.signature(CHILD_B)));
    }
    let mut h = Harness::empty();
    h.add(R302);
    assert!(h.run(false).1.is_empty());
    let old = (h.messages(R302), h.signature(R302));
    rewrite_document(&h.dir(R302), "runtime-state.json", |doc| {
        doc["session_metadata"]
            .as_object_mut()
            .unwrap()
            .remove("parent_session_id");
    });
    assert!(matches!(
        MistralVibeUnifiedParser.parse(&h.dir(R302)),
        Err(UnifiedError::Corrupt(_))
    ));
    assert_eq!(h.run(true).0.errors, 1);
    assert_eq!(old, (h.messages(R302), h.signature(R302)));
}

#[test]
fn duplicate_subagent_ids_roundtrip_distinct_prompts_and_results() {
    use serde_json::json;
    let mut h = Harness::empty();
    h.add(CHILD_B);
    rewrite_document(&h.dir(CHILD_B), "projection-state.json", |doc| {
        let entries = doc["snapshot"]["history"]["entries"]
            .as_array_mut()
            .unwrap();
        for (prompt, result) in [
            ("First task", "First result"),
            ("Second task", "Second result"),
        ] {
            entries.push(json!({"id":"effect-repeat", "type":"effect", "generationStatus":"completed", "detail":{"kind":"subagent", "agentName":"worker", "input":{"task":prompt}}, "state":{"status":"completed", "outputText":result}}));
        }
    });
    assert!(h.run(false).1.is_empty());
    let rows: Vec<(String, String, String, String)> = h.indexer.db.prepare(
        "SELECT s.id,s.prompt,s.result_summary,s.parser_ref FROM transcript_items t JOIN subagents s ON s.id=t.subagent_id WHERE t.session_id=?1 ORDER BY t.item_index"
    ).unwrap().query_map([CHILD_B], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap();
    assert_eq!(rows.len(), 2);
    assert_ne!(rows[0].0, rows[1].0);
    assert_eq!(
        (&rows[0].1, &rows[0].2, &rows[0].3),
        (
            &"First task".into(),
            &"First result".into(),
            &"repeat".into()
        )
    );
    assert_eq!(
        (&rows[1].1, &rows[1].2, &rows[1].3),
        (
            &"Second task".into(),
            &"Second result".into(),
            &"repeat".into()
        )
    );
}

#[test]
fn symlinked_unified_ancestor_is_indexed() {
    let mut h = Harness::fixtures();
    let target = h.temp.path().join("stores");
    fs::rename(h.root().join("unified"), &target).unwrap();
    std::os::unix::fs::symlink(target, h.root().join("unified")).unwrap();
    let (stats, errors) = h.run(true);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(stats.indexed, 10);
}

#[test]
fn snapshot_envelope_rejections_preserve_prior_contents_and_signature() {
    use crate::parsers::mistral_vibe_unified::store::test_support::rewrite_journal_record;
    use serde_json::{Value, json};
    let mut failures = Vec::new();
    for mode in ["initial", "advanced", "set_envelope"] {
        for (pointer, value, expected) in [
            ("/session/id", json!("other"), "IdentityConflict"),
            ("/session/id", Value::Null, "Corrupt"),
            ("/session/rootSessionId", json!("other"), "IdentityConflict"),
            (
                "/session/parentSessionId",
                json!("other"),
                "IdentityConflict",
            ),
            ("/format", json!("future"), "Unsupported"),
            ("/format", Value::Null, "Corrupt"),
        ] {
            let id = if mode == "initial" { CHILD_B } else { R302 };
            let mut h = Harness::empty();
            h.add(id);
            assert!(h.run(false).1.is_empty());
            let old = (h.row(id), h.messages(id), h.signature(id));
            if mode == "initial" {
                rewrite_document(&h.dir(id), "projection-state.json", |doc| {
                    *doc["snapshot"].pointer_mut(pointer).unwrap() = value;
                });
            } else {
                let current: Value =
                    serde_json::from_slice(&fs::read(h.dir(id).join("CURRENT")).unwrap()).unwrap();
                let path = h
                    .dir(id)
                    .join("generations")
                    .join(current["generation"].as_str().unwrap())
                    .join("projection-state.json");
                let doc: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
                let mut snapshot = doc["snapshot"].clone();
                *snapshot.pointer_mut(pointer).unwrap() = value;
                rewrite_journal_record(&h.dir(id), 10, |record| {
                    if mode == "advanced" {
                        record["type"] = json!("projection_advanced");
                        record["payload"] = json!({"watermark":10, "snapshot":snapshot});
                    } else {
                        record["type"] = json!("projection_delta");
                        record["payload"] = json!({"watermark":10,"delta":[{"op":"set_envelope", "state":snapshot}]});
                    }
                });
            }
            let result = MistralVibeUnifiedParser.parse(&h.dir(id));
            let classified = matches!(
                (&result, expected),
                (Err(UnifiedError::Corrupt(_)), "Corrupt")
                    | (Err(UnifiedError::Unsupported(_)), "Unsupported")
                    | (Err(UnifiedError::IdentityConflict(_)), "IdentityConflict")
            );
            let (stats, _) = h.run(true);
            if !classified
                || stats.errors != 1
                || old != (h.row(id), h.messages(id), h.signature(id))
            {
                failures.push(format!("{mode} {pointer}: expected {expected}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn malformed_reasoning_preserves_prior_contents_and_signature() {
    use serde_json::json;
    for (field, value, unsupported) in [
        ("outcome", json!({"type":"future"}), true),
        ("generationStatus", json!("future"), true),
        ("text", json!(42), false),
        ("summary", json!([42]), false),
    ] {
        let mut h = Harness::empty();
        h.add(CHILD_B);
        assert!(h.run(false).1.is_empty());
        let old = (h.messages(CHILD_B), h.signature(CHILD_B));
        rewrite_document(&h.dir(CHILD_B), "projection-state.json", |doc| {
            let mut reasoning = json!({"id":"r", "type":"reasoning", "generationStatus":"completed", "text":"thinking", "summary":[], "outcome":{"type":"committed"}});
            reasoning[field] = value;
            doc["snapshot"]["history"]["entries"]
                .as_array_mut()
                .unwrap()
                .insert(1, reasoning);
        });
        let result = MistralVibeUnifiedParser.parse(&h.dir(CHILD_B));
        assert!(
            if unsupported {
                matches!(result, Err(UnifiedError::Unsupported(_)))
            } else {
                matches!(result, Err(UnifiedError::Corrupt(_)))
            },
            "{field}: {result:?}"
        );
        assert_eq!(h.run(true).0.errors, 1);
        assert_eq!(old, (h.messages(CHILD_B), h.signature(CHILD_B)));
    }
}

#[test]
fn closed_state_unions_preserve_prior_contents_and_signature() {
    use serde_json::json;
    let cases = [
        (
            "/history/entries/1/outcome/type",
            json!("future"),
            "Unsupported",
        ),
        ("/history/entries/1/outcome", json!(null), "Corrupt"),
        (
            "/history/entries/1/outcome",
            json!({"type":"discarded"}),
            "Corrupt",
        ),
        (
            "/history/entries/1/outcome",
            json!({"type":"discarded", "cause":"future"}),
            "Unsupported",
        ),
        (
            "/history/entries/1/generationStatus",
            json!("future"),
            "Unsupported",
        ),
        (
            "/history/entries/1/generationStatus",
            json!(null),
            "Corrupt",
        ),
        ("/session/status/type", json!("future"), "Unsupported"),
        ("/session/status", json!(null), "Corrupt"),
        ("/latestTurn/status", json!("future"), "Unsupported"),
        ("/latestTurn/status", json!(null), "Corrupt"),
    ];
    let mut failures = Vec::new();
    for (pointer, value, expected) in cases {
        let mut h = Harness::empty();
        h.add(CHILD_B);
        assert!(h.run(false).1.is_empty());
        let old = (h.row(CHILD_B), h.messages(CHILD_B), h.signature(CHILD_B));
        rewrite_document(&h.dir(CHILD_B), "projection-state.json", |doc| {
            *doc["snapshot"].pointer_mut(pointer).unwrap() = value;
        });
        let result = MistralVibeUnifiedParser.parse(&h.dir(CHILD_B));
        let classified = matches!(
            (&result, expected),
            (Err(UnifiedError::Corrupt(_)), "Corrupt")
                | (Err(UnifiedError::Unsupported(_)), "Unsupported")
        );
        let (stats, _) = h.run(true);
        if !classified
            || stats.errors != 1
            || old != (h.row(CHILD_B), h.messages(CHILD_B), h.signature(CHILD_B))
        {
            failures.push(format!("{pointer}: expected {expected}, got {result:?}"));
        }
    }
    for (kind, status, expected) in [
        ("future", "completed", "Unsupported"),
        ("tool", "pending", "Unsupported"),
        ("tool", "failed", "Corrupt"),
    ] {
        let mut h = Harness::empty();
        h.add(CHILD_B);
        assert!(h.run(false).1.is_empty());
        let old = (h.row(CHILD_B), h.messages(CHILD_B), h.signature(CHILD_B));
        rewrite_document(&h.dir(CHILD_B), "projection-state.json", |doc| {
            doc["snapshot"]["history"]["entries"]
                .as_array_mut()
                .unwrap()
                .push(json!({
                    "id":"effect-bad", "type":"effect", "generationStatus":"completed",
                    "detail":{"kind":kind,"toolName":"read_file"}, "state":{"status":status}
                }));
        });
        let result = MistralVibeUnifiedParser.parse(&h.dir(CHILD_B));
        let classified = matches!(
            (&result, expected),
            (Err(UnifiedError::Corrupt(_)), "Corrupt")
                | (Err(UnifiedError::Unsupported(_)), "Unsupported")
        );
        let (stats, _) = h.run(true);
        if !classified
            || stats.errors != 1
            || old != (h.row(CHILD_B), h.messages(CHILD_B), h.signature(CHILD_B))
        {
            failures.push(format!("effect {kind}/{status}: {result:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn indexes_unified_and_legacy_side_by_side() {
    let mut h = Harness::fixtures();
    let (stats, errors) = h.run(false);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(stats.indexed, 10);
    assert_eq!(h.count("sessions"), 10);
    let values: (String, i64, i64, i64) = h
        .indexer
        .db
        .query_row(
            "SELECT ending_status, edit_count, read_count, command_count FROM sessions WHERE id=?1",
            [R302],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(values, ("clean".into(), 2, 1, 1));
}

#[test]
fn explicit_and_derived_children_are_linked() {
    let mut h = Harness::fixtures();
    assert!(h.run(false).1.is_empty());
    assert_eq!(
        h.links(R302),
        vec![Some(CHILD_A.into()), Some(CHILD_B.into()), None]
    );
    assert_eq!(h.links(F301), vec![Some(CHILD_C.into())]);
}

#[test]
fn child_indexed_before_parent_is_linked() {
    for child_first in [true, false] {
        let mut h = Harness::empty();
        h.add(if child_first { CHILD_A } else { R302 });
        assert!(h.run(true).1.is_empty());
        h.add(if child_first { R302 } else { CHILD_A });
        let (stats, errors) = h.run(true);
        assert!(errors.is_empty());
        assert_eq!(stats.skipped, 1);
        assert_eq!(h.links(R302)[0], Some(CHILD_A.into()));
    }
}

#[test]
fn deeper_nesting_is_rejected_in_both_orders() {
    let child = "child-cd3a1a55a67abb357cddaa83";
    let grandchild = "child-f3f3440f1d900f6bd4712277";
    for child_first in [true, false] {
        let mut h = Harness::empty();
        h.copy("vibe_unified_variants/deeper");
        let root = h.root().join("unified");
        let ids = if child_first {
            [child, grandchild]
        } else {
            [grandchild, child]
        };
        let mut stats = IndexingStats::default();
        let mut errors = VecDeque::new();
        h.indexer
            .index_unified_candidates(
                &root,
                ids.map(|id| root.join(id)).into_iter(),
                false,
                &mut stats,
                &mut errors,
            )
            .unwrap();
        h.assert_no_contents(grandchild);
        assert!(h.exists(child));
        assert_eq!(stats.errors, 1);
        assert!(errors[0].message.contains("nesting"));
        assert_eq!(h.links(child), vec![None]);
    }
}

#[test]
fn deeper_nesting_is_removed_when_parent_is_revealed_cross_pass() {
    let child = "child-cd3a1a55a67abb357cddaa83";
    let grandchild = "child-f3f3440f1d900f6bd4712277";
    for child_first in [true, false] {
        let mut h = Harness::empty();
        let ids = if child_first {
            [child, grandchild]
        } else {
            [grandchild, child]
        };
        for id in ids {
            copy_tree(
                &crate::fixture_path(&format!("vibe_unified_variants/deeper/unified/{id}")),
                &h.dir(id),
            );
            let (_, errors) = h.run(true);
            if id == ids[0] {
                assert!(errors.is_empty());
                assert!(h.exists(id));
            }
        }
        h.assert_no_contents(grandchild);
        assert_eq!(h.links(child), vec![None]);
        assert!(h.exists(child));
    }
}

#[test]
fn deeper_nesting_cleanup_does_not_claim_other_source_rows() {
    let child = "child-cd3a1a55a67abb357cddaa83";
    let grandchild = "child-f3f3440f1d900f6bd4712277";
    let mut h = Harness::empty();
    copy_tree(
        &crate::fixture_path(&format!(
            "vibe_unified_variants/deeper/unified/{grandchild}"
        )),
        &h.dir(grandchild),
    );
    assert!(h.run(true).1.is_empty());
    let outside = h.temp.path().join("other-source/unified").join(grandchild);
    fs::create_dir_all(&outside).unwrap();
    h.indexer
        .db
        .execute(
            "UPDATE sessions SET file_path=?1 WHERE id=?2",
            rusqlite::params![outside.to_string_lossy(), grandchild],
        )
        .unwrap();
    h.indexer
        .db
        .execute(
            "UPDATE vibe_unified_signatures SET session_dir=?1 WHERE session_dir=?2",
            rusqlite::params![
                outside.to_string_lossy(),
                h.dir(grandchild).to_string_lossy()
            ],
        )
        .unwrap();
    let old = h.row(grandchild);
    copy_tree(
        &crate::fixture_path(&format!("vibe_unified_variants/deeper/unified/{child}")),
        &h.dir(child),
    );
    h.run(true);
    assert_eq!(old, h.row(grandchild));
    let n: i64 = h
        .indexer
        .db
        .query_row(
            "SELECT count(*) FROM vibe_unified_signatures WHERE session_dir=?1",
            [outside.to_string_lossy().as_ref()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);
}

#[test]
fn incremental_noop_skips_all_unified() {
    let mut h = Harness::fixtures();
    assert!(h.run(true).1.is_empty());
    let (stats, errors) = h.run(true);
    assert!(errors.is_empty());
    assert_eq!(stats.indexed, 0);
    assert_eq!(stats.skipped, 10);
    assert_eq!(h.count("vibe_unified_signatures"), 7);
}

#[test]
fn journal_append_reindexes_only_that_session() {
    let mut h = Harness::fixtures();
    h.copy("vibe_unified_variants/growing");
    assert!(h.run(true).1.is_empty());
    h.append();
    let (stats, errors) = h.run(true);
    assert!(errors.is_empty());
    assert_eq!((stats.indexed, stats.skipped), (1, 10));
    assert!(h.messages(GROW).contains(&"Late answer".into()));
    assert_eq!(h.run(true).0.indexed, 0);
}

#[test]
fn meta_json_change_reindexes() {
    let mut h = Harness::fixtures();
    assert!(h.run(true).1.is_empty());
    // Appearance matters as well as mutation and disappearance.
    fs::write(h.dir(R302).join("meta.json"), b"{}").unwrap();
    assert_eq!(h.run(true).0.indexed, 1);
    fs::write(h.dir(R302).join("meta.json"), b"{\"model\":\"changed\"}").unwrap();
    assert_eq!(h.run(true).0.indexed, 1);
    fs::remove_file(h.dir(R302).join("meta.json")).unwrap();
    assert_eq!(h.run(true).0.indexed, 1);
}

#[test]
fn saved_result_touch_reindexes() {
    let mut h = Harness::fixtures();
    assert!(h.run(true).1.is_empty());
    let path = h.dir(R302).join("tool-results/bash-call%3A9.json");
    let bytes = fs::read(&path).unwrap();
    fs::write(&path, bytes).unwrap();
    let file = fs::File::options().write(true).open(path).unwrap();
    file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(2))
        .unwrap();
    let (stats, errors) = h.run(true);
    assert!(errors.is_empty());
    assert_eq!((stats.indexed, stats.skipped), (1, 9));
}

#[test]
fn favorite_survives_reindex() {
    let mut h = Harness::empty();
    h.copy("vibe_unified_variants/growing");
    assert!(h.run(true).1.is_empty());
    assert!(crate::database::toggle_pin(&h.temp.path().join("index.db"), GROW).unwrap());
    h.append();
    assert_eq!(h.run(true).0.indexed, 1);
    assert!(h.messages(GROW).contains(&"Late answer".into()));
    let pin: Option<i64> = h
        .indexer
        .db
        .query_row("SELECT pinned_at FROM sessions WHERE id=?1", [GROW], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(pin.is_some());
}

#[test]
fn failure_keeps_previous_row() {
    let mut h = Harness::fixtures();
    assert!(h.run(true).1.is_empty());
    let before = h.messages(R302);
    let row = h.row(R302);
    let signature = h.signature(R302);
    let (path, original) = corrupt_document(&h.dir(R302));
    // Immutable chunks are deliberately outside incremental freshness scope.
    let (stats, errors) = h.run(false);
    assert_eq!((stats.errors, stats.indexed), (1, 9));
    assert_eq!(errors.len(), 1);
    assert_eq!(
        errors[0].location.as_deref(),
        Some(h.dir(R302).to_str().unwrap())
    );
    assert_eq!(h.messages(R302), before);
    assert_eq!(h.row(R302), row);
    assert_eq!(h.signature(R302), signature);
    fs::write(path, original).unwrap();
    let (stats, errors) = h.run(false);
    assert!(errors.is_empty());
    assert_eq!(stats.indexed, 10);
    assert_eq!(h.messages(R302), before);
}

#[test]
fn corrupt_child_never_indexed_is_not_linked() {
    let mut h = Harness::fixtures();
    pool_child_projection(&h.dir(CHILD_B));
    corrupt_document(&h.dir(CHILD_B));
    let (stats, errors) = h.run(false);
    assert_eq!(stats.errors, 1);
    assert_eq!(errors.len(), 1);
    assert!(!h.exists(CHILD_B));
    assert_eq!(h.links(R302)[1], None);
}

#[test]
fn ignores_scratchpad_catalog_and_staging_entries() {
    let mut h = Harness::fixtures();
    for name in ["scratchpad", ".staging-1", "not valid!"] {
        let path = h.root().join("unified").join(name);
        fs::create_dir(&path).unwrap();
        fs::write(path.join("CURRENT"), b"invalid").unwrap();
    }
    fs::write(h.root().join("unified/.session-index.json"), b"invalid").unwrap();
    assert!(h.run(false).1.is_empty());
    assert_eq!(h.count("sessions"), 10);
}

#[test]
fn missing_saved_result_then_restored() {
    let mut h = Harness::fixtures();
    assert!(h.run(true).1.is_empty());
    let before = h.messages(R302);
    let signature = h.signature(R302);
    let path = h.dir(R302).join("tool-results/bash-call%3A9.json");
    let bytes = fs::read(&path).unwrap();
    fs::remove_file(&path).unwrap();
    let (stats, errors) = h.run(true);
    assert_eq!((stats.errors, stats.skipped), (1, 9));
    assert_eq!(errors.len(), 1);
    assert_eq!(h.messages(R302), before);
    assert_eq!(h.signature(R302), signature);
    fs::write(path, bytes).unwrap();
    let (stats, errors) = h.run(true);
    assert!(errors.is_empty());
    assert_eq!(stats.indexed, 1);
}

#[test]
fn deleted_dir_removes_row_and_clears_parent_link() {
    let mut h = Harness::fixtures();
    assert!(h.run(true).1.is_empty());
    fs::remove_dir_all(h.dir(CHILD_B)).unwrap();
    let (stats, errors) = h.run(true);
    assert!(errors.is_empty());
    assert_eq!(stats.removed, 1);
    h.assert_no_contents(CHILD_B);
    assert_eq!(h.links(R302)[1], None);
    let before = h.count("sessions");
    fs::rename(h.root().join("unified"), h.root().join("offline")).unwrap();
    assert_eq!(h.run(true).0.removed, 0);
    assert_eq!(h.count("sessions"), before);
    fs::write(h.root().join("unified"), b"not a directory").unwrap();
    assert_eq!(h.run(true).0.removed, 0);
    assert_eq!(h.count("sessions"), before);
}

#[test]
fn deleting_tree_removes_root_and_children() {
    for incremental in [true, false] {
        let mut h = Harness::empty();
        h.copy("vibe_unified_variants/deleting/active");
        assert!(h.run(true).1.is_empty());
        assert!(h.exists(DELETE_CHILD));
        h.copy("vibe_unified_variants/deleting/deleting");
        let (stats, errors) = h.run(incremental);
        assert!(errors.is_empty());
        assert_eq!(stats.removed, 2);
        h.assert_no_contents(DELETE);
        h.assert_no_contents(DELETE_CHILD);
        assert_eq!(h.run(incremental).0.indexed, 0);
        h.assert_no_contents(DELETE_CHILD);
    }
}

#[test]
fn ephemeral_store_is_not_indexed() {
    let mut h = Harness::empty();
    h.copy("vibe_unified_variants/ephemeral");
    assert!(h.run(false).1.is_empty());
    assert_eq!(h.count("sessions"), 0);
    assert_eq!(h.count("vibe_unified_signatures"), 0);
    h.add(R302);
    assert!(h.run(true).1.is_empty());
    rewrite_document(&h.dir(R302), "runtime-state.json", |doc| {
        doc["storage_lifetime"] = "ephemeral".into()
    });
    assert!(h.run(true).1.is_empty());
    h.assert_no_contents(R302);
}

#[test]
fn absent_current_in_indexed_dir_is_diagnosed_and_kept() {
    let mut h = Harness::fixtures();
    assert!(h.run(true).1.is_empty());
    let before = h.messages(R302);
    let signature = h.signature(R302);
    fs::remove_file(h.dir(R302).join("CURRENT")).unwrap();
    let (stats, errors) = h.run(true);
    assert_eq!(stats.errors, 1);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].message.contains("Incomplete"));
    assert_eq!(h.messages(R302), before);
    assert_eq!(h.signature(R302), signature);
}

#[test]
fn legacy_id_collision_keeps_prior_record() {
    let mut h = Harness::empty();
    let legacy = h.root().join("legacy");
    fs::create_dir(&legacy).unwrap();
    fs::write(
        legacy.join("meta.json"),
        serde_json::json!({"session_id": R302, "start_time":"2026-10-20T10:00:00Z"}).to_string(),
    )
    .unwrap();
    fs::write(
        legacy.join("messages.jsonl"),
        b"{\"role\":\"user\",\"content\":\"Legacy collision prompt\"}\n",
    )
    .unwrap();
    assert!(h.run(true).1.is_empty());
    assert!(h.exists(R302));
    h.add(R302);
    let (stats, errors) = h.run(true);
    assert_eq!(stats.errors, 1);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].message.contains(legacy.to_str().unwrap()));
    assert_eq!(h.messages(R302), vec!["Legacy collision prompt"]);
    let path: String = h
        .indexer
        .db
        .query_row("SELECT file_path FROM sessions WHERE id=?1", [R302], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(path, legacy.to_string_lossy());
}

#[test]
fn migrated_legacy_and_unified_are_both_indexed() {
    let mut h = Harness::fixtures();
    assert!(h.run(false).1.is_empty());
    assert!(h.exists(LEG) && h.exists(MIG));
    assert_eq!(h.messages(LEG).len(), 2);
    assert_eq!(
        h.messages(MIG),
        vec![
            "Ancienne question",
            "Ancienne réponse",
            "Nouvelle question",
            "Nouvelle réponse"
        ]
    );
}

#[test]
fn meta_json_appearing_invalidates_signature() {
    let mut h = Harness::empty();
    h.copy("vibe_unified_variants/growing");
    assert!(!h.dir(GROW).join("meta.json").exists());
    assert!(h.run(true).1.is_empty());
    fs::write(h.dir(GROW).join("meta.json"), b"{}").unwrap();
    assert_eq!(h.run(true).0.indexed, 1);
}

#[test]
fn explicit_child_binding_must_match_spawn_action() {
    let mut h = Harness::fixtures();
    assert!(h.run(true).1.is_empty());
    // A valid depth-one child of the same root, but from a different action,
    // is not evidence that this spawn effect owns it.
    let mut parent = MistralVibeUnifiedParser.parse(&h.dir(R302)).unwrap();
    parent.parsed.subagents[0].agent_id = Some(CHILD_B.into());
    let tx = h.indexer.db.transaction().unwrap();
    SessionIndexer::replace_session_contents_tx(&tx, &parent.parsed).unwrap();
    SessionIndexer::link_vibe_unified_subagents_tx(&tx, &parent.parsed).unwrap();
    tx.commit().unwrap();
    assert_eq!(h.links(R302)[0], None);
    // The reciprocal child-side link update must enforce the same evidence.
    let child = MistralVibeUnifiedParser.parse(&h.dir(CHILD_B)).unwrap();
    let tx = h.indexer.db.transaction().unwrap();
    SessionIndexer::link_vibe_unified_subagents_tx(&tx, &child.parsed).unwrap();
    tx.commit().unwrap();
    assert_eq!(h.links(R302)[0], None);
}

#[test]
fn no_user_messages_prunes_contents_links_and_signature() {
    let mut h = Harness::fixtures();
    assert!(h.run(true).1.is_empty());
    let path = h.dir(CHILD_B);
    rewrite_document(&path, "projection-state.json", |doc| {
        doc["snapshot"]["history"]["entries"] = serde_json::json!([]);
    });
    assert!(h.run(true).1.is_empty());
    h.assert_no_contents(CHILD_B);
    assert_eq!(h.links(R302)[1], None);
}

#[test]
fn clear_all_sessions_clears_unified_signatures() {
    let mut h = Harness::fixtures();
    assert!(h.run(false).1.is_empty());
    assert_eq!(h.count("vibe_unified_signatures"), 7);
    h.indexer.clear_all_sessions().unwrap();
    assert_eq!(h.count("vibe_unified_signatures"), 0);
    assert_eq!(h.run(true).0.indexed, 10);
}

#[test]
fn unified_signature_and_contents_commit_atomically() {
    let mut h = Harness::empty();
    h.copy("vibe_unified_variants/growing");
    assert!(h.run(true).1.is_empty());
    let before = h.messages(GROW);
    let signature = h.signature(GROW);
    h.append();
    h.indexer.db.execute_batch("CREATE TRIGGER fail_signature BEFORE UPDATE ON vibe_unified_signatures BEGIN SELECT RAISE(ABORT, 'signature failure'); END;").unwrap();
    let mut errors = VecDeque::new();
    assert!(
        h.indexer
            .index_vibe_sessions_internal(&h.root(), true, &mut errors)
            .is_err()
    );
    assert_eq!(h.messages(GROW), before);
    assert_eq!(h.signature(GROW), signature);
    h.indexer
        .db
        .execute_batch("DROP TRIGGER fail_signature")
        .unwrap();
    assert_eq!(h.run(true).0.indexed, 1);
}

#[test]
fn unified_pointer_flip_during_read_defers_then_reports() {
    let mut h = Harness::fixtures();
    assert!(h.run(true).1.is_empty());
    let path = h.dir(R302);
    let before = h.messages(R302);
    let signature = h.signature(R302);
    let mut stats = IndexingStats::default();
    let mut errors = VecDeque::new();
    for attempt in 1..=3 {
        h.indexer
            .handle_unified_outcome(
                &path,
                Err(UnifiedError::Unstable("pointer flipped".into())),
                &mut stats,
                &mut errors,
            )
            .unwrap();
        assert_eq!(stats.skipped, 1);
        assert_eq!(errors.len(), attempt - 1);
        assert_eq!(h.messages(R302), before);
        assert_eq!(h.signature(R302), signature);
    }
    let parsed = MistralVibeUnifiedParser.parse(&path).unwrap();
    h.indexer
        .handle_unified_outcome(&path, Ok(parsed), &mut stats, &mut errors)
        .unwrap();
    assert_eq!(
        UNIFIED_DEFERRALS
            .lock()
            .unwrap()
            .get(&path)
            .copied()
            .unwrap_or(0),
        0
    );
    h.indexer
        .handle_unified_outcome(
            &path,
            Err(UnifiedError::Unstable("new race".into())),
            &mut stats,
            &mut errors,
        )
        .unwrap();
    assert_eq!(stats.skipped, 2);
    assert_eq!(errors.len(), 2);
    // Physical deletion also resets a pending deferral.
    fs::remove_dir_all(path.clone()).unwrap();
    h.run(true);
    assert!(!UNIFIED_DEFERRALS.lock().unwrap().contains_key(&path));
}

#[test]
fn deleting_tree_is_atomic_and_cleans_unlisted_owned_children() {
    let mut h = Harness::empty();
    h.copy("vibe_unified_variants/deleting/active");
    assert!(h.run(true).1.is_empty());
    let root_signature = h.signature(DELETE);
    let child_signature = h.signature(DELETE_CHILD);
    h.indexer.db.execute_batch("CREATE TRIGGER fail_child_delete BEFORE DELETE ON sessions WHEN OLD.is_subagent=1 BEGIN SELECT RAISE(ABORT, 'child deletion failed'); END;").unwrap();
    let mut stats = IndexingStats::default();
    let mut errors = VecDeque::new();
    // Empty list deliberately tests ownership rather than only the list.
    let result = h.indexer.handle_unified_outcome(
        &h.dir(DELETE),
        Err(UnifiedError::DeletingTree {
            child_session_ids: vec![],
        }),
        &mut stats,
        &mut errors,
    );
    assert!(result.is_err());
    assert!(h.exists(DELETE) && h.exists(DELETE_CHILD));
    assert_eq!(h.signature(DELETE), root_signature);
    assert_eq!(h.signature(DELETE_CHILD), child_signature);
    assert_eq!(stats.removed, 0);
    h.indexer
        .db
        .execute_batch("DROP TRIGGER fail_child_delete")
        .unwrap();
    h.indexer
        .handle_unified_outcome(
            &h.dir(DELETE),
            Err(UnifiedError::DeletingTree {
                child_session_ids: vec![],
            }),
            &mut stats,
            &mut errors,
        )
        .unwrap();
    assert_eq!(stats.removed, 2);
    h.assert_no_contents(DELETE);
    h.assert_no_contents(DELETE_CHILD);
}

#[test]
fn source_root_absence_never_prunes_unified_rows() {
    let mut h = Harness::fixtures();
    assert!(h.run(true).1.is_empty());
    fs::rename(h.root(), h.temp.path().join("offline")).unwrap();
    assert_eq!(h.run(true).0.removed, 0);
    assert_eq!(h.count("sessions"), 10);
    assert_eq!(h.count("vibe_unified_signatures"), 7);
}

#[test]
fn unified_root_disappearing_after_enumeration_never_prunes_rows() {
    let mut h = Harness::fixtures();
    assert!(h.run(true).1.is_empty());
    let root = h.root().join("unified");
    // read_dir succeeded, but the mount/source is gone before the sweep.
    let entries = fs::read_dir(&root).unwrap();
    fs::rename(&root, h.root().join("offline")).unwrap();
    let mut stats = IndexingStats::default();
    let mut errors = VecDeque::new();
    h.indexer
        .index_unified_candidates(
            &root,
            entries.map(|entry| entry.unwrap().path()),
            true,
            &mut stats,
            &mut errors,
        )
        .unwrap();
    assert_eq!(stats.removed, 0);
    assert_eq!(h.count("sessions"), 10);
    assert_eq!(h.count("vibe_unified_signatures"), 7);
}

#[test]
fn legacy_collision_with_existing_unified_keeps_prior_record() {
    let mut h = Harness::empty();
    h.add(R302);
    assert!(h.run(true).1.is_empty());
    let before = h.messages(R302);
    let signature = h.signature(R302);
    let legacy = h.root().join("legacy");
    fs::create_dir(&legacy).unwrap();
    fs::write(
        legacy.join("meta.json"),
        serde_json::json!({"session_id":R302, "start_time":"2026-10-20T10:00:00Z"}).to_string(),
    )
    .unwrap();
    fs::write(
        legacy.join("messages.jsonl"),
        b"{\"role\":\"user\",\"content\":\"colliding legacy\"}\n",
    )
    .unwrap();
    let (stats, errors) = h.run(true);
    assert_eq!(stats.errors, 1);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].message.contains(h.dir(R302).to_str().unwrap()));
    assert_eq!(h.messages(R302), before);
    assert_eq!(h.signature(R302), signature);
}

#[test]
fn malformed_or_old_reader_signature_forces_reindex() {
    let mut h = Harness::fixtures();
    assert!(h.run(true).1.is_empty());
    h.indexer
        .db
        .execute(
            "UPDATE vibe_unified_signatures SET signature='not json' WHERE session_dir=?1",
            [h.dir(R302).to_string_lossy().as_ref()],
        )
        .unwrap();
    assert_eq!(h.run(true).0.indexed, 1);
    let mut sig = h.indexer.unified_signature(&h.dir(R302)).unwrap().unwrap();
    sig.reader_version += 1;
    h.indexer
        .db
        .execute(
            "UPDATE vibe_unified_signatures SET signature=?1 WHERE session_dir=?2",
            rusqlite::params![sig.to_storage(), h.dir(R302).to_string_lossy()],
        )
        .unwrap();
    assert_eq!(h.run(true).0.indexed, 1);
}

#[test]
fn deleting_tree_never_reinserts_children_in_either_enumeration_order() {
    for child_first in [true, false] {
        let mut h = Harness::empty();
        h.copy("vibe_unified_variants/deleting/active");
        assert!(h.run(true).1.is_empty());
        h.copy("vibe_unified_variants/deleting/deleting");
        let mut paths = vec![h.dir(DELETE), h.dir(DELETE_CHILD)];
        if child_first {
            paths.reverse();
        }
        // No child INSERT is allowed, even transiently before tree cleanup.
        h.indexer.db.execute_batch("CREATE TRIGGER reject_reinsert BEFORE INSERT ON sessions WHEN NEW.is_subagent=1 BEGIN SELECT RAISE(ABORT, 'deleted child reinserted'); END;").unwrap();
        let mut stats = IndexingStats::default();
        let mut errors = VecDeque::new();
        h.indexer
            .index_unified_candidates(
                &h.root().join("unified"),
                paths.into_iter(),
                false,
                &mut stats,
                &mut errors,
            )
            .unwrap();
        assert!(errors.is_empty());
        assert_eq!((stats.indexed, stats.removed), (0, 2));
        h.assert_no_contents(DELETE);
        h.assert_no_contents(DELETE_CHILD);
    }
}
