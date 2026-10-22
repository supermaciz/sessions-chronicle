use super::ParseError;
use super::test_support;
use super::v2::V2Reader;
use crate::models::{ContextKind, Role, ToolCall, ToolCallStatus, TranscriptItemKind};
use crate::parsers::ParsedSession;

/// Builds the synthetic fixture in a temporary database and parses `session_id`.
fn parse_session(session_id: &str) -> ParsedSession {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("fixture.db");
    let conn = test_support::create_v2_db(&db_path);
    drop(conn);
    let reader = V2Reader::open(&db_path).unwrap();
    reader.parse_session(session_id).unwrap()
}

fn find_tool<'a>(parsed: &'a ParsedSession, id: &str) -> &'a ToolCall {
    parsed.tool_calls.iter().find(|tc| tc.id == id).unwrap()
}

#[test]
fn v2_sequence_and_content_order() {
    let parsed = test_support::parse_fixture();

    // Messages appear in native `seq` order, with assistant text on either side
    // of the reasoning block.
    let contents: Vec<(&str, Role)> = parsed
        .messages
        .iter()
        .map(|m| (m.content.as_str(), m.role))
        .collect();
    assert_eq!(
        contents,
        vec![
            ("real prompt", Role::User),
            ("answer part one", Role::Assistant),
            ("answer part two", Role::Assistant),
            ("prompt with skill", Role::User),
            ("final answer", Role::Assistant),
        ]
    );

    // Exact transcript-kind sequence: system context, user, assistant text,
    // assistant text, user, attached-skill context, standalone-skill context,
    // assistant text, compaction-summary context.
    let kinds: Vec<TranscriptItemKind> = parsed
        .transcript_items
        .iter()
        .map(|t| t.kind.clone())
        .collect();
    assert_eq!(
        kinds,
        vec![
            TranscriptItemKind::Context,
            TranscriptItemKind::Message,
            TranscriptItemKind::Message,
            TranscriptItemKind::Message,
            TranscriptItemKind::Message,
            TranscriptItemKind::Context,
            TranscriptItemKind::Context,
            TranscriptItemKind::Message,
            TranscriptItemKind::Context,
        ]
    );

    // Source identities: whole-message context uses the native message id,
    // user-attached skills use `{message_id}:skill:{position}`.
    assert_eq!(
        parsed.transcript_items[0].context_id.as_deref(),
        Some("msg_sys_01")
    );
    assert_eq!(
        parsed.transcript_items[5].context_id.as_deref(),
        Some("msg_user_skill:skill:0")
    );
    assert_eq!(
        parsed.transcript_items[6].context_id.as_deref(),
        Some("msg_skill_01")
    );
    assert_eq!(
        parsed.transcript_items[8].context_id.as_deref(),
        Some("msg_comp_01")
    );

    // Reasoning attaches to the item that follows it (the second assistant text).
    assert_eq!(parsed.reasoning_attachments.len(), 1);
    assert_eq!(parsed.reasoning_attachments[0].transcript_item_index, 3);
    assert_eq!(
        parsed.reasoning_attachments[0].visible_text.as_deref(),
        Some("thinking out loud")
    );

    // Per-message model is preserved, and a missing model is not inherited.
    assert_eq!(parsed.messages[1].model.as_deref(), Some("test/model-a"));
    assert_eq!(parsed.messages[4].model, None);
}

#[test]
fn v2_context_and_eligibility() {
    let parsed = test_support::parse_fixture();

    assert_eq!(parsed.session.first_prompt.as_deref(), Some("real prompt"));
    assert!(
        parsed
            .messages
            .iter()
            .all(|m| !m.content.contains("contextonlyneedle"))
    );
    assert_eq!(
        parsed
            .context_entries
            .iter()
            .filter(|c| c.kind == ContextKind::CompactionSummary)
            .count(),
        1
    );

    // One system context and two skill contexts (one user-attached, one
    // standalone); the text-less skill attachment adds no fabricated body.
    assert_eq!(
        parsed
            .context_entries
            .iter()
            .filter(|c| c.kind == ContextKind::System)
            .count(),
        1
    );
    assert_eq!(
        parsed
            .context_entries
            .iter()
            .filter(|c| c.kind == ContextKind::Skill)
            .count(),
        2
    );
    assert!(
        parsed
            .context_entries
            .iter()
            .all(|c| c.name.as_deref() != Some("Skill B"))
    );

    // Repeated skill name/text in different source messages stay separate.
    let skill_ids: Vec<&str> = parsed
        .context_entries
        .iter()
        .filter(|c| c.kind == ContextKind::Skill)
        .map(|c| c.id.as_str())
        .collect();
    assert_eq!(skill_ids, vec!["msg_user_skill:skill:0", "msg_skill_01"]);

    // Context item_index agrees with its transcript index.
    for entry in &parsed.context_entries {
        let item = &parsed.transcript_items[entry.item_index as usize];
        assert_eq!(item.kind, TranscriptItemKind::Context);
        assert_eq!(item.context_id.as_deref(), Some(entry.id.as_str()));
    }

    // A valid system-only session has no user messages.
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("system_only.db");
    test_support::create_v2_db(&db_path);
    let reader = V2Reader::open(&db_path).unwrap();
    let err = reader.parse_session("ses_v2_system_only").unwrap_err();
    assert!(matches!(
        err.downcast_ref::<ParseError>(),
        Some(ParseError::NoUserMessages)
    ));

    // A NULL title also falls back to the first genuine user prompt.
    let parsed_null = reader.parse_session("ses_v2_null_title").unwrap();
    assert_eq!(
        parsed_null.session.first_prompt.as_deref(),
        Some("null title prompt")
    );
}

