//! Streaming recovery-journal replay for Mistral Vibe Unified stores.
//!
//! The generation loader returns the snapshot projection of one
//! generation; this module streams that generation's journal segment
//! bounded by the size sampled while loading, validates every record
//! (pinned fact E: digest, version, type, sequence, chain) and applies
//! the projection records on top of the snapshot (pinned fact F).  A
//! final line fragment without its newline is ignored, exactly like
//! upstream.  Read stability — retrying a whole load when the store is
//! republished mid-read — lives in [`super::load_stable`].

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};
use std::path::Path;

use serde_json::Value;

use super::canonical::sha256_hex;
use super::error::UnifiedError;
use super::store::{LoadedStore, ProjectionState, reject_symlink};

/// Pinned snapshot format of the public session state (fact G): the same
/// gate the generation loader applies to the projection document.
const PROJECTION_FORMAT: &str = "harness.public-session-state/v1";

/// The 14 journal record kinds of `store_format` minor 9 (fact E).
const RECORD_KINDS: [&str; 14] = [
    "command_reserved",
    "core_input",
    "action_intent",
    "action_result",
    "process_operation_dispatched",
    "process_state_changed",
    "process_notification_submitted",
    "callback_registered",
    "callback_resolved",
    "receipt_succeeded",
    "receipt_failed",
    "projection_advanced",
    "projection_delta",
    "session_relocated",
];

/// The record kinds that move the public projection (fact F).
const PROJECTION_KINDS: [&str; 3] = [
    "projection_advanced",
    "projection_delta",
    "session_relocated",
];

/// Upstream digest slicing marker (fact E): the digest follows the last
/// occurrence of this byte sequence in the record line.
const RECORD_SHA_MARKER: &[u8] = b",\"record_sha256\":\"";

/// Replays the journal segment named by the manifest over the loaded
/// generation, reading at most `store.journal.size` bytes (the size
/// sampled before returning the generation): growth after the sample
/// stays invisible, so a concurrently written store is published as a
/// validated prefix.  Every complete line is digest-, version-, type-,
/// payload-, sequence- and chain-checked (fact E); projection records
/// are applied (fact F) and `session_relocated` also updates the runtime
/// working directory.  The sampled prefix of a replaced or shrunken
/// segment still validates because the digest and chain rules pin every
/// record on its own.
pub(crate) fn replay_journal(
    session_dir: &Path,
    store: &mut LoadedStore,
) -> Result<(), UnifiedError> {
    replay_journal_with_open_hook(session_dir, store, || {})
}

fn replay_journal_with_open_hook(
    session_dir: &Path,
    store: &mut LoadedStore,
    after_open: impl FnOnce(),
) -> Result<(), UnifiedError> {
    let journal_path = session_dir.join(&store.journal.relative_path);
    reject_symlink(&journal_path)?;
    let file = File::open(&journal_path).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => UnifiedError::Incomplete(format!(
            "journal segment {} is missing",
            store.journal.relative_path
        )),
        _ => UnifiedError::Io(error),
    })?;
    after_open();
    let mut reader = BufReader::new(file).take(store.journal.size);

    let mut line: Vec<u8> = Vec::new();
    let mut expected_sequence = store.journal_first_sequence;
    let mut previous_digest: Option<String> = None;
    loop {
        line.clear();
        let read = reader.read_until(b'\n', &mut line)?;
        if read == 0 || line.last() != Some(&b'\n') {
            // Fact E: a final fragment without its newline is ignored.
            break;
        }
        let record_line = &line[..line.len() - 1];
        let digest = record_digest(record_line)?;
        let record: Value = serde_json::from_slice(record_line).map_err(|error| {
            UnifiedError::Corrupt(format!("journal record is not valid JSON: {error}"))
        })?;
        apply_record(
            &record,
            &digest,
            &mut expected_sequence,
            &mut previous_digest,
            store,
        )?;
    }
    Ok(())
}

