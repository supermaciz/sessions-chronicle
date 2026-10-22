use super::ParseError;
use super::test_support;
use super::v2::V2Reader;
use crate::models::{ContextKind, Role, TranscriptItemKind};

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