#[test]
fn v2_usage_is_assistant_only() {
    let parsed = test_support::parse_fixture();

    let usage = parsed.token_usage.expect("assistant usage present");
    // assistant 10 + 20; the session rollup 999 is ignored.
    assert_eq!(usage.input_tokens, 30);
    // assistant 3 + 4.
    assert_eq!(usage.output_tokens, 7);
    // Known-zero reasoning/cache remain absent rather than manufactured zero usage.
    assert_eq!(usage.reasoning_tokens, None);
    assert_eq!(usage.cache_read_tokens, None);
    assert_eq!(usage.cache_write_tokens, None);
}

#[test]
fn v2_malformed_rows_continue() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("malformed.db");
    test_support::create_v2_db(&db_path);

    let reader = V2Reader::open(&db_path).unwrap();
    let parsed = reader.parse_session("ses_v2_malformed").unwrap();

    let contents: Vec<&str> = parsed.messages.iter().map(|m| m.content.as_str()).collect();
    assert_eq!(contents, vec!["first real prompt", "assistant reply"]);
}

#[test]
fn v2_corrupt_user_is_not_ineligible() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("corrupt.db");
    test_support::create_v2_db(&db_path);

    let reader = V2Reader::open(&db_path).unwrap();

    // A corrupt user record prevents establishing eligibility: it must be
    // IncompleteTranscript, not the policy exclusion NoUserMessages.
    let err = reader.parse_session("ses_v2_corrupt_user").unwrap_err();
    assert!(matches!(
        err.downcast_ref::<ParseError>(),
        Some(ParseError::IncompleteTranscript)
    ));

    // A corrupt item followed by a valid user/content preserves the latter.
    let parsed = reader.parse_session("ses_v2_malformed_then_valid").unwrap();
    assert_eq!(parsed.messages.len(), 1);
    assert_eq!(parsed.messages[0].content, "valid prompt");
    assert_eq!(parsed.messages[0].role, Role::User);
}

#[test]
fn v2_unreadable_user_time_created_is_incomplete() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("badtime.db");
    let conn = test_support::create_v2_db(&db_path);
    conn.execute_batch("DELETE FROM session_message; DELETE FROM session_v2;")
        .unwrap();
    conn.execute(
        "INSERT INTO session_v2 (id, project_id, directory, title, parent_id, version, slug, model,
                                  tokens_input, tokens_output, tokens_reasoning, tokens_cache_read,
                                  tokens_cache_write, time_created, time_updated)
         VALUES ('ses_badtime', 'prj', '/tmp/proj', NULL, NULL, '0.0.0', 'slug-badtime', NULL,
                 0, 0, 0, 0, 0, 1712000000000, 1712000000000)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
         VALUES ('msg-badtime', 'ses_badtime', 'user', 1, 9223372036854775807, 9223372036854775807, ?1)",
        rusqlite::params![serde_json::json!({ "text": "hello" }).to_string()],
    )
    .unwrap();
    drop(conn);

    let reader = V2Reader::open(&db_path).unwrap();
    let err = reader.parse_session("ses_badtime").unwrap_err();
    assert!(matches!(
        err.downcast_ref::<ParseError>(),
        Some(ParseError::IncompleteTranscript)
    ));
}