/// Applies one journal record on top of the loaded store.  `digest` is
/// the record's line digest (fact E); the envelope checks run in the
/// order version, type, payload, sequence, chain, then the watermark
/// rule and the projection dispatch (fact F).  Every other known kind
/// was fully envelope-checked and is skipped.
fn apply_record(
    record: &Value,
    digest: &str,
    expected_sequence: &mut u64,
    previous_digest: &mut Option<String>,
    store: &mut LoadedStore,
) -> Result<(), UnifiedError> {
    // The parsed record_sha256 must agree with the line digest (fact E).
    let record_sha256 = record
        .get("record_sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            UnifiedError::Corrupt("journal record record_sha256 is missing".to_string())
        })?;
    if record_sha256 != digest {
        return Err(UnifiedError::Corrupt(
            "journal record record_sha256 does not match its line digest".to_string(),
        ));
    }

    let version = record
        .get("recovery_journal_record_version")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            UnifiedError::Corrupt("journal record version is not an integer".to_string())
        })?;
    if version != 1 {
        return Err(UnifiedError::Unsupported(format!(
            "recovery_journal_record_version {version}"
        )));
    }

    let kind = record
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| UnifiedError::Corrupt("journal record type is missing".to_string()))?;
    if !RECORD_KINDS.contains(&kind) {
        return Err(UnifiedError::Unsupported(format!("record type {kind:?}")));
    }

    let payload = record
        .get("payload")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            UnifiedError::Corrupt("journal record payload is not an object".to_string())
        })?;

    // Sequence (fact E): starts at the manifest first_sequence and
    // increments by one per record.
    let sequence = record
        .get("sequence")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            UnifiedError::Corrupt("journal record sequence is not an integer".to_string())
        })?;
    if sequence != *expected_sequence {
        return Err(UnifiedError::Corrupt(format!(
            "journal record sequence {sequence} does not follow the expected {expected_sequence}"
        )));
    }
    *expected_sequence = expected_sequence.checked_add(1).ok_or_else(|| {
        UnifiedError::Corrupt(format!(
            "journal sequence {sequence} has no representable successor"
        ))
    })?;

    // Chain (fact E): null for the segment's first record, then the
    // previous record's digest.
    match previous_digest {
        None => {
            if !matches!(record.get("previous_record_sha256"), Some(Value::Null)) {
                return Err(UnifiedError::Corrupt(
                    "first journal record must chain to a null previous digest".to_string(),
                ));
            }
        }
        Some(previous) => {
            let chained = record
                .get("previous_record_sha256")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    UnifiedError::Corrupt(
                        "journal record previous_record_sha256 is missing".to_string(),
                    )
                })?;
            if chained != previous {
                return Err(UnifiedError::Corrupt(
                    "journal record does not chain to the previous digest".to_string(),
                ));
            }
        }
    }
    *previous_digest = Some(digest.to_string());

    // Watermark (fact F): required on projection records, validated and
    // advanced wherever it is present, never below the current one.
    match payload.get("watermark") {
        Some(watermark) => {
            let watermark = watermark.as_u64().ok_or_else(|| {
                UnifiedError::Corrupt(format!("{kind} record watermark is not an integer"))
            })?;
            if watermark < store.projection.watermark {
                return Err(UnifiedError::Corrupt(format!(
                    "{} record watermark {watermark} is below the current watermark {}",
                    kind, store.projection.watermark
                )));
            }
            store.projection.watermark = watermark;
        }
        None if PROJECTION_KINDS.contains(&kind) => {
            return Err(UnifiedError::Corrupt(format!(
                "{kind} record watermark is missing"
            )));
        }
        None => {}
    }

    // Dispatch (fact F).
    match kind {
        "projection_advanced" => {
            apply_advanced_snapshot(&mut store.projection, payload.get("snapshot"))?;
        }
        "projection_delta" => {
            apply_ops(&mut store.projection, delta_ops(payload)?)?;
        }
        "session_relocated" => {
            let metadata = payload
                .get("metadata")
                .and_then(Value::as_object)
                .ok_or_else(|| {
                    UnifiedError::Corrupt("session_relocated metadata is not an object".to_string())
                })?;
            store.runtime.cwd = metadata
                .get("cwd")
                .and_then(Value::as_str)
                .map(str::to_string);
            apply_ops(&mut store.projection, delta_ops(payload)?)?;
        }
        _ => {}
    }
    if PROJECTION_KINDS.contains(&kind) {
        super::store::validate_envelope(&store.projection.envelope, &store.runtime)?;
    }
    Ok(())
}

/// Computes the fact-E record digest: in the line without its newline,
/// find the last `,"record_sha256":"`; the 64 bytes after it are the
/// digest and must be followed by a quote; the SHA-256 of the line with
/// the entire field (including its leading comma) omitted must equal them.
fn record_digest(line: &[u8]) -> Result<String, UnifiedError> {
    let marker_start = line
        .windows(RECORD_SHA_MARKER.len())
        .rposition(|window| window == RECORD_SHA_MARKER)
        .ok_or_else(|| {
            UnifiedError::Corrupt("journal record has no record_sha256 marker".to_string())
        })?;
    let start = marker_start + RECORD_SHA_MARKER.len();
    if line.len() < start + 65 || line[start + 64] != b'"' {
        return Err(UnifiedError::Corrupt(
            "journal record_sha256 digest is malformed".to_string(),
        ));
    }
    let digest_bytes = &line[start..start + 64];
    let spliced: Vec<u8> = [&line[..marker_start], &line[start + 65..]].concat();
    let computed = sha256_hex(&spliced);
    if computed.as_bytes() != digest_bytes {
        return Err(UnifiedError::Corrupt(
            "journal record does not match its record_sha256 digest".to_string(),
        ));
    }
    Ok(computed)
}

/// Reads the op list of a projection record payload (fact F).
fn delta_ops(payload: &serde_json::Map<String, Value>) -> Result<&[Value], UnifiedError> {
    payload
        .get("delta")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| UnifiedError::Corrupt("projection record delta is not an array".to_string()))
}

/// Replaces the whole projection with a `projection_advanced` snapshot,
/// split into envelope and entries the same way the generation loader
/// splits the projection document.
fn apply_advanced_snapshot(
    state: &mut ProjectionState,
    snapshot: Option<&Value>,
) -> Result<(), UnifiedError> {
    let snapshot = snapshot.and_then(Value::as_object).ok_or_else(|| {
        UnifiedError::Corrupt("projection_advanced snapshot is not an object".to_string())
    })?;
    let format = snapshot
        .get("format")
        .and_then(Value::as_str)
        .ok_or_else(|| UnifiedError::Corrupt("snapshot format is missing".to_string()))?;
    if format != PROJECTION_FORMAT {
        return Err(UnifiedError::Unsupported(format!(
            "snapshot format {format}"
        )));
    }
    let entries = snapshot
        .get("history")
        .and_then(|history| history.get("entries"))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            UnifiedError::Corrupt("snapshot history entries are not an array".to_string())
        })?;
    let mut envelope = Value::Object(snapshot.clone());
    envelope["history"]["entries"] = Value::Array(Vec::new());
    state.envelope = envelope;
    state.entries = entries.clone();
    Ok(())
}

