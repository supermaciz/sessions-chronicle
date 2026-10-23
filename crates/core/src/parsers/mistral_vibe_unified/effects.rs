//! Effect mapping for Mistral Vibe Unified projections.
//!
//! Effect history entries (fact H) become tool-call rows with their
//! per-status output and error texts, subagent effects become subagent
//! rows, large results referenced by a receipt (fact K) are resolved from
//! the store's local `tool-results/` records, and skill payload blocks
//! (fact I) become settled `skill` tool calls placed right after their
//! user message.

use std::collections::HashSet;

use serde_json::Value;

use super::convert::{Transcript, closed_variant, required_string, text_blocks_joined};
use super::error::UnifiedError;
use super::saved_results::SavedResultResolver;
use super::store::child_session_id;
use crate::models::{Subagent, ToolCall, ToolCallStatus, TranscriptItem, TranscriptItemKind};

/// Maps effect history entries and skill payload blocks into tool-call
/// and subagent rows on the shared [`Transcript`] accumulator.
pub(crate) struct EffectSink<'a> {
    session_id: &'a str,
    results: &'a mut SavedResultResolver,
    used_tool_call_ids: HashSet<String>,
}

impl<'a> EffectSink<'a> {
    /// Creates the sink for one conversion pass; `results` resolves the
    /// receipt-referenced saved records of the session's own store.
    pub(crate) fn new(session_id: &'a str, results: &'a mut SavedResultResolver) -> Self {
        Self {
            session_id,
            results,
            used_tool_call_ids: HashSet::new(),
        }
    }

    /// Maps one `effect` history entry (fact H) to a tool-call or
    /// subagent row plus its transcript item.
    pub(crate) fn push_effect(
        &mut self,
        entry: &Value,
        out: &mut Transcript,
    ) -> Result<(), UnifiedError> {
        let entry_id = entry
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| UnifiedError::Corrupt("effect entry id is missing".to_string()))?;
        let action_id = entry_id.strip_prefix("effect-").ok_or_else(|| {
            UnifiedError::Corrupt(format!(
                "effect entry id {entry_id:?} is missing the \"effect-\" prefix"
            ))
        })?;
        let turn_id = entry.get("turnId").and_then(Value::as_str);
        let detail = entry
            .get("detail")
            .ok_or_else(|| UnifiedError::Corrupt("effect detail is missing".to_string()))?;
        let (status, error_text) = effect_status(entry)?;
        let kind = closed_variant(
            detail.get("kind"),
            "effect detail.kind",
            &["tool", "subagent"],
        )?;

        // Subagent effects (fact H): one subagent row plus its item; the
        // child link is resolved by the indexer, never here.
        if kind == "subagent" {
            let subagent = Subagent {
                id: format!("{}-{entry_id}", self.session_id),
                agent_id: Some(
                    detail
                        .get("childSessionId")
                        .and_then(Value::as_str)
                        .filter(|child| !child.is_empty())
                        .map(str::to_string)
                        .unwrap_or_else(|| child_session_id(self.session_id, action_id)),
                ),
                agent_name: detail
                    .get("agentName")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                session_id: self.session_id.to_string(),
                title: detail
                    .get("agentName")
                    .and_then(Value::as_str)
                    .filter(|name| !name.is_empty())
                    .unwrap_or("Subagent")
                    .to_string(),
                prompt: detail
                    .pointer("/input/task")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                result_summary: subagent_result_summary(entry),
                child_session_id: None,
                parser_ref: Some(action_id.to_string()),
            };
            let id = subagent.id.clone();
            out.subagents.push(subagent);
            self.push_row_item(out, TranscriptItemKind::Subagent, None, Some(id), turn_id);
            return Ok(());
        }