#[test]
fn v2_reasoning_skips_context_and_attaches_to_next_visible_item() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("reason_ctx.db");
    let conn = test_support::create_v2_db(&db_path);
    conn.execute_batch("DELETE FROM session_message; DELETE FROM session_v2;")
        .unwrap();
    conn.execute(
        "INSERT INTO session_v2 (id, project_id, directory, title, parent_id, version, slug, model,
                                  tokens_input, tokens_output, tokens_reasoning, tokens_cache_read,
                                  tokens_cache_write, time_created, time_updated)
         VALUES ('ses_reason_ctx', 'prj', '/tmp/proj', NULL, NULL, '0.0.0', 'slug-reason-ctx', NULL,
                 0, 0, 0, 0, 0, 1712000000000, 1712000000000)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
         VALUES ('msg-r-user', 'ses_reason_ctx', 'user', 1, 1712000010000, 1712000010000, ?1)",
        rusqlite::params![serde_json::json!({ "text": "prompt" }).to_string()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
         VALUES ('msg-r-reason', 'ses_reason_ctx', 'assistant', 2, 1712000020000, 1712000020000, ?1)",
        rusqlite::params![serde_json::json!({
            "agent": "primary",
            "content": [{ "type": "reasoning", "text": "thinking about compaction" }],
            "time": { "created": 1712000020000_i64 }
        })
        .to_string()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
         VALUES ('msg-r-ctx', 'ses_reason_ctx', 'system', 3, 1712000030000, 1712000030000, ?1)",
        rusqlite::params![
            serde_json::json!({
                "text": "context row",
                "description": "System prompt",
                "time": { "created": 1712000030000_i64 }
            })
            .to_string()
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
         VALUES ('msg-r-final', 'ses_reason_ctx', 'assistant', 4, 1712000040000, 1712000040000, ?1)",
        rusqlite::params![serde_json::json!({
            "agent": "primary",
            "content": [{ "type": "text", "text": "final answer" }],
            "time": { "created": 1712000040000_i64 }
        })
        .to_string()],
    )
    .unwrap();
    drop(conn);

    let reader = V2Reader::open(&db_path).unwrap();
    let parsed = reader.parse_session("ses_reason_ctx").unwrap();

    // The reasoning must attach to the following assistant text item, not the
    // intervening context item.
    assert_eq!(parsed.reasoning_attachments.len(), 1);
    assert_eq!(
        parsed.reasoning_attachments[0].visible_text.as_deref(),
        Some("thinking about compaction")
    );
    let context_item = parsed
        .transcript_items
        .iter()
        .position(|t| t.kind == TranscriptItemKind::Context)
        .expect("context transcript item");
    let assistant_item = parsed
        .transcript_items
        .iter()
        .position(|t| {
            t.kind == TranscriptItemKind::Message
                && parsed.messages[t.message_index.unwrap() as usize].role == Role::Assistant
        })
        .expect("assistant text transcript item");
    assert_ne!(
        parsed.reasoning_attachments[0].transcript_item_index as usize, context_item,
        "reasoning must not attach to the context item"
    );
    assert_eq!(
        parsed.reasoning_attachments[0].transcript_item_index as usize,
        assistant_item
    );
}

#[test]
fn v2_snapshot_is_consistent() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("snapshot.db");

    let writer = test_support::create_v2_db(&db_path);
    writer.execute_batch("PRAGMA journal_mode=WAL").unwrap();

    // A reader opened before the write keeps one snapshot.
    let reader_a = V2Reader::open(&db_path).unwrap();
    let ids = reader_a.list_session_ids().unwrap();
    assert!(ids.iter().any(|id| id == "ses_v2"));

    // A separate writer updates a message after enumeration.
    writer
        .execute(
            "UPDATE session_message
                SET data = json_set(data, '$.content[0].text', 'final answer v2')
              WHERE id = 'msg_final'",
            [],
        )
        .unwrap();

    // Parsing through the retained snapshot still sees the old text.
    let parsed_a = reader_a.parse_session("ses_v2").unwrap();
    let final_a = parsed_a
        .messages
        .iter()
        .find(|m| m.content.starts_with("final answer"))
        .expect("final answer message");
    assert_eq!(final_a.content, "final answer");

    drop(reader_a);

    // A newly opened reader sees the update.
    let reader_b = V2Reader::open(&db_path).unwrap();
    let parsed_b = reader_b.parse_session("ses_v2").unwrap();
    let final_b = parsed_b
        .messages
        .iter()
        .find(|m| m.content.starts_with("final answer"))
        .expect("final answer message");
    assert_eq!(final_b.content, "final answer v2");

    drop(writer);
}