/// Applies one projection delta op list (pinned fact F):
/// `append_entry` pushes, `replace_entry` replaces the first entry with
/// that id, `remove_entry` removes all of them, `set_history_entries`
/// replaces the list and `set_envelope` replaces every snapshot field
/// except `history.entries`.  Unknown ops are unsupported; matching
/// failures are corrupt.
pub(crate) fn apply_ops(state: &mut ProjectionState, ops: &[Value]) -> Result<(), UnifiedError> {
    for op in ops {
        let op = op.as_object().ok_or_else(|| {
            UnifiedError::Corrupt("projection delta op is not an object".to_string())
        })?;
        match op.get("op").and_then(Value::as_str) {
            Some("append_entry") => {
                let entry = op_entry(op)?;
                state.entries.push(entry.clone());
            }
            Some("replace_entry") => {
                let id = op_id(op)?;
                let entry = op_entry(op)?;
                let position = state
                    .entries
                    .iter()
                    .position(|entry| entry_id(entry) == Some(id))
                    .ok_or_else(|| {
                        UnifiedError::Corrupt(format!("replace_entry found no entry {id:?}"))
                    })?;
                state.entries[position] = entry.clone();
            }
            Some("remove_entry") => {
                let id = op_id(op)?;
                let before = state.entries.len();
                state.entries.retain(|entry| entry_id(entry) != Some(id));
                if state.entries.len() == before {
                    return Err(UnifiedError::Corrupt(format!(
                        "remove_entry found no entry {id:?}"
                    )));
                }
            }
            Some("set_history_entries") => {
                let entries = op.get("entries").and_then(Value::as_array).ok_or_else(|| {
                    UnifiedError::Corrupt(
                        "set_history_entries entries are not an array".to_string(),
                    )
                })?;
                state.entries = entries.clone();
            }
            Some("set_envelope") => {
                let envelope_state =
                    op.get("state").and_then(Value::as_object).ok_or_else(|| {
                        UnifiedError::Corrupt("set_envelope state is not an object".to_string())
                    })?;
                // Fact F: the state replaces every snapshot field except
                // history.entries.  The replacement envelope is built
                // from the incoming state alone so fields it omits
                // disappear; its own history.entries is ignored and the
                // prior entries placeholder survives.
                if let Some(history) = envelope_state.get("history")
                    && !history.is_object()
                {
                    return Err(UnifiedError::Corrupt(
                        "set_envelope state history is not an object".to_string(),
                    ));
                }
                let preserved_entries = state
                    .envelope
                    .get("history")
                    .and_then(|history| history.get("entries"))
                    .cloned()
                    .unwrap_or_else(|| Value::Array(Vec::new()));
                let mut envelope = Value::Object(envelope_state.clone());
                envelope["history"]["entries"] = preserved_entries;
                state.envelope = envelope;
            }
            Some(other) => {
                return Err(UnifiedError::Unsupported(format!(
                    "projection delta op {other:?}"
                )));
            }
            None => {
                return Err(UnifiedError::Corrupt(
                    "projection delta op is missing its op field".to_string(),
                ));
            }
        }
    }
    Ok(())
}

/// Reads an entry's `id` field, the key replace/remove ops match on.
fn entry_id(entry: &Value) -> Option<&str> {
    entry.get("id").and_then(Value::as_str)
}

fn op_id(op: &serde_json::Map<String, Value>) -> Result<&str, UnifiedError> {
    op.get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| UnifiedError::Corrupt("projection delta op id is not a string".to_string()))
}