        let tool_name = detail
            .get("toolName")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                UnifiedError::Corrupt("effect detail toolName is missing".to_string())
            })?;
        let output_text = self.output_text(entry)?;
        let created_at = entry.get("createdAt").and_then(Value::as_i64);
        let updated_at = entry.get("updatedAt").and_then(Value::as_i64);
        // A running or pending effect has not ended: no end timestamp,
        // no duration.
        let (ended_at, duration_ms) =
            if matches!(status, ToolCallStatus::Running | ToolCallStatus::Pending) {
                (None, None)
            } else {
                (
                    updated_at.map(|millis| millis / 1000),
                    match (created_at, updated_at) {
                        (Some(created), Some(updated)) => updated.checked_sub(created),
                        _ => None,
                    },
                )
            };
        let id = self.unique_tool_call_id(&format!("{}-{entry_id}", self.session_id));
        out.tool_calls.push(ToolCall {
            id: id.clone(),
            session_id: self.session_id.to_string(),
            subagent_id: None,
            tool_name: tool_name.to_string(),
            status,
            title: entry
                .get("title")
                .and_then(Value::as_str)
                .map(str::to_string),
            summary: detail
                .pointer("/display/summary")
                .and_then(Value::as_str)
                .map(str::to_string),
            input_json: match detail.get("input") {
                None | Some(Value::Null) => None,
                Some(input) => serde_json::to_string(input).map(Some).map_err(|error| {
                    UnifiedError::Corrupt(format!("effect input is not serializable: {error}"))
                })?,
            },
            output_text,
            error_text,
            started_at: created_at.map(|millis| millis / 1000),
            ended_at,
            duration_ms,
            parser_call_id: Some(action_id.to_string()),
        });
        self.push_row_item(out, TranscriptItemKind::ToolCall, Some(id), None, turn_id);
        Ok(())
    }

    /// Maps one skill payload block (fact I) of a user message to a
    /// settled `skill` tool call placed right after the message item.
    pub(crate) fn push_skill(
        &mut self,
        message_entry: &Value,
        name: &str,
        text: &str,
        out: &mut Transcript,
    ) {
        let message_id = message_entry
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let id = self.unique_tool_call_id(&format!(
            "{}-{message_id}:invoked-skill:{name}",
            self.session_id
        ));
        out.tool_calls.push(ToolCall {
            id: id.clone(),
            session_id: self.session_id.to_string(),
            subagent_id: None,
            tool_name: "skill".to_string(),
            status: ToolCallStatus::Completed,
            title: None,
            summary: None,
            input_json: Some(serde_json::json!({ "name": name }).to_string()),
            output_text: Some(text.to_string()),
            error_text: None,
            started_at: None,
            ended_at: None,
            duration_ms: None,
            parser_call_id: None,
        });
        let turn_id = message_entry.get("turnId").and_then(Value::as_str);
        self.push_row_item(out, TranscriptItemKind::ToolCall, Some(id), None, turn_id);
    }

    /// Appends the transcript item of a freshly pushed row, attaching
    /// same-turn pending reasoning through [`Transcript::push_item`].
    fn push_row_item(
        &self,
        out: &mut Transcript,
        kind: TranscriptItemKind,
        tool_call_id: Option<String>,
        subagent_id: Option<String>,
        turn_id: Option<&str>,
    ) {
        let item_index = out.transcript_items.len() as i64;
        out.push_item(
            TranscriptItem {
                session_id: self.session_id.to_string(),
                item_index,
                kind,
                message_index: None,
                tool_call_id,
                subagent_id,
                context_id: None,
            },
            turn_id,
        );
    }

    /// Tool-call ids: a repeated id gets `-dup{n}`.
    fn unique_tool_call_id(&mut self, base: &str) -> String {
        if self.used_tool_call_ids.insert(base.to_string()) {
            return base.to_string();
        }
        let mut repeat = 1;
        loop {
            let candidate = format!("{base}-dup{repeat}");
            if self.used_tool_call_ids.insert(candidate.clone()) {
                return candidate;
            }
            repeat += 1;
        }
    }

    /// Output text (pinned rule): a receipt (fact K) resolves to the
    /// saved local record; otherwise `outputText` when non-empty, else
    /// pretty-printed `state.output`, else nothing.
    fn output_text(&mut self, entry: &Value) -> Result<Option<String>, UnifiedError> {
        if let Some(blocks) = entry
            .pointer("/state/output/content")
            .and_then(Value::as_array)
        {
            let text = text_blocks_joined(blocks);
            if let Some(saved) = self.results.resolve(&text)? {
                return Ok(Some(saved));
            }
        }
        if let Some(output_text) = entry
            .pointer("/state/outputText")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
        {
            return Ok(Some(output_text.to_string()));
        }
        match entry.pointer("/state/output") {
            None | Some(Value::Null) => Ok(None),
            Some(output) => serde_json::to_string_pretty(output)
                .map(Some)
                .map_err(|error| {
                    UnifiedError::Corrupt(format!("effect output is not serializable: {error}"))
                }),
        }
    }
}