#[test]
fn v2_tool_lifecycle() {
    let parsed = parse_session("ses_v2_tools");

    let streaming = find_tool(&parsed, "call_stream");
    assert_eq!(streaming.status, ToolCallStatus::Pending);
    assert_eq!(streaming.input_json.as_deref(), Some("{\"path\":"));

    let running = find_tool(&parsed, "call_run");
    assert_eq!(running.status, ToolCallStatus::Running);
    assert_eq!(running.input_json.as_deref(), Some("{\"command\":\"ls\"}"));
    assert_eq!(running.duration_ms, None);

    let completed = find_tool(&parsed, "call_done");
    assert_eq!(completed.status, ToolCallStatus::Completed);
    assert_eq!(completed.output_text.as_deref(), Some("first\nsecond"));
    assert_eq!(completed.started_at, Some(1712000201200));
    assert_eq!(completed.ended_at, Some(1712000201700));
    assert_eq!(completed.duration_ms, Some(500));

    let failed = find_tool(&parsed, "call_err");
    assert_eq!(failed.status, ToolCallStatus::Error);
    assert_eq!(failed.error_text.as_deref(), Some("permission denied"));
    assert_eq!(failed.output_text.as_deref(), Some("partial output"));

    let unknown = find_tool(&parsed, "call_unk");
    assert_eq!(unknown.status, ToolCallStatus::Unknown);

    let invalid_duration = find_tool(&parsed, "call_inv");
    assert_eq!(invalid_duration.status, ToolCallStatus::Completed);
    assert_eq!(invalid_duration.duration_ms, None);
    assert_eq!(invalid_duration.started_at, Some(1712000202200));
    assert_eq!(invalid_duration.ended_at, Some(1712000202000));

    assert_eq!(parsed.tool_calls.len(), 6);
}

#[test]
fn v2_subagent_completion_links_once() {
    // task + completion: metadata.childID (not the completion prose) is
    // authoritative, and the absent result is filled from the completion.
    let task_done = parse_session("ses_v2_task_done");
    assert_eq!(task_done.subagents.len(), 1);
    assert_eq!(
        task_done.subagents[0].child_session_id.as_deref(),
        Some("ses_child_a")
    );
    assert_eq!(
        task_done.subagents[0].result_summary.as_deref(),
        Some("<subagent sessionID=\"ses_child_fake\" state=\"completed\">done</subagent>")
    );
    assert!(task_done.tool_calls.iter().all(|tc| tc.tool_name != "task"));
    // The synthetic completion must not create a context row.
    assert!(task_done.context_entries.is_empty());

    // completion alone: a standalone subagent row keyed by the native message id.
    let completion_only = parse_session("ses_v2_completion_only");
    assert_eq!(completion_only.subagents.len(), 1);
    assert_eq!(completion_only.subagents[0].id, "msg_completion");
    assert_eq!(
        completion_only.subagents[0].child_session_id.as_deref(),
        Some("ses_child_o")
    );

    // child missing: the native childID is preserved even though unindexed.
    let child_missing = parse_session("ses_v2_child_missing");
    assert_eq!(child_missing.subagents.len(), 1);
    assert_eq!(
        child_missing.subagents[0].child_session_id.as_deref(),
        Some("ses_child_missing")
    );
    assert_eq!(child_missing.subagents[0].id, "msg_missing_completion");

    // two resumed calls to the same child: two subagents, no task tool calls,
    // and completion text never overwrites explicit task results.
    let resumed = parse_session("ses_v2_resumed");
    assert_eq!(resumed.subagents.len(), 2);
    assert!(resumed.tool_calls.iter().all(|tc| tc.tool_name != "task"));
    let results: Vec<&str> = resumed
        .subagents
        .iter()
        .map(|s| s.result_summary.as_deref().unwrap())
        .collect();
    assert_eq!(results, vec!["first result", "second result"]);
    assert!(
        resumed
            .subagents
            .iter()
            .all(|s| s.child_session_id.as_deref() == Some("ses_child_r"))
    );

    // completion before a task: correlation only looks backwards.
    let before = parse_session("ses_v2_completion_before_task");
    assert_eq!(before.subagents.len(), 2);
    assert_eq!(before.subagents[0].id, "msg_early_completion");
    assert_eq!(
        before.subagents[0].result_summary.as_deref(),
        Some("early completion")
    );
    assert_eq!(before.subagents[1].id, "call_b1");
    assert_eq!(
        before.subagents[1].result_summary.as_deref(),
        Some("late task")
    );

    // ambiguous duplicate-sequence candidates: no merge, completion standalone.
    let ambiguous = parse_session("ses_v2_ambiguous");
    assert_eq!(ambiguous.subagents.len(), 3);
    let results: Vec<&str> = ambiguous
        .subagents
        .iter()
        .map(|s| s.result_summary.as_deref().unwrap())
        .collect();
    assert_eq!(
        results,
        vec!["task one result", "task two result", "ambiguous completion"]
    );
    assert!(ambiguous.tool_calls.iter().all(|tc| tc.tool_name != "task"));

    // input.task_id alone does not establish a child link.
    let task_id = parse_session("ses_v2_task_id_link");
    assert_eq!(task_id.subagents.len(), 2);
    assert_eq!(task_id.subagents[0].child_session_id, None);
    assert_eq!(
        task_id.subagents[0].result_summary.as_deref(),
        Some("resumed result")
    );
    assert_eq!(
        task_id.subagents[1].child_session_id.as_deref(),
        Some("ses_requested")
    );
    assert_eq!(task_id.subagents[1].id, "msg_task_id_completion");
}