fn op_entry(op: &serde_json::Map<String, Value>) -> Result<&Value, UnifiedError> {
    op.get("entry")
        .filter(|entry| entry.is_object())
        .ok_or_else(|| UnifiedError::Corrupt("projection delta op entry is missing".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsers::mistral_vibe_unified::canonical::{canonical_json, sha256_hex};
    use crate::parsers::mistral_vibe_unified::load_stable;
    use crate::parsers::mistral_vibe_unified::store::load_generation;
    use crate::parsers::mistral_vibe_unified::store::test_support::{
        copy_fixture, rewrite_journal_record,
    };
    use serde_json::json;
    use std::cell::Cell;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    // Fixture IDs (Task 2 contract).
    const R302: &str = "aaaaaaaa-0000-4000-8000-000000000302";
    const R300: &str = "aaaaaaaa-0000-4000-8000-000000000300";
    const ROLLOVER: &str = "cccccccc-0000-4000-8000-000000000003";
    const GROWING: &str = "cccccccc-0000-4000-8000-000000000004";
    const UNKNOWN_OP: &str = "cccccccc-0000-4000-8000-000000000005";
    const UNKNOWN_RECORD: &str = "cccccccc-0000-4000-8000-000000000006";

    fn session_rel(id: &str) -> String {
        format!("vibe_sessions/unified/{id}")
    }

    /// Copies a session fixture into a temp directory, keeping the temp
    /// directory alive and returning the session directory copy.
    fn copy_store(id: &str) -> (TempDir, PathBuf) {
        let temp = copy_fixture(&session_rel(id));
        let session = temp.path().join(id);
        (temp, session)
    }

    /// Loads a generation and replays its journal in one step.
    fn replayed(session_dir: &Path) -> LoadedStore {
        let mut store = load_generation(session_dir).unwrap();
        replay_journal(session_dir, &mut store).unwrap();
        store
    }

    fn expect_variant(result: Result<(), UnifiedError>, expected: &'static str) {
        let error = result.expect_err("expected the replay to fail");
        assert_eq!(
            error_variant(&error),
            expected,
            "replay failed with the wrong variant: {error}"
        );
    }

    fn error_variant(error: &UnifiedError) -> &'static str {
        match error {
            UnifiedError::NoUserMessages => "NoUserMessages",
            UnifiedError::Ephemeral => "Ephemeral",
            UnifiedError::DeletingTree { .. } => "DeletingTree",
            UnifiedError::Unsupported(_) => "Unsupported",
            UnifiedError::Corrupt(_) => "Corrupt",
            UnifiedError::Incomplete(_) => "Incomplete",
            UnifiedError::IdentityConflict(_) => "IdentityConflict",
            UnifiedError::Unstable(_) => "Unstable",
            UnifiedError::MissingSavedResult(_) => "MissingSavedResult",
            UnifiedError::Io(_) => "Io",
        }
    }

    fn read_pointer(session_dir: &Path) -> Value {
        let raw = fs::read(session_dir.join("CURRENT")).unwrap();
        assert_eq!(raw.last(), Some(&b'\n'));
        serde_json::from_slice(&raw[..raw.len() - 1]).unwrap()
    }

    fn generation_dir(session_dir: &Path) -> PathBuf {
        session_dir
            .join("generations")
            .join(read_pointer(session_dir)["generation"].as_str().unwrap())
    }

    fn read_document(path: &Path) -> Value {
        let raw = fs::read(path).unwrap();
        assert_eq!(raw.last(), Some(&b'\n'));
        serde_json::from_slice(&raw[..raw.len() - 1]).unwrap()
    }

    fn write_stored(path: &Path, body: &[u8]) {
        let mut stored = body.to_vec();
        stored.push(b'\n');
        fs::write(path, stored).unwrap();
    }

    fn journal_path(session_dir: &Path) -> PathBuf {
        let manifest = read_document(&generation_dir(session_dir).join("manifest.json"));
        session_dir.join(
            manifest["recovery_journal_segment"]["path"]
                .as_str()
                .unwrap(),
        )
    }

    fn entry_ids(entries: &[Value]) -> Vec<&str> {
        entries
            .iter()
            .map(|entry| entry["id"].as_str().unwrap())
            .collect()
    }

    fn message(entry: &Value) -> (&str, &str) {
        (
            entry["id"].as_str().unwrap(),
            entry["content"][0]["text"].as_str().unwrap(),
        )
    }

    fn message_texts(store: &LoadedStore) -> Vec<(&str, &str)> {
        store
            .projection
            .entries
            .iter()
            .filter(|entry| entry["type"] == json!("message"))
            .map(message)
            .collect()
    }

    /// Builds a second coherent generation in a copied store: the same
    /// documents with the generation, `snapshot_sequence` and journal
    /// segment bumped, a fresh empty segment and the pointer bytes that
    /// publish it.  Flipping between the original pointer and this one
    /// switches between two fully valid store states (the stability
    /// tests must not publish a corrupt pointer/manifest pair).
    fn write_alternate_generation(session_dir: &Path) -> Vec<u8> {
        let pointer = read_pointer(session_dir);
        let generation_dir = session_dir
            .join("generations")
            .join(pointer["generation"].as_str().unwrap());
        let new_sequence = pointer["snapshot_sequence"].as_u64().unwrap() + 1;
        let new_generation = format!("{new_sequence:016}");
        let new_dir = session_dir.join("generations").join(&new_generation);
        fs::create_dir_all(&new_dir).unwrap();

        // Projection document: bumped sequence, digest follows.
        let mut projection = read_document(&generation_dir.join("projection-state.json"));
        projection["snapshot_sequence"] = json!(new_sequence);
        let projection_body = canonical_json(&projection).unwrap();
        write_stored(&new_dir.join("projection-state.json"), &projection_body);

        // Runtime document: bumped sequence, digest follows.
        let mut runtime = read_document(&generation_dir.join("runtime-state.json"));
        runtime["snapshot_sequence"] = json!(new_sequence);
        let runtime_body = canonical_json(&runtime).unwrap();
        write_stored(&new_dir.join("runtime-state.json"), &runtime_body);

        // Checkpoint document: copied verbatim.
        fs::copy(
            generation_dir.join("checkpoint.json"),
            new_dir.join("checkpoint.json"),
        )
        .unwrap();

        // Manifest: new generation identity, updated document digests and
        // a fresh empty journal segment.
        let first_sequence = new_sequence + 1;
        let segment = format!("journal/{first_sequence:016}.jsonl");
        let mut manifest = read_document(&generation_dir.join("manifest.json"));
        manifest["generation"] = json!(new_generation);
        manifest["snapshot_sequence"] = json!(new_sequence);
        manifest["projection_state"]["sha256"] = json!(sha256_hex(&projection_body));
        manifest["runtime_state"]["sha256"] = json!(sha256_hex(&runtime_body));
        manifest["recovery_journal_segment"] =
            json!({"first_sequence": first_sequence, "path": segment});
        let manifest_body = canonical_json(&manifest).unwrap();
        write_stored(&new_dir.join("manifest.json"), &manifest_body);

        // Empty journal segment for the new generation.
        fs::write(session_dir.join(&segment), b"").unwrap();

        // Pointer that publishes the new generation, newline included.
        let mut new_pointer = pointer.clone();
        new_pointer["generation"] = json!(new_generation);
        new_pointer["snapshot_sequence"] = json!(new_sequence);
        new_pointer["manifest_sha256"] = json!(sha256_hex(&manifest_body));
        let mut body = canonical_json(&new_pointer).unwrap();
        body.push(b'\n');
        body
    }

    #[test]
    fn upstream_writer_fixed_digest_vector() {
        // Pinned _storage.py writer: hash the header/payload BEFORE inserting
        // record_sha256. Fixed bytes deliberately do not use our splice helper.
        let line = r#"{"payload":{"text":"Résumé 🚀"},"previous_record_sha256":null,"record_sha256":"c958f277ea827a385e8ea3eaac80935f1d8f9973ce13190f086449ae4ff3e55e","recovery_journal_record_version":1,"sequence":1,"type":"core_input"}"#;
        assert_eq!(
            record_digest(line.as_bytes()).unwrap(),
            "c958f277ea827a385e8ea3eaac80935f1d8f9973ce13190f086449ae4ff3e55e"
        );
    }

    #[test]
    fn apply_ops_matches_upstream_semantics() {
        // replace_entry changes only the first match (fact F).
        let mut state = ProjectionState {
            envelope: json!({"session": {"id": "s"}, "history": {"entries": []}}),
            entries: vec![
                json!({"id": "x1", "text": "a"}),
                json!({"id": "y", "text": "b"}),
                json!({"id": "x1", "text": "c"}),
            ],
            watermark: 0,
        };
        apply_ops(
            &mut state,
            &[json!({"op": "replace_entry", "id": "x1", "entry": {"id": "x1", "text": "z"}})],
        )
        .unwrap();
        assert_eq!(entry_ids(&state.entries), ["x1", "y", "x1"]);
        assert_eq!(state.entries[0]["text"].as_str(), Some("z"));
        assert_eq!(state.entries[2]["text"].as_str(), Some("c"));

        // remove_entry removes every match.
        let mut state = ProjectionState {
            envelope: json!({"history": {"entries": []}}),
            entries: vec![
                json!({"id": "x1", "text": "a"}),
                json!({"id": "y", "text": "b"}),
                json!({"id": "x1", "text": "c"}),
            ],
            watermark: 0,
        };
        apply_ops(&mut state, &[json!({"op": "remove_entry", "id": "x1"})]).unwrap();
        assert_eq!(entry_ids(&state.entries), ["y"]);

        // Removing or replacing a missing id is corrupt.
        let mut state = ProjectionState {
            envelope: json!({"history": {"entries": []}}),
            entries: vec![json!({"id": "y", "text": "b"})],
            watermark: 0,
        };
        assert!(matches!(
            apply_ops(&mut state, &[json!({"op": "remove_entry", "id": "absent"})]),
            Err(UnifiedError::Corrupt(_))
        ));
        assert!(matches!(
            apply_ops(
                &mut state,
                &[json!({"op": "replace_entry", "id": "absent", "entry": {"id": "absent"}})]
            ),
            Err(UnifiedError::Corrupt(_))
        ));

        // set_envelope replaces every snapshot field except
        // history.entries; its own history.entries is ignored.
        let mut state = ProjectionState {
            envelope: json!({"session": {"id": "s"}, "history": {"entries": []}}),
            entries: vec![json!({"id": "y", "text": "b"})],
            watermark: 0,
        };
        apply_ops(
            &mut state,
            &[json!({
                "op": "set_envelope",
                "state": {
                    "session": {"id": "t"},
                    "history": {"entries": [{"id": "sneaky"}]},
                }
            })],
        )
        .unwrap();
        assert_eq!(entry_ids(&state.entries), ["y"]);
        assert_eq!(state.envelope["session"]["id"].as_str(), Some("t"));
        assert_eq!(
            state.envelope["history"]["entries"]
                .as_array()
                .unwrap()
                .len(),
            0
        );

        // set_history_entries replaces the list.
        apply_ops(
            &mut state,
            &[json!({"op": "set_history_entries", "entries": [{"id": "n1"}]})],
        )
        .unwrap();
        assert_eq!(entry_ids(&state.entries), ["n1"]);

        // Unknown op is unsupported.
        assert!(matches!(
            apply_ops(&mut state, &[json!({"op": "move_entry"})]),
            Err(UnifiedError::Unsupported(_))
        ));
    }

    #[test]
    fn set_envelope_replaces_the_whole_envelope() {
        // Fact F: set_envelope replaces every snapshot field except
        // history.entries, so fields the incoming state omits disappear
        // instead of surviving from the old envelope (review finding:
        // the op is a replacement, not a merge).
        let mut state = ProjectionState {
            envelope: json!({
                "format": "harness.public-session-state/v1",
                "session": {"id": "s", "title": "Old title"},
                "latestTurn": {"id": "t1", "status": "in_progress"},
                "history": {"cursor": 7, "entries": []},
            }),
            entries: vec![json!({"id": "y", "text": "b"})],
            watermark: 0,
        };
        apply_ops(
            &mut state,
            &[json!({
                "op": "set_envelope",
                "state": {
                    "session": {"id": "t"},
                    "history": {"cursor": 9, "entries": [{"id": "sneaky"}]},
                }
            })],
        )
        .unwrap();
        // The envelope is the incoming state, not a merge: fields the
        // state omits (format, latestTurn) are gone.
        assert!(state.envelope.get("format").is_none());
        assert!(state.envelope.get("latestTurn").is_none());
        // history is replaced too, except its entries: the cursor moved
        // to the incoming value and the state's own entries were ignored
        // in favor of the prior placeholder.
        assert_eq!(state.envelope["history"]["cursor"].as_u64(), Some(9));
        assert_eq!(
            state.envelope["history"]["entries"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert_eq!(state.envelope["session"]["id"].as_str(), Some("t"));
        // The current history entries are kept.
        assert_eq!(entry_ids(&state.entries), ["y"]);
    }

    #[test]
    fn replays_r302_journal_and_ignores_unterminated_tail() {
        let store = replayed(&crate::fixture_path(&session_rel(R302)));

        assert_eq!(
            entry_ids(&store.projection.entries),
            [
                "e1",
                "e2",
                "effect-act-read",
                "e4",
                "e5",
                "e6",
                "e7",
                "effect-act-patch",
                "effect-act-bash",
                "effect-act-spawn-explicit",
                "effect-act-spawn-derived",
                "effect-act-spawn-missing",
                "e13",
                "e14",
                "e15",
                "e16",
                "e17",
                "e18",
                "e18b",
                "effect-act-gh",
                "assistant-act-c",
                "e20b",
                "effect-act-write",
                "effect-act-todo",
            ]
        );
        // The removed reasoning entry is gone.
        assert!(
            !store
                .projection
                .entries
                .iter()
                .any(|entry| entry["id"] == json!("reasoning-act-c-0"))
        );
        // In-place replacement: the act-gh effect completed and the
        // assistant entry settled.
        let act_gh = store
            .projection
            .entries
            .iter()
            .find(|entry| entry["id"] == json!("effect-act-gh"))
            .unwrap();
        assert_eq!(act_gh["state"]["status"].as_str(), Some("completed"));
        let assistant = store
            .projection
            .entries
            .iter()
            .find(|entry| entry["id"] == json!("assistant-act-c"))
            .unwrap();
        assert_eq!(
            assistant["content"][0]["text"].as_str(),
            Some("Doc ajoutée.")
        );

        // session_relocated applied the envelope and the new cwd.
        assert_eq!(
            store.projection.envelope["latestTurn"]["status"].as_str(),
            Some("completed")
        );
        assert_eq!(
            store.projection.envelope["session"]["status"]["type"].as_str(),
            Some("idle")
        );
        assert_eq!(
            store.runtime.cwd.as_deref(),
            Some("/home/fixture/projects/moved")
        );
        assert_eq!(store.projection.watermark, 9);
    }

    #[test]
    fn replay_is_idempotent() {
        let fixture = crate::fixture_path(&session_rel(R302));
        let first = replayed(&fixture);
        let second = replayed(&fixture);

        assert_eq!(first.projection.envelope, second.projection.envelope);
        assert_eq!(first.projection.entries, second.projection.entries);
        assert_eq!(first.projection.watermark, second.projection.watermark);
        assert_eq!(first.runtime.cwd, second.runtime.cwd);
    }

    #[test]
    fn r300_duplicate_ids_follow_first_match_replace() {
        let store = replayed(&crate::fixture_path(&session_rel(R300)));

        assert_eq!(
            message_texts(&store),
            [
                ("u1", "Première question (éditée)"),
                ("a1", "Réponse"),
                ("u1", "Première question"),
            ]
        );
    }

    #[test]
    fn replay_rejects_non_record_line() {
        let (_temp, session) = copy_store(R302);
        let path = journal_path(&session);
        let mut data = fs::read(&path).unwrap();
        data.extend_from_slice(b"{\"not\":\"a record\"}\n");
        fs::write(&path, data).unwrap();

        let mut store = load_generation(&session).unwrap();
        expect_variant(replay_journal(&session, &mut store), "Corrupt");
    }

    #[test]
    fn replay_rejects_edited_record_payload() {
        let (_temp, session) = copy_store(R302);
        let path = journal_path(&session);
        let mut data = fs::read(&path).unwrap();
        // Flip one payload byte inside record 7 (the appended entry id)
        // without recomputing the digest: "e18b" becomes "e18c".
        let marker = b"\"id\":\"e18b\"";
        let position = data
            .windows(marker.len())
            .position(|window| window == marker)
            .unwrap();
        assert_eq!(data[position + 9], b'b');
        data[position + 9] = b'c';
        fs::write(&path, data).unwrap();

        let mut store = load_generation(&session).unwrap();
        expect_variant(replay_journal(&session, &mut store), "Corrupt");
    }

    #[test]
    fn replay_rejects_swapped_records() {
        let (_temp, session) = copy_store(R302);
        let path = journal_path(&session);
        let data = fs::read(&path).unwrap();
        // Split into complete lines plus the unterminated tail, then swap
        // the records with sequence 6 and 7.
        let mut lines: Vec<Vec<u8>> = Vec::new();
        let mut rest = data.as_slice();
        while let Some(position) = rest.iter().position(|&byte| byte == b'\n') {
            let (line, after) = rest.split_at(position);
            lines.push(line.to_vec());
            rest = &after[1..];
        }
        lines.swap(1, 2);
        let mut output = Vec::new();
        for line in &lines {
            output.extend_from_slice(line);
            output.push(b'\n');
        }
        output.extend_from_slice(rest); // Unterminated tail is preserved.
        fs::write(&path, output).unwrap();

        let mut store = load_generation(&session).unwrap();
        expect_variant(replay_journal(&session, &mut store), "Corrupt");
    }

    #[test]
    fn replay_rejects_unknown_record_type() {
        let temp = copy_fixture(&format!(
            "vibe_unified_variants/unknown-record/unified/{UNKNOWN_RECORD}"
        ));
        let session = temp.path().join(UNKNOWN_RECORD);

        let mut store = load_generation(&session).unwrap();
        expect_variant(replay_journal(&session, &mut store), "Unsupported");
    }

    #[test]
    fn replay_rejects_unknown_op() {
        let temp = copy_fixture(&format!(
            "vibe_unified_variants/unknown-op/unified/{UNKNOWN_OP}"
        ));
        let session = temp.path().join(UNKNOWN_OP);

        let mut store = load_generation(&session).unwrap();
        expect_variant(replay_journal(&session, &mut store), "Unsupported");
    }

    #[test]
    fn replay_rejects_decreasing_watermark() {
        // The last complete record (action_result, sequence 10) gains a
        // stale watermark; its digest is recomputed so only the watermark
        // rule can reject it.
        let (_temp, session) = copy_store(R302);
        rewrite_journal_record(&session, 10, |record| {
            record["payload"]["watermark"] = json!(3);
        });
        let mut store = load_generation(&session).unwrap();
        expect_variant(replay_journal(&session, &mut store), "Corrupt");

        // The last projection record (sequence 9) is rejected the same
        // way when its watermark moves backwards.
        let (_temp, session) = copy_store(R302);
        rewrite_journal_record(&session, 9, |record| {
            record["payload"]["watermark"] = json!(8);
        });
        let mut store = load_generation(&session).unwrap();
        expect_variant(replay_journal(&session, &mut store), "Corrupt");
    }

    #[test]
    fn journal_growth_after_sample_is_ignored() {
        let temp = copy_fixture(&format!("vibe_unified_variants/growing/unified/{GROWING}"));
        let session = temp.path().join(GROWING);
        let mut store = load_generation(&session).unwrap();
        let sampled_size = store.journal.size;

        // The store grows after the sample: the next record is appended.
        let append = fs::read(crate::fixture_path(
            "vibe_unified_variants/growing/append.jsonl",
        ))
        .unwrap();
        assert_eq!(append.last(), Some(&b'\n'));
        let path = session.join(&store.journal.relative_path);
        let mut data = fs::read(&path).unwrap();
        data.extend_from_slice(&append);
        fs::write(&path, data).unwrap();

        // The bounded replay publishes the sampled prefix only, and the
        // recorded sample stays the pre-replay one.
        replay_journal(&session, &mut store).unwrap();
        assert_eq!(
            message_texts(&store),
            [("u1", "Growing question"), ("a1", "Growing answer")]
        );
        assert_eq!(store.journal.size, sampled_size);

        // A fresh load samples the grown journal and sees the record.
        let ((), fresh) = load_stable(&session, |_| Ok(())).unwrap();
        assert_eq!(
            message_texts(&fresh),
            [
                ("u1", "Growing question"),
                ("a1", "Growing answer"),
                ("a2", "Late answer"),
            ]
        );
        assert!(fresh.journal.size > sampled_size);
    }

    #[test]
    fn rollover_generations_replay_to_same_history() {
        let base = "vibe_unified_variants/rollover";
        let gen1 = replayed(&crate::fixture_path(&format!(
            "{base}/gen1/unified/{ROLLOVER}"
        )));
        let gen2 = replayed(&crate::fixture_path(&format!(
            "{base}/gen2/unified/{ROLLOVER}"
        )));

        assert_eq!(gen1.projection.entries, gen2.projection.entries);
        assert_eq!(
            entry_ids(&gen1.projection.entries),
            ["u1", "a1", "u2", "a2"]
        );
    }

    #[test]
    fn unstable_replay_errors_retry_before_classification() {
        for during in [false, true] {
            let (_temp, session) = copy_store(R302);
            let calls = Cell::new(0);
            let result = super::super::load_stable_with_replay(
                &session,
                |_| Ok(()),
                |path, store| {
                    calls.set(calls.get() + 1);
                    let journal = path.join(&store.journal.relative_path);
                    let replace = || {
                        // Valid upstream record, wrong sequence for R302. Replace
                        // the inode, not just the bytes: demonstrable instability.
                        let source = crate::fixture_path(&session_rel(R300))
                            .join("journal/0000000000000002.jsonl");
                        let sibling = journal.with_extension("replacement");
                        fs::copy(&source, &sibling).unwrap();
                        if during {
                            // Repopulate the already opened inode too, so replay
                            // fails on a valid record from a different segment.
                            fs::write(&journal, fs::read(&source).unwrap()).unwrap();
                        }
                        fs::rename(sibling, &journal).unwrap();
                    };
                    if during {
                        // The replay has opened its handle before replacement.
                        replay_journal_with_open_hook(path, store, replace)
                    } else {
                        replace();
                        replay_journal(path, store)
                    }
                },
            );
            assert!(
                matches!(result, Err(UnifiedError::Unstable(_))),
                "during={during}: {result:?}"
            );
            assert_eq!(calls.get(), 2);
        }
    }

    #[test]
    fn current_symlink_is_rejected_by_load_stability_and_freshness() {
        use std::os::unix::fs::symlink;
        let (_temp, session) = copy_store(R302);
        let store = load_generation(&session).unwrap();
        let signature = super::super::UnifiedSignature::from_parse(&store, vec![]);
        let outside = session.parent().unwrap().join("outside-current");
        fs::write(&outside, &store.current_bytes).unwrap();
        fs::remove_file(session.join("CURRENT")).unwrap();
        symlink(outside, session.join("CURRENT")).unwrap();
        let rejected = matches!(load_generation(&session), Err(UnifiedError::Corrupt(_)));
        assert!(
            rejected
                && super::super::read_is_stable(&session, &store).is_err()
                && !signature.is_current(&session),
            "CURRENT symlink escaped load/stability/freshness boundary"
        );
    }

    #[test]
    fn stable_corruption_is_not_retried() {
        let (_temp, session) = copy_store(R302);
        rewrite_journal_record(&session, 10, |record| {
            record["sequence"] = json!(99);
        });
        let calls = Cell::new(0);
        let result = super::super::load_stable_with_replay(
            &session,
            |_| Ok(()),
            |path, store| {
                calls.set(calls.get() + 1);
                replay_journal(path, store)
            },
        );
        assert!(matches!(result, Err(UnifiedError::Corrupt(_))));
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn removal_outcomes_require_a_stable_pointer() {
        for kind in ["empty", "ephemeral", "deleting"] {
            let (_temp, session) = copy_store(R302);
            let pointer_a = fs::read(session.join("CURRENT")).unwrap();
            let pointer_b = write_alternate_generation(&session);
            let calls = Cell::new(0);
            let result: Result<((), LoadedStore), _> = load_stable(&session, |_| {
                calls.set(calls.get() + 1);
                fs::write(
                    session.join("CURRENT"),
                    if calls.get() % 2 == 1 {
                        &pointer_b
                    } else {
                        &pointer_a
                    },
                )
                .unwrap();
                Err(match kind {
                    "empty" => UnifiedError::NoUserMessages,
                    "ephemeral" => UnifiedError::Ephemeral,
                    _ => UnifiedError::DeletingTree {
                        child_session_ids: vec![],
                    },
                })
            });
            assert!(
                matches!(result, Err(UnifiedError::Unstable(_))),
                "{kind}: {result:?}"
            );
            assert_eq!(calls.get(), 2);
        }
    }

    #[test]
    fn load_stable_retries_once_then_reports_unstable() {
        // A pointer that flips to another coherent generation on every
        // build call never stabilizes.
        let (_temp, session) = copy_store(R302);
        let pointer_a = fs::read(session.join("CURRENT")).unwrap();
        let pointer_b = write_alternate_generation(&session);
        let calls = Cell::new(0u32);
        let result = load_stable(&session, |_store| {
            let count = calls.get() + 1;
            calls.set(count);
            let target = if count % 2 == 1 {
                &pointer_b
            } else {
                &pointer_a
            };
            fs::write(session.join("CURRENT"), target).unwrap();
            Ok(())
        });
        let error = result.expect_err("expected the store to stay unstable");
        assert!(
            matches!(error, UnifiedError::Unstable(_)),
            "unexpected error: {error}"
        );
        assert_eq!(calls.get(), 2);

        // A single publication between the load and the check retries
        // once and succeeds on the new coherent generation.
        let (_temp, session) = copy_store(R302);
        let pointer_b = write_alternate_generation(&session);
        let calls = Cell::new(0u32);
        let ((), store) = load_stable(&session, |_store| {
            let count = calls.get() + 1;
            calls.set(count);
            if count == 1 {
                fs::write(session.join("CURRENT"), &pointer_b).unwrap();
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(calls.get(), 2);
        assert_eq!(store.journal_first_sequence, 6);
        assert_eq!(
            store.journal.relative_path,
            "journal/0000000000000006.jsonl"
        );
        assert_eq!(store.projection.entries.len(), 17);
    }

    #[test]
    fn load_stable_retries_when_publication_removes_files() {
        // The first build call sees referenced files vanish (Incomplete)
        // while the pointer flips to a coherent generation; the retry
        // loads that generation and succeeds.
        let (_temp, session) = copy_store(R302);
        let pointer_b = write_alternate_generation(&session);
        let calls = Cell::new(0u32);
        let ((), store) = load_stable(&session, |_store| {
            let count = calls.get() + 1;
            calls.set(count);
            if count == 1 {
                fs::write(session.join("CURRENT"), &pointer_b).unwrap();
                return Err(UnifiedError::Incomplete("chunk is missing".to_string()));
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(calls.get(), 2);
        assert_eq!(store.journal_first_sequence, 6);
    }

    #[test]
    fn load_stable_reports_incomplete_without_pointer_change() {
        // A chunk file goes missing while the pointer never moves: the
        // honest Incomplete error is reported, not retried.
        let (_temp, session) = copy_store(R302);
        let manifest = read_document(&generation_dir(&session).join("manifest.json"));
        let digest = manifest["projection_state"]["chunks"][0].as_str().unwrap();
        fs::remove_file(session.join("chunks").join(format!("{digest}.json"))).unwrap();

        let calls = Cell::new(0u32);
        let result = load_stable(&session, |_store| {
            calls.set(calls.get() + 1);
            Ok(())
        });
        assert!(
            matches!(result, Err(UnifiedError::Incomplete(_))),
            "unexpected error: {result:?}"
        );
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn load_stable_reports_unstable_when_journal_is_replaced() {
        let (_temp, session) = copy_store(R302);
        let journal_path = journal_path(&session);
        let calls = Cell::new(0u32);
        let result = load_stable(&session, |_store| {
            // Replace the segment with a fresh inode holding the same
            // bytes, like a rollover rewriting the segment mid-read.
            let count = calls.get() + 1;
            calls.set(count);
            let data = fs::read(&journal_path).unwrap();
            let sibling = journal_path.with_extension("tmp");
            fs::write(&sibling, data).unwrap();
            fs::rename(&sibling, &journal_path).unwrap();
            Ok(())
        });
        assert!(
            matches!(result, Err(UnifiedError::Unstable(_))),
            "unexpected error: {result:?}"
        );
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn load_stable_retries_when_journal_shrinks() {
        let (_temp, session) = copy_store(R302);
        let journal_path = journal_path(&session);
        let calls = Cell::new(0u32);
        let ((), store) = load_stable(&session, |_store| {
            let count = calls.get() + 1;
            calls.set(count);
            if count == 1 {
                // Compaction: keep only the first complete record.
                let data = fs::read(&journal_path).unwrap();
                let end = data.iter().position(|&byte| byte == b'\n').unwrap() + 1;
                fs::write(&journal_path, &data[..end]).unwrap();
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(calls.get(), 2);
        // The retry published the sampled prefix of the shrunken journal.
        assert_eq!(entry_ids(&store.projection.entries).last(), Some(&"e18"));
        assert_eq!(store.projection.watermark, 5);
    }
}