/// Effect status (pinned rule): running/pending stay unfinished,
/// completed settles, failed/cancelled/skipped carry an error text, and
/// any other status is unsupported.
fn effect_status(entry: &Value) -> Result<(ToolCallStatus, Option<String>), UnifiedError> {
    let status = entry
        .pointer("/state/status")
        .and_then(Value::as_str)
        .ok_or_else(|| UnifiedError::Corrupt("effect state status is missing".to_string()))?;
    match status {
        "running" => Ok((ToolCallStatus::Running, None)),
        "completed" => Ok((ToolCallStatus::Completed, None)),
        "failed" => Ok((
            ToolCallStatus::Error,
            Some(
                required_string(
                    entry.pointer("/state/error/message"),
                    "effect error.message",
                )?
                .to_string(),
            ),
        )),
        "cancelled" | "skipped" => Ok((
            ToolCallStatus::Error,
            Some(required_string(entry.pointer("/state/reason"), "effect reason")?.to_string()),
        )),
        other => Err(UnifiedError::Unsupported(format!(
            "effect status {other:?}"
        ))),
    }
}

/// Subagent result summary (pinned rule): non-empty `outputText`, else
/// `error.message`.
fn subagent_result_summary(entry: &Value) -> Option<String> {
    entry
        .pointer("/state/outputText")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .or_else(|| {
            entry
                .pointer("/state/error/message")
                .and_then(Value::as_str)
        })
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ToolCallStatus, TranscriptItem};
    use crate::parsers::ParsedSession;
    use crate::parsers::mistral_vibe_unified::convert::convert;
    use crate::parsers::mistral_vibe_unified::store::{
        FileSample, Identity, LoadedStore, ProjectionState, RuntimeView,
        test_support::{copy_fixture, rewrite_document},
    };
    use crate::parsers::mistral_vibe_unified::{
        MistralVibeUnifiedParser, UnifiedParse, child_session_id,
    };
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    // Fixture IDs (Task 2 contract; child IDs re-derived with rule J).
    const R302: &str = "aaaaaaaa-0000-4000-8000-000000000302";
    const R300: &str = "aaaaaaaa-0000-4000-8000-000000000300";
    const CHILD_A: &str = "child-ea9b00b3ad4061f4ca36fd81";
    const V_UNKNOWN_STATUS: &str = "cccccccc-0000-4000-8000-000000000009";
    const T0_MS: i64 = 1_792_700_000_000;
    const IN_MEMORY: &str = "in-memory";

    fn unified_rel(id: &str) -> String {
        format!("vibe_sessions/unified/{id}")
    }

    fn variant_rel(name: &str, id: &str) -> String {
        format!("vibe_unified_variants/{name}/unified/{id}")
    }

    fn parse_fixture(rel: &str) -> Result<UnifiedParse, UnifiedError> {
        MistralVibeUnifiedParser.parse(&crate::fixture_path(rel))
    }

    /// Copies a Unified fixture store into a temp directory, keeping the
    /// session directory name the store's identity requires.
    fn copy_unified(id: &str) -> (TempDir, PathBuf) {
        let temp = copy_fixture(&unified_rel(id));
        let session = temp.path().join(id);
        (temp, session)
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

    /// Receipt text shaped exactly like the upstream adapter's (fact K).
    fn receipt(path: &str, size: usize) -> String {
        format!(
            "<system>The tool output is large. To preserve your context window, it was saved to the file system under {path}.\nOutput size: {size} characters\nMax size before saving: 16000 characters</system>\n<truncated-output>\nThe full output is available in the saved file.\n</truncated-output>"
        )
    }

    // ------------------------------------------------------------------
    // In-memory conversion harness: effect entries built as JSON values
    // over a synthetic root store whose session directory is never
    // touched on disk.
    // ------------------------------------------------------------------

    fn in_memory_session_dir() -> PathBuf {
        PathBuf::from("/home/fixture/.vibe/logs/session/unified").join(IN_MEMORY)
    }

    fn in_memory_store(entries: Vec<Value>) -> LoadedStore {
        LoadedStore {
            current_bytes: b"{}\n".to_vec(),
            journal_first_sequence: 2,
            meta: None,
            journal: FileSample {
                relative_path: "journal/0000000000000002.jsonl".to_string(),
                size: 0,
                mtime_ns: 0,
                dev: 0,
                ino: 0,
            },
            runtime: RuntimeView {
                session_id: IN_MEMORY.to_string(),
                ephemeral: false,
                identity: Identity::Root,
                cwd: Some("/home/fixture/projects/original".to_string()),
                active_model: Some("mistral-large-4".to_string()),
                deleting_children: None,
            },
            projection: ProjectionState {
                envelope: json!({
                    "format": "harness.public-session-state/v1",
                    "session": {
                        "id": IN_MEMORY,
                        "createdAt": T0_MS,
                        "updatedAt": T0_MS,
                        "status": {"type": "idle"},
                    },
                    "latestTurn": null,
                    "history": {"entries": []},
                }),
                entries,
                watermark: 1,
            },
        }
    }

    fn convert_entries(entries: Vec<Value>) -> Result<ParsedSession, UnifiedError> {
        let store = in_memory_store(entries);
        let session_dir = in_memory_session_dir();
        let mut results = SavedResultResolver::new(session_dir.join("tool-results"));
        let mut effects = EffectSink::new(IN_MEMORY, &mut results);
        convert(&session_dir, &store, &mut effects)
    }

    fn in_memory_user_message() -> Value {
        json!({
            "type": "message", "id": "u1", "turnId": "t1",
            "createdAt": T0_MS, "updatedAt": T0_MS, "generationStatus": "completed",
            "role": "user", "source": "turn_start",
            "content": [ {"type": "text", "text": "Do things"} ],
        })
    }

    fn tool_effect(id: &str, status: &str, state_fields: Value) -> Value {
        let mut state = json!({"status": status});
        if let (Some(target), Some(fields)) = (state.as_object_mut(), state_fields.as_object()) {
            for (key, value) in fields {
                target.insert(key.clone(), value.clone());
            }
        }
        json!({
            "type": "effect", "id": format!("effect-{id}"), "turnId": "t1",
            "createdAt": T0_MS, "updatedAt": T0_MS, "generationStatus": "completed",
            "title": format!("Effect {id}"),
            "detail": {
                "kind": "tool",
                "toolName": "file_system.bash",
                "input": {"command": "ls"},
                "display": {"summary": "file_system.bash"},
            },
            "state": state,
        })
    }

    // ------------------------------------------------------------------
    // R302: tool calls, statuses, outputs, subagents (brief steps 1a-1c)
    // ------------------------------------------------------------------

    #[test]
    fn r302_tool_calls_in_order() {
        let parsed = parse_fixture(&unified_rel(R302)).unwrap().parsed;

        let names: Vec<&str> = parsed
            .tool_calls
            .iter()
            .map(|call| call.tool_name.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "skill",
                "file_system.read_file",
                "file_system.search_replace",
                "file_system.bash",
                "connector_github_app.search_code",
                "file_system.write_file",
                "vibe.todo",
            ]
        );
        let statuses: Vec<ToolCallStatus> =
            parsed.tool_calls.iter().map(|call| call.status).collect();
        assert_eq!(
            statuses,
            [
                ToolCallStatus::Completed, // skill payload call
                ToolCallStatus::Completed, // act-read
                ToolCallStatus::Error,     // act-patch (failed)
                ToolCallStatus::Completed, // act-bash
                ToolCallStatus::Completed, // act-gh
                ToolCallStatus::Error,     // act-write (cancelled)
                ToolCallStatus::Error,     // act-todo (skipped)
            ]
        );

        assert_eq!(
            parsed.tool_calls[2].error_text.as_deref(),
            Some("patch did not apply")
        );
        assert_eq!(
            parsed.tool_calls[5].error_text.as_deref(),
            Some("Interrupted")
        );
        assert_eq!(
            parsed.tool_calls[6].error_text.as_deref(),
            Some("Hook denied")
        );

        // The receipt of act-bash resolves to the saved record; the
        // truncated receipt text itself never reaches the row.
        let bash_output = parsed.tool_calls[3]
            .output_text
            .as_deref()
            .expect("act-bash output");
        assert!(bash_output.contains("FULLOUTPUT-SENTINEL"));
        assert!(!bash_output.contains("<truncated-output>"));

        // The skill payload call settles with the payload block.
        let skill = &parsed.tool_calls[0];
        assert_eq!(skill.id, format!("{R302}-e1:invoked-skill:vibe-skill"));
        assert_eq!(
            skill.input_json.as_deref(),
            Some("{\"name\":\"vibe-skill\"}")
        );
        let skill_output = skill.output_text.as_deref().expect("skill output");
        assert!(skill_output.starts_with("<skill_content name=\"vibe-skill\">"));

        // Pinned field mapping on act-read.
        let read = &parsed.tool_calls[1];
        assert_eq!(read.id, format!("{R302}-effect-act-read"));
        assert_eq!(read.parser_call_id.as_deref(), Some("act-read"));
        assert_eq!(read.title.as_deref(), Some("Read Cargo.toml"));
        assert_eq!(read.summary.as_deref(), Some("file_system.read_file"));
        assert_eq!(
            read.input_json.as_deref(),
            Some("{\"path\":\"Cargo.toml\"}")
        );
        assert_eq!(read.subagent_id, None);
        assert_eq!(read.started_at, Some(1_792_700_003));
        assert_eq!(read.ended_at, Some(1_792_700_003));
        assert_eq!(read.duration_ms, Some(0));

        // act-gh was settled by its journal replacement.
        let gh = &parsed.tool_calls[4];
        assert_eq!(gh.parser_call_id.as_deref(), Some("act-gh"));
        assert_eq!(gh.output_text.as_deref(), Some("2 results"));

        // Transcript item kinds interleave rows and messages in history
        // order (7 messages, 7 tool calls, 3 subagents).
        let kinds: Vec<&str> = parsed
            .transcript_items
            .iter()
            .map(|item| item.kind.to_storage())
            .collect();
        assert_eq!(
            kinds,
            [
                "message",
                "tool_call",
                "tool_call",
                "message",
                "message",
                "tool_call",
                "tool_call",
                "subagent",
                "subagent",
                "subagent",
                "message",
                "message",
                "message",
                "tool_call",
                "message",
                "tool_call",
                "tool_call",
            ]
        );
    }

    #[test]
    fn r302_subagents() {
        let parsed = parse_fixture(&unified_rel(R302)).unwrap().parsed;
        let child_b = child_session_id(R302, "act-spawn-derived");
        let ghost = child_session_id(R302, "act-spawn-missing");

        assert_eq!(parsed.subagents.len(), 3);
        let agent_ids: Vec<&str> = parsed
            .subagents
            .iter()
            .map(|row| row.agent_id.as_deref().expect("agent_id"))
            .collect();
        assert_eq!(agent_ids, [CHILD_A, child_b.as_str(), ghost.as_str()]);
        let titles: Vec<&str> = parsed
            .subagents
            .iter()
            .map(|row| row.title.as_str())
            .collect();
        assert_eq!(titles, ["scout", "auditor", "ghost"]);
        let summaries: Vec<Option<&str>> = parsed
            .subagents
            .iter()
            .map(|row| row.result_summary.as_deref())
            .collect();
        assert_eq!(
            summaries,
            [
                Some("Found 3 parsers"),
                Some("Audit done"),
                Some("spawn failed"),
            ]
        );
        for row in &parsed.subagents {
            assert_eq!(row.child_session_id, None, "links are set by the indexer");
            assert_eq!(row.session_id, R302);
        }

        let scout = &parsed.subagents[0];
        assert_eq!(scout.id, format!("{R302}-effect-act-spawn-explicit"));
        assert_eq!(scout.agent_name.as_deref(), Some("scout"));
        assert_eq!(scout.prompt.as_deref(), Some("Explore the parser"));
        assert_eq!(scout.parser_ref.as_deref(), Some("act-spawn-explicit"));

        // Each subagent row carries a transcript item.
        let subagent_items: Vec<&TranscriptItem> = parsed
            .transcript_items
            .iter()
            .filter(|item| item.kind.to_storage() == "subagent")
            .collect();
        assert_eq!(subagent_items.len(), 3);
        for (item, row) in subagent_items.iter().zip(&parsed.subagents) {
            assert_eq!(item.subagent_id.as_deref(), Some(row.id.as_str()));
        }
    }

    #[test]
    fn sandbox_receipt_resolves_local_record() {
        let parsed = parse_fixture(&unified_rel(CHILD_A)).unwrap().parsed;

        assert_eq!(parsed.tool_calls.len(), 1);
        let call = &parsed.tool_calls[0];
        assert_eq!(call.parser_call_id.as_deref(), Some("act-sbx"));
        let output = call.output_text.as_deref().expect("sandbox output");
        assert!(output.contains("SANDBOX-SENTINEL"));
        assert!(!output.contains("<truncated-output>"));
    }

    #[test]
    fn old_receipt_variant_resolves() {
        let parsed = parse_fixture(&unified_rel(R300)).unwrap().parsed;

        let big = parsed
            .tool_calls
            .iter()
            .find(|call| call.parser_call_id.as_deref() == Some("act-big"))
            .expect("act-big tool call");
        let output = big.output_text.as_deref().expect("old receipt output");
        assert!(output.contains("OLDRECEIPT-SENTINEL"));
        assert!(!output.contains("<truncated-output>"));
    }

    #[test]
    fn receipt_with_encoded_name_resolves_local_record() {
        // Review Focus 4: the percent-encoded basename is used verbatim.
        let session = crate::fixture_path(&unified_rel(R302));
        let saved =
            fs::read_to_string(session.join("tool-results").join("bash-call%3A9.json")).unwrap();
        let size = saved.encode_utf16().count();

        let mut resolver = SavedResultResolver::new(session.join("tool-results"));
        let resolved = resolver
            .resolve(&receipt(
                &format!(
                    "/home/fixture/.vibe/logs/session/unified/{R302}/tool-results/bash-call%3A9.json"
                ),
                size,
            ))
            .unwrap()
            .expect("the encoded record resolves");
        assert!(resolved.contains("FULLOUTPUT-SENTINEL"));
        assert_eq!(resolver.into_consumed()[0].0, "bash-call%3A9.json");

        // A receipt rewritten in memory to escape tool-results is
        // corrupt, and so is an absolute foreign path.
        let escape = receipt("/x/tool-results/../meta.json", size);
        let mut resolver = SavedResultResolver::new(session.join("tool-results"));
        assert_eq!(
            error_variant(&resolver.resolve(&escape).unwrap_err()),
            "Corrupt"
        );
        let foreign = receipt("/etc/passwd", size);
        assert_eq!(
            error_variant(&resolver.resolve(&foreign).unwrap_err()),
            "Corrupt"
        );
    }

    // ------------------------------------------------------------------
    // Temp-copy and variant negative cases (brief steps 1d-1e)
    // ------------------------------------------------------------------

    #[test]
    fn missing_saved_result_is_reported() {
        let (temp, session) = copy_unified(R302);
        fs::remove_file(session.join("tool-results").join("bash-call%3A9.json")).unwrap();

        let error = MistralVibeUnifiedParser
            .parse(&session)
            .expect_err("the saved record is missing");
        assert_eq!(error_variant(&error), "MissingSavedResult");
        let _ = temp;
    }

    #[test]
    fn tampered_saved_result_is_corrupt() {
        let (temp, session) = copy_unified(R302);
        let path = session.join("tool-results").join("bash-call%3A9.json");
        let mut bytes = fs::read(&path).unwrap();
        bytes.push(b'}');
        fs::write(&path, bytes).unwrap();

        let error = MistralVibeUnifiedParser
            .parse(&session)
            .expect_err("the saved record no longer matches its receipt");
        assert_eq!(error_variant(&error), "Corrupt");
        let _ = temp;
    }

    #[test]
    fn unknown_effect_status_is_unsupported() {
        let error = parse_fixture(&variant_rel("unknown-status", V_UNKNOWN_STATUS))
            .expect_err("unknown effect status");
        assert_eq!(error_variant(&error), "Unsupported");
    }

    #[test]
    fn child_with_mismatched_spawn_key_is_identity_conflict() {
        let child_b = child_session_id(R302, "act-spawn-derived");
        let (temp, session) = copy_unified(&child_b);
        rewrite_document(&session, "runtime-state.json", |document| {
            document["session_metadata"]["subagent_spawn_key"] = json!("act-other");
        });

        let error = MistralVibeUnifiedParser
            .parse(&session)
            .expect_err("the spawn key no longer derives the child id");
        assert_eq!(error_variant(&error), "IdentityConflict");
        let _ = temp;
    }

    // ------------------------------------------------------------------
    // Statuses and id uniqueness (pinned rules without fixtures)
    // ------------------------------------------------------------------

    #[test]
    fn running_effects_have_no_end() {
        let entries = vec![
            in_memory_user_message(),
            tool_effect("act-run", "running", json!({"outputText": ""})),
        ];

        let parsed = convert_entries(entries).unwrap();
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].status, ToolCallStatus::Running);
        for call in &parsed.tool_calls {
            assert!(
                call.started_at.is_some(),
                "a started effect keeps its start"
            );
            assert_eq!(
                call.ended_at, None,
                "a running or pending effect has not ended"
            );
            assert_eq!(call.duration_ms, None);
        }
    }

    #[test]
    fn repeated_effect_id_gets_dup_suffix() {
        let effect = tool_effect("act-dup", "completed", json!({"outputText": "ok"}));
        let entries = vec![in_memory_user_message(), effect.clone(), effect];

        let parsed = convert_entries(entries).unwrap();
        assert_eq!(parsed.tool_calls.len(), 2);
        assert_eq!(
            parsed.tool_calls[0].id,
            format!("{IN_MEMORY}-effect-act-dup")
        );
        assert_eq!(
            parsed.tool_calls[1].id,
            format!("{IN_MEMORY}-effect-act-dup-dup1")
        );
    }
}