#[test]
fn v2_subagent_invocation_names_and_child_metadata() {
    for name in ["subagent", "task"] {
        for child_key in [Some("sessionID"), Some("sessionId"), None] {
            let dir = tempfile::tempdir().unwrap();
            let db_path = dir.path().join("fixture.db");
            let conn = test_support::create_v2_db(&db_path);
            let mut metadata = serde_json::json!({"status": "completed", "truncated": false});
            if let Some(key) = child_key {
                metadata[key] = serde_json::json!("ses_synthetic_child");
            }
            let data = serde_json::json!({"content": [{
                "type": "tool", "id": "call_synthetic", "name": name,
                "executed": true, "native": true,
                "state": {
                    "input": {"agent": "explore", "description": "Inspect sample",
                              "prompt": "Synthetic prompt", "task_id": "ses_not_a_link"},
                    "content": [{"type": "text", "text": "Synthetic result"}],
                    "metadata": metadata
                },
                "time": {"created": 1712000000000_i64, "completed": 1712000000100_i64}
            }]});
            conn.execute(
                "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
                 VALUES ('msg_synthetic', 'ses_v2', 'assistant', 999, 1712000000000, 1712000000100, ?1)",
                [data.to_string()],
            ).unwrap();
            drop(conn);
            let parsed = V2Reader::open(&db_path)
                .unwrap()
                .parse_session("ses_v2")
                .unwrap();
            let item = parsed
                .transcript_items
                .iter()
                .find(|item| item.subagent_id.as_deref() == Some("call_synthetic"))
                .expect("invocation must be a subagent transcript item");
            assert_eq!(item.kind, TranscriptItemKind::Subagent);
            assert_eq!(item.tool_call_id, None);
            assert!(
                parsed
                    .tool_calls
                    .iter()
                    .all(|call| call.id != "call_synthetic")
            );
            let subagent = parsed
                .subagents
                .iter()
                .find(|s| s.id == "call_synthetic")
                .unwrap();
            assert_eq!(
                subagent.child_session_id.as_deref(),
                child_key.map(|_| "ses_synthetic_child")
            );
            assert_eq!(subagent.agent_name.as_deref(), Some("explore"));
            assert_eq!(subagent.title, "Inspect sample");
            assert_eq!(subagent.prompt.as_deref(), Some("Synthetic prompt"));
            assert_eq!(subagent.result_summary.as_deref(), Some("Synthetic result"));
        }
    }
}

#[test]
fn v2_skill_calls_remain_tool_calls() {
    let parsed = parse_session("ses_v2_skill_tool");

    // The ordinary `skill` tool call is a tool call, not context.
    assert_eq!(parsed.tool_calls.len(), 1);
    assert_eq!(parsed.tool_calls[0].tool_name, "skill");
    assert_eq!(parsed.tool_calls[0].status, ToolCallStatus::Completed);

    // The injected skill content stays context, not a tool call or message.
    let skill_context: Vec<_> = parsed
        .context_entries
        .iter()
        .filter(|c| c.kind == ContextKind::Skill)
        .collect();
    assert_eq!(skill_context.len(), 1);
    assert_eq!(skill_context[0].id, "msg_skill_tool_user:skill:0");

    // No task tool was emitted, and no skill body leaked into messages.
    assert!(parsed.subagents.is_empty());
    assert!(parsed.messages.iter().all(|m| m.role != Role::Assistant
        || m.content != "Run the brainstorming workflow before starting creative work."));
}
