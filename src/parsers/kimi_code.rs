#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Role, TokenUsage, ToolCallStatus, TranscriptItemKind};
    use serde_json::{Value, json};
    use std::fs;
    use std::path::PathBuf;

    fn write_bundle(state: Value, records: &[&str]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        let session_dir = root
            .path()
            .join("sessions/wd_test_aaaaaaaaaaaa/session_00000000-0000-4000-8000-000000000001");
        fs::create_dir_all(session_dir.join("agents/main")).unwrap();
        fs::write(
            session_dir.join("state.json"),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();
        fs::write(
            session_dir.join("agents/main/wire.jsonl"),
            format!("{}\n", records.join("\n")),
        )
        .unwrap();
        root
    }

    fn session_dir(root: &tempfile::TempDir) -> PathBuf {
        root.path()
            .join("sessions/wd_test_aaaaaaaaaaaa/session_00000000-0000-4000-8000-000000000001")
    }

    fn write_agent_journal(root: &tempfile::TempDir, agent_id: &str, records: &[&str]) {
        let path = session_dir(root).join("agents").join(agent_id);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("wire.jsonl"), format!("{}\n", records.join("\n"))).unwrap();
    }

    fn parse_single_agent_call(records: &[&str]) -> ParsedSession {
        let mut metadata = state();
        metadata["agents"] = json!({
            "main": { "type": "main", "parentAgentId": null },
            "agent-0": { "type": "sub", "parentAgentId": "main" }
        });
        let root = write_bundle(metadata, records);
        write_agent_journal(
            &root,
            "agent-0",
            &[
                r#"{"type":"turn.prompt","time":1785320000030,"input":[{"type":"text","text":"Child"}],"origin":{"kind":"user"}}"#,
            ],
        );
        KimiCodeParser::new(root.path())
            .parse_session_dir(&session_dir(&root))
            .unwrap()
            .main
    }

    fn state() -> Value {
        json!({
            "createdAt": "2026-07-29T10:00:00Z",
            "updatedAt": 1785320005000_i64,
            "title": "New Session",
            "workDir": "/tmp/project",
            "agents": { "main": { "type": "main", "parentAgentId": null } }
        })
    }

    #[test]
    fn main_messages_use_turn_prompt_once_and_preserve_text_part_order() {
        let root = write_bundle(
            state(),
            &[
                r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Human prompt"}],"origin":{"kind":"user"}}"#,
                r#"{"type":"context.append_message","time":1785320000001,"message":{"role":"user","content":[{"type":"text","text":"Human prompt"}],"origin":{"kind":"user"}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320001000,"event":{"type":"content.part","stepUuid":"s1","part":{"type":"text","text":"First"}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320001001,"event":{"type":"content.part","stepUuid":"s1","part":{"type":"text","text":"Second"}}}"#,
            ],
        );

        let parsed = KimiCodeParser::new(root.path())
            .parse_session_dir(&session_dir(&root))
            .unwrap();
        let contents: Vec<_> = parsed
            .main
            .messages
            .iter()
            .map(|message| (message.role.clone(), message.content.as_str()))
            .collect();

        assert_eq!(
            contents,
            vec![
                (Role::User, "Human prompt"),
                (Role::Assistant, "First"),
                (Role::Assistant, "Second"),
            ]
        );
        assert_eq!(
            parsed.main.session.first_prompt.as_deref(),
            Some("Human prompt")
        );
        assert_eq!(
            parsed.main.session.id,
            "session_00000000-0000-4000-8000-000000000001"
        );
    }

    #[test]
    fn context_message_is_fallback_and_injected_origins_do_not_validate() {
        let fallback = write_bundle(
            state(),
            &[
                r#"{"type":"context.append_message","message":{"role":"user","content":[{"type":"text","text":"Fallback prompt"}],"origin":{"kind":"user"}}}"#,
                r#"{"type":"context.append_loop_event","event":{"type":"content.part","stepUuid":"s1","part":{"type":"text","text":"Answer"}}}"#,
            ],
        );
        let parsed = KimiCodeParser::new(fallback.path())
            .parse_session_dir(&session_dir(&fallback))
            .unwrap();
        assert_eq!(parsed.main.messages[0].content, "Fallback prompt");

        let injected = write_bundle(
            state(),
            &[
                r#"{"type":"turn.prompt","input":[{"type":"text","text":"Injected"}],"origin":{"kind":"injection"}}"#,
                r#"{"type":"turn.prompt","input":[{"type":"text","text":"Skill body"}],"origin":{"kind":"skill_activation"}}"#,
            ],
        );
        let error = KimiCodeParser::new(injected.path())
            .parse_session_dir(&session_dir(&injected))
            .unwrap_err();
        assert!(matches!(
            error.downcast_ref::<ParseError>(),
            Some(ParseError::NoUserMessages)
        ));
    }

    #[test]
    fn reasoning_attaches_to_next_visible_item_and_malformed_lines_are_local() {
        let root = write_bundle(
            state(),
            &[
                r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Explain"}],"origin":{"kind":"user"}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000100,"event":{"type":"content.part","stepUuid":"s1","part":{"type":"think","think":"Reason one"}}}"#,
                "not-json",
                r#"{"type":"context.append_loop_event","time":1785320000200,"event":{"type":"content.part","stepUuid":"s1","part":{"type":"think","think":"Reason two"}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000300,"event":{"type":"content.part","stepUuid":"s1","part":{"type":"text","text":"Visible"}}}"#,
                r#"{"type":"unknown.future.record","time":1785320000400}"#,
            ],
        );
        let parsed = KimiCodeParser::new(root.path())
            .parse_session_dir(&session_dir(&root))
            .unwrap();

        assert_eq!(parsed.main.reasoning_attachments.len(), 1);
        assert_eq!(
            parsed.main.reasoning_attachments[0].visible_text.as_deref(),
            Some("Reason one\nReason two")
        );
        assert_eq!(
            parsed.main.reasoning_attachments[0].transcript_item_index,
            1
        );
        assert_eq!(
            parsed.main.transcript_items[1].kind,
            TranscriptItemKind::Message
        );
    }

    #[test]
    fn reasoning_from_an_interrupted_turn_is_dropped_at_the_next_prompt() {
        let root = write_bundle(
            state(),
            &[
                r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Explain"}],"origin":{"kind":"user"}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000100,"event":{"type":"content.part","stepUuid":"s1","part":{"type":"think","think":"Interrupted reasoning"}}}"#,
                r#"{"type":"turn.prompt","time":1785320000200,"input":[{"type":"text","text":"Never mind"}],"origin":{"kind":"user"}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000300,"event":{"type":"content.part","stepUuid":"s1","part":{"type":"text","text":"Visible"}}}"#,
            ],
        );
        let parsed = KimiCodeParser::new(root.path())
            .parse_session_dir(&session_dir(&root))
            .unwrap();

        assert!(parsed.main.reasoning_attachments.is_empty());
    }

    #[test]
    fn metadata_paths_ids_titles_and_timestamps_follow_precedence() {
        let mut metadata = state();
        metadata["id"] = json!("session_wrong");
        metadata["title"] = json!("  Frozen title  ");
        metadata["cwd"] = json!("/ignored/cwd");
        let root = write_bundle(
            metadata,
            &[
                r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Prompt"}],"origin":{"kind":"user"}}"#,
            ],
        );
        let parsed = KimiCodeParser::new(root.path())
            .parse_session_dir(&session_dir(&root))
            .unwrap();

        assert_eq!(
            parsed.main.session.id,
            "session_00000000-0000-4000-8000-000000000001"
        );
        assert_eq!(
            parsed.main.session.project_path.as_deref(),
            Some("/tmp/project")
        );
        assert_eq!(
            parsed.main.session.first_prompt.as_deref(),
            Some("Frozen title")
        );
        assert_eq!(parsed.main.session.start_time.timestamp(), 1_785_319_200);
        assert_eq!(parsed.main.session.last_updated.timestamp(), 1_785_320_005);
    }

    #[test]
    fn user_media_parts_use_safe_local_placeholders() {
        let root = write_bundle(
            state(),
            &[
                r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Review"},{"type":"image_url","image_url":{"url":"https://invalid.example/image"}},{"type":"audio_url","audio_url":{"url":"https://invalid.example/audio"}},{"type":"video_url","video_url":{"url":"https://invalid.example/video"}}],"origin":{"kind":"user"}}"#,
            ],
        );
        let parsed = KimiCodeParser::new(root.path())
            .parse_session_dir(&session_dir(&root))
            .unwrap();
        assert_eq!(
            parsed.main.messages[0].content,
            "Review\n[image]\n[audio]\n[video]"
        );
    }

    #[test]
    fn invalid_metadata_and_overflowing_wire_times_fall_back_locally() {
        let mut metadata = state();
        metadata["createdAt"] = json!("invalid");
        metadata["updatedAt"] = json!("invalid");
        let root = write_bundle(
            metadata,
            &[
                r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Prompt"}],"origin":{"kind":"user"}}"#,
                r#"{"type":"context.append_loop_event","time":18446744073709551615,"event":{"type":"content.part","stepUuid":"s1","part":{"type":"text","text":"Answer"}}}"#,
            ],
        );
        let parsed = KimiCodeParser::new(root.path())
            .parse_session_dir(&session_dir(&root))
            .unwrap();
        assert_eq!(parsed.main.session.start_time.timestamp(), 1_785_320_000);
        assert!(parsed.main.session.last_updated >= parsed.main.session.start_time);
        assert_eq!(
            parsed.main.messages[1].timestamp.timestamp_millis(),
            parsed.main.session.start_time.timestamp_millis() + 1
        );
    }

    #[test]
    fn main_last_updated_uses_latest_wire_time_after_state_update() {
        let mut metadata = state();
        metadata["updatedAt"] = json!(1785320001000_i64);
        let root = write_bundle(
            metadata,
            &[
                r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Prompt"}],"origin":{"kind":"user"}}"#,
                r#"{"type":"context.append_loop_event","time":1785320010000,"event":{"type":"content.part","stepUuid":"s1","part":{"type":"text","text":"Latest"}}}"#,
            ],
        );

        let parsed = KimiCodeParser::new(root.path())
            .parse_session_dir(&session_dir(&root))
            .unwrap();

        assert_eq!(
            parsed.main.session.last_updated.timestamp_millis(),
            1_785_320_010_000
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_state_file() {
        use std::os::unix::fs::symlink;

        let root = write_bundle(
            state(),
            &[
                r#"{"type":"turn.prompt","input":[{"type":"text","text":"Prompt"}],"origin":{"kind":"user"}}"#,
            ],
        );
        let state_path = session_dir(&root).join("state.json");
        let outside = root.path().join("outside-state.json");
        fs::rename(&state_path, &outside).unwrap();
        symlink(&outside, &state_path).unwrap();

        assert!(
            KimiCodeParser::new(root.path())
                .parse_session_dir(&session_dir(&root))
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_main_journal() {
        use std::os::unix::fs::symlink;

        let root = write_bundle(
            state(),
            &[
                r#"{"type":"turn.prompt","input":[{"type":"text","text":"Prompt"}],"origin":{"kind":"user"}}"#,
            ],
        );
        let journal_path = session_dir(&root).join("agents/main/wire.jsonl");
        let outside = root.path().join("outside-wire.jsonl");
        fs::rename(&journal_path, &outside).unwrap();
        symlink(&outside, &journal_path).unwrap();

        assert!(
            KimiCodeParser::new(root.path())
                .parse_session_dir(&session_dir(&root))
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_child_agent_directory() {
        use std::os::unix::fs::symlink;

        let mut metadata = state();
        metadata["agents"]["agent-0"] = json!({"type": "sub", "parentAgentId": "main"});
        let root = write_bundle(
            metadata,
            &[
                r#"{"type":"turn.prompt","input":[{"type":"text","text":"Main"}],"origin":{"kind":"user"}}"#,
            ],
        );
        let outside = root.path().join("outside-agent");
        fs::create_dir(&outside).unwrap();
        fs::write(
            outside.join("wire.jsonl"),
            r#"{"type":"turn.prompt","input":[{"type":"text","text":"Child"}],"origin":{"kind":"user"}}"#,
        )
        .unwrap();
        symlink(&outside, session_dir(&root).join("agents/agent-0")).unwrap();

        assert!(
            KimiCodeParser::new(root.path())
                .parse_session_dir(&session_dir(&root))
                .is_err()
        );
    }

    #[test]
    fn tools_correlate_strictly_and_preserve_pending_and_error_states() {
        let root = write_bundle(
            state(),
            &[
                r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Run tools"}],"origin":{"kind":"user"}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000100,"event":{"type":"tool.call","stepUuid":"s1","toolCallId":"Bash_0","name":"Bash","args":{"command":"true"}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000200,"event":{"type":"tool.result","toolCallId":"Bash_0","result":{"output":"ok"}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000300,"event":{"type":"tool.call","stepUuid":"s1","toolCallId":"Read_0","name":"Read","args":{"path":"safe.txt"}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000400,"event":{"type":"tool.result","toolCallId":"Read_0","result":{"output":[{"type":"text","text":"denied"}],"isError":true}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000500,"event":{"type":"tool.call","stepUuid":"s1","toolCallId":"Pending_0","name":"Search","args":{}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000600,"event":{"type":"tool.result","toolCallId":"missing","result":{"output":"ignore"}}}"#,
            ],
        );
        let parsed = KimiCodeParser::new(root.path())
            .parse_session_dir(&session_dir(&root))
            .unwrap();

        assert_eq!(parsed.main.tool_calls.len(), 3);
        assert_eq!(parsed.main.tool_calls[0].status, ToolCallStatus::Completed);
        assert_eq!(parsed.main.tool_calls[0].output_text.as_deref(), Some("ok"));
        assert_eq!(
            parsed.main.tool_calls[0].parser_call_id.as_deref(),
            Some("Bash_0")
        );
        assert_eq!(parsed.main.tool_calls[0].duration_ms, Some(100));
        assert_eq!(parsed.main.tool_calls[1].status, ToolCallStatus::Error);
        assert_eq!(
            parsed.main.tool_calls[1].error_text.as_deref(),
            Some("denied")
        );
        assert_eq!(parsed.main.tool_calls[2].status, ToolCallStatus::Pending);
        assert_eq!(parsed.main.transcript_items.len(), 4);
    }

    #[test]
    fn unnamed_tool_calls_are_retained_and_match_later_results() {
        let root = write_bundle(
            state(),
            &[
                r#"{"type":"turn.prompt","input":[{"type":"text","text":"Run"}],"origin":{"kind":"user"}}"#,
                r#"{"type":"context.append_loop_event","event":{"type":"tool.call","toolCallId":"missing-name","args":{}}}"#,
                r#"{"type":"context.append_loop_event","event":{"type":"tool.result","toolCallId":"missing-name","result":{"output":"first"}}}"#,
                r#"{"type":"context.append_loop_event","event":{"type":"tool.call","toolCallId":"blank-name","name":"   ","args":{}}}"#,
                r#"{"type":"context.append_loop_event","event":{"type":"tool.result","toolCallId":"blank-name","result":{"output":"second"}}}"#,
            ],
        );
        let parsed = KimiCodeParser::new(root.path())
            .parse_session_dir(&session_dir(&root))
            .unwrap();

        assert_eq!(parsed.main.tool_calls.len(), 2);
        assert_eq!(parsed.main.tool_calls[0].tool_name, "unknown");
        assert_eq!(parsed.main.tool_calls[0].status, ToolCallStatus::Completed);
        assert_eq!(
            parsed.main.tool_calls[0].output_text.as_deref(),
            Some("first")
        );
        assert_eq!(parsed.main.tool_calls[1].tool_name, "unknown");
        assert_eq!(parsed.main.tool_calls[1].status, ToolCallStatus::Completed);
        assert_eq!(
            parsed.main.tool_calls[1].output_text.as_deref(),
            Some("second")
        );
        assert_eq!(parsed.main.transcript_items.len(), 3);
        assert_eq!(
            parsed.main.transcript_items[1].kind,
            TranscriptItemKind::ToolCall
        );
        assert_eq!(
            parsed.main.transcript_items[2].kind,
            TranscriptItemKind::ToolCall
        );
    }

    #[test]
    fn reasoning_attaches_to_a_tool_at_the_same_transcript_position() {
        let root = write_bundle(
            state(),
            &[
                r#"{"type":"turn.prompt","input":[{"type":"text","text":"Inspect"}],"origin":{"kind":"user"}}"#,
                r#"{"type":"context.append_loop_event","event":{"type":"content.part","stepUuid":"step-a","part":{"type":"think","think":"Need a file"}}}"#,
                r#"{"type":"context.append_loop_event","event":{"type":"tool.call","stepUuid":"step-a","toolCallId":"Read_0","name":"Read","args":{"path":"README.md"}}}"#,
            ],
        );
        let parsed = KimiCodeParser::new(root.path())
            .parse_session_dir(&session_dir(&root))
            .unwrap();
        assert_eq!(
            parsed.main.reasoning_attachments[0].transcript_item_index,
            1
        );
        assert_eq!(
            parsed.main.transcript_items[1].kind,
            TranscriptItemKind::ToolCall
        );
    }

    #[test]
    fn models_follow_steps_and_turn_usage_wins_without_double_counting() {
        let root = write_bundle(
            state(),
            &[
                r#"{"type":"turn.prompt","input":[{"type":"text","text":"Compare"}],"origin":{"kind":"user"}}"#,
                r#"{"type":"llm.request","model":"kimi-k2","modelAlias":"moonshot-ai/kimi-k2","turnStep":"0.1"}"#,
                r#"{"type":"context.append_loop_event","event":{"type":"step.begin","uuid":"step-1","turnId":"0","step":1}}"#,
                r#"{"type":"context.append_loop_event","event":{"type":"content.part","stepUuid":"step-1","part":{"type":"text","text":"Old model"}}}"#,
                r#"{"type":"config.update","modelAlias":"fallback-only"}"#,
                r#"{"type":"llm.request","model":"kimi-k3","modelAlias":"moonshot-ai/kimi-k3","turnStep":"0.2"}"#,
                r#"{"type":"context.append_loop_event","event":{"type":"step.begin","uuid":"step-2","turnId":"0","step":2}}"#,
                r#"{"type":"context.append_loop_event","event":{"type":"content.part","stepUuid":"step-2","part":{"type":"text","text":"New model"}}}"#,
                r#"{"type":"context.append_loop_event","event":{"type":"step.begin","uuid":"step-3","turnId":"0","step":3}}"#,
                r#"{"type":"context.append_loop_event","event":{"type":"content.part","stepUuid":"step-3","part":{"type":"text","text":"Fallback model"}}}"#,
                r#"{"type":"usage.record","usageScope":"turn","usage":{"inputOther":100,"output":20,"inputCacheRead":30,"inputCacheCreation":4}}"#,
                r#"{"type":"context.append_loop_event","event":{"type":"step.end","uuid":"step-2","usage":{"inputOther":999,"output":999,"inputCacheRead":999,"inputCacheCreation":999}}}"#,
            ],
        );
        let parsed = KimiCodeParser::new(root.path())
            .parse_session_dir(&session_dir(&root))
            .unwrap();

        let assistants: Vec<_> = parsed
            .main
            .messages
            .iter()
            .filter(|message| message.role == Role::Assistant)
            .collect();
        assert_eq!(assistants[0].model.as_deref(), Some("moonshot-ai/kimi-k2"));
        assert_eq!(assistants[1].model.as_deref(), Some("moonshot-ai/kimi-k3"));
        assert_eq!(assistants[2].model.as_deref(), Some("fallback-only"));
        assert_eq!(
            parsed.main.token_usage,
            Some(TokenUsage {
                input_tokens: 100,
                output_tokens: 20,
                cache_read_tokens: Some(30),
                cache_write_tokens: Some(4),
                reasoning_tokens: None,
            })
        );
    }

    #[test]
    fn unscoped_request_binds_to_only_the_next_step() {
        let root = write_bundle(
            state(),
            &[
                r#"{"type":"turn.prompt","input":[{"type":"text","text":"Model"}],"origin":{"kind":"user"}}"#,
                r#"{"type":"llm.request","modelAlias":"next-model"}"#,
                r#"{"type":"context.append_loop_event","event":{"type":"step.begin","uuid":"step-1","turnId":"0","step":1}}"#,
                r#"{"type":"context.append_loop_event","event":{"type":"content.part","stepUuid":"step-1","part":{"type":"text","text":"First"}}}"#,
                r#"{"type":"context.append_loop_event","event":{"type":"step.begin","uuid":"step-2","turnId":"0","step":2}}"#,
                r#"{"type":"context.append_loop_event","event":{"type":"content.part","stepUuid":"step-2","part":{"type":"text","text":"Second"}}}"#,
            ],
        );
        let parsed = KimiCodeParser::new(root.path())
            .parse_session_dir(&session_dir(&root))
            .unwrap();
        assert_eq!(parsed.main.messages[1].model.as_deref(), Some("next-model"));
        assert_eq!(parsed.main.messages[2].model, None);
    }

    #[test]
    fn step_usage_is_fallback_and_overflowing_record_is_rejected() {
        let root = write_bundle(
            state(),
            &[
                r#"{"type":"turn.prompt","input":[{"type":"text","text":"Usage"}],"origin":{"kind":"user"}}"#,
                r#"{"type":"context.append_loop_event","event":{"type":"step.end","uuid":"step-1","usage":{"inputOther":10,"output":2,"inputCacheRead":3,"inputCacheCreation":1}}}"#,
                r#"{"type":"context.append_loop_event","event":{"type":"step.end","uuid":"step-2","usage":{"inputOther":9223372036854775807,"output":2,"inputCacheRead":0,"inputCacheCreation":0}}}"#,
            ],
        );
        let parsed = KimiCodeParser::new(root.path())
            .parse_session_dir(&session_dir(&root))
            .unwrap();
        assert_eq!(parsed.main.token_usage.as_ref().unwrap().input_tokens, 10);
        assert_eq!(parsed.main.token_usage.as_ref().unwrap().output_tokens, 2);
    }

    #[test]
    fn direct_sibling_and_nested() {
        let mut metadata = state();
        metadata["agents"] = json!({
            "main": { "type": "main", "parentAgentId": null },
            "agent-0": { "type": "sub", "parentAgentId": "main" },
            "agent-1": { "type": "sub", "parentAgentId": "main" },
            "agent-nested": { "type": "sub", "parentAgentId": "agent-0" },
            "independent-0": { "type": "independent", "parentAgentId": null },
            "unknown-parent": { "type": "sub", "parentAgentId": "unknown" },
            "cycle-a": { "type": "sub", "parentAgentId": "cycle-b" },
            "cycle-b": { "type": "sub", "parentAgentId": "cycle-a" }
        });
        let root = write_bundle(
            metadata,
            &[
                r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Main"}],"origin":{"kind":"user"}}"#,
            ],
        );
        write_agent_journal(
            &root,
            "agent-0",
            &[
                r#"{"type":"turn.prompt","time":1785320000100,"input":[{"type":"text","text":"Child 0"}],"origin":{"kind":"user"}}"#,
            ],
        );
        write_agent_journal(
            &root,
            "agent-1",
            &[
                r#"{"type":"turn.prompt","time":1785320000200,"input":[{"type":"text","text":"Child 1"}],"origin":{"kind":"user"}}"#,
            ],
        );
        write_agent_journal(
            &root,
            "agent-nested",
            &[
                r#"{"type":"turn.prompt","time":1785320000300,"input":[{"type":"text","text":"Nested"}],"origin":{"kind":"user"}}"#,
            ],
        );

        let parsed = KimiCodeParser::new(root.path())
            .parse_session_dir(&session_dir(&root))
            .unwrap();

        assert_eq!(
            parsed
                .children
                .iter()
                .map(|child| child.session.id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "kimi-subagent::session_00000000-0000-4000-8000-000000000001::agent-0",
                "kimi-subagent::session_00000000-0000-4000-8000-000000000001::agent-1",
                "kimi-subagent::session_00000000-0000-4000-8000-000000000001::agent-nested",
            ]
        );
        assert_eq!(
            parsed.children[0].session.parent_session_id.as_deref(),
            Some(parsed.main.session.id.as_str())
        );
        assert_eq!(
            parsed.children[2].session.parent_session_id.as_deref(),
            Some(parsed.children[0].session.id.as_str())
        );
        assert!(
            parsed
                .children
                .iter()
                .all(|child| child.session.is_subagent)
        );
    }

    #[test]
    fn matched_agent_call() {
        let mut metadata = state();
        metadata["agents"] = json!({
            "main": { "type": "main", "parentAgentId": null },
            "agent-0": { "type": "sub", "parentAgentId": "main" }
        });
        let root = write_bundle(
            metadata,
            &[
                r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Main"}],"origin":{"kind":"user"}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000010,"event":{"type":"content.part","part":{"type":"text","text":"Planning"}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000020,"event":{"type":"tool.call","toolCallId":"Agent_0","name":"Agent","args":{"agent_id":"agent-0","subagent_type":"explore","prompt":"Inspect parser safety"}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000030,"event":{"type":"tool.result","toolCallId":"Agent_0","result":{"output":"done"}}}"#,
            ],
        );
        write_agent_journal(
            &root,
            "agent-0",
            &[
                r#"{"type":"turn.prompt","time":1785320000025,"input":[{"type":"text","text":"Child"}],"origin":{"kind":"user"}}"#,
            ],
        );

        let matched = KimiCodeParser::new(root.path())
            .parse_session_dir(&session_dir(&root))
            .unwrap()
            .main;

        assert_eq!(matched.subagents[0].agent_id.as_deref(), Some("agent-0"));
        assert_eq!(
            matched.subagents[0].prompt.as_deref(),
            Some("Inspect parser safety")
        );
        assert_eq!(
            matched.subagents[0].child_session_id.as_deref(),
            Some("kimi-subagent::session_00000000-0000-4000-8000-000000000001::agent-0")
        );
        assert_eq!(matched.subagents[0].parser_ref.as_deref(), Some("Agent_0"));
        assert!(
            matched
                .tool_calls
                .iter()
                .all(|call| call.parser_call_id.as_deref() != Some("Agent_0"))
        );
        assert_eq!(
            matched.transcript_items[2].kind,
            TranscriptItemKind::Subagent
        );
    }

    #[test]
    fn ambiguous_agent_calls() {
        let mut metadata = state();
        metadata["agents"] = json!({
            "main": { "type": "main", "parentAgentId": null },
            "agent-0": { "type": "sub", "parentAgentId": "main" },
            "agent-1": { "type": "sub", "parentAgentId": "main" }
        });
        let root = write_bundle(
            metadata,
            &[
                r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Main"}],"origin":{"kind":"user"}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000010,"event":{"type":"tool.call","toolCallId":"Agent_0","name":"Agent","args":{}}}"#,
                r#"{"type":"context.append_loop_event","event":{"type":"tool.call","toolCallId":"Agent_1","name":"Agent","args":{}}}"#,
            ],
        );
        write_agent_journal(
            &root,
            "agent-0",
            &[
                r#"{"type":"turn.prompt","time":1785320000020,"input":[{"type":"text","text":"Child 0"}],"origin":{"kind":"user"}}"#,
            ],
        );
        write_agent_journal(
            &root,
            "agent-1",
            &[
                r#"{"type":"turn.prompt","time":1785320000030,"input":[{"type":"text","text":"Child 1"}],"origin":{"kind":"user"}}"#,
            ],
        );

        let parsed = KimiCodeParser::new(root.path())
            .parse_session_dir(&session_dir(&root))
            .unwrap();

        assert_eq!(parsed.children.len(), 2);
        assert_eq!(parsed.main.subagents.len(), 0);
        assert!(
            parsed
                .main
                .tool_calls
                .iter()
                .all(|call| call.tool_name == "Agent")
        );
    }

    #[test]
    fn chronological_agent_matching_rejects_mixed_batch() {
        let mut metadata = state();
        metadata["agents"] = json!({
            "main": { "type": "main", "parentAgentId": null },
            "agent-0": { "type": "sub", "parentAgentId": "main" },
            "agent-1": { "type": "sub", "parentAgentId": "main" }
        });
        let root = write_bundle(
            metadata,
            &[
                r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Main"}],"origin":{"kind":"user"}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000010,"event":{"type":"tool.call","toolCallId":"Agent_0","name":"Agent","args":{}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000030,"event":{"type":"tool.call","toolCallId":"Agent_1","name":"Agent","args":{}}}"#,
            ],
        );
        write_agent_journal(
            &root,
            "agent-0",
            &[
                r#"{"type":"turn.prompt","time":1785320000020,"input":[{"type":"text","text":"Child 0"}],"origin":{"kind":"user"}}"#,
            ],
        );
        write_agent_journal(
            &root,
            "agent-1",
            &[
                r#"{"type":"turn.prompt","time":1785320000025,"input":[{"type":"text","text":"Child 1"}],"origin":{"kind":"user"}}"#,
            ],
        );

        let parsed = KimiCodeParser::new(root.path())
            .parse_session_dir(&session_dir(&root))
            .unwrap();

        assert_eq!(parsed.children.len(), 2);
        assert!(parsed.main.subagents.is_empty());
        assert_eq!(parsed.main.tool_calls.len(), 2);
        assert!(
            parsed
                .main
                .tool_calls
                .iter()
                .all(|call| call.tool_name == "Agent")
        );
    }

    #[test]
    fn result_resume_does_not_match_agent() {
        let mut metadata = state();
        metadata["agents"] = json!({
            "main": { "type": "main", "parentAgentId": null },
            "agent-0": { "type": "sub", "parentAgentId": "main" }
        });
        let root = write_bundle(
            metadata,
            &[
                r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Main"}],"origin":{"kind":"user"}}"#,
                r#"{"type":"context.append_loop_event","event":{"type":"tool.call","toolCallId":"Agent_0","name":"Agent","args":{}}}"#,
                r#"{"type":"context.append_loop_event","event":{"type":"tool.result","toolCallId":"Agent_0","result":{"resume":"agent-0"}}}"#,
            ],
        );
        write_agent_journal(
            &root,
            "agent-0",
            &[
                r#"{"type":"turn.prompt","time":1785320000020,"input":[{"type":"text","text":"Child"}],"origin":{"kind":"user"}}"#,
            ],
        );

        let parsed = KimiCodeParser::new(root.path())
            .parse_session_dir(&session_dir(&root))
            .unwrap();

        assert_eq!(parsed.children.len(), 1);
        assert!(parsed.main.subagents.is_empty());
        assert_eq!(
            parsed.main.tool_calls[0].parser_call_id.as_deref(),
            Some("Agent_0")
        );
    }

    #[test]
    fn duplicate_explicit_agent_id_does_not_fall_back_to_another_child() {
        let mut metadata = state();
        metadata["agents"] = json!({
            "main": { "type": "main", "parentAgentId": null },
            "agent-0": { "type": "sub", "parentAgentId": "main" },
            "agent-1": { "type": "sub", "parentAgentId": "main" }
        });
        let root = write_bundle(
            metadata,
            &[
                r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Main"}],"origin":{"kind":"user"}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000010,"event":{"type":"tool.call","toolCallId":"Agent_0","name":"Agent","args":{"agent_id":"agent-0"}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000020,"event":{"type":"tool.call","toolCallId":"Agent_1","name":"Agent","args":{"agent_id":"agent-0"}}}"#,
            ],
        );
        write_agent_journal(
            &root,
            "agent-0",
            &[
                r#"{"type":"turn.prompt","time":1785320000030,"input":[{"type":"text","text":"Child 0"}],"origin":{"kind":"user"}}"#,
            ],
        );
        write_agent_journal(
            &root,
            "agent-1",
            &[
                r#"{"type":"turn.prompt","time":1785320000040,"input":[{"type":"text","text":"Child 1"}],"origin":{"kind":"user"}}"#,
            ],
        );

        let parsed = KimiCodeParser::new(root.path())
            .parse_session_dir(&session_dir(&root))
            .unwrap();

        assert_eq!(parsed.main.subagents.len(), 1);
        assert_eq!(
            parsed.main.subagents[0].agent_id.as_deref(),
            Some("agent-0")
        );
        assert!(parsed.main.tool_calls.iter().any(|call| {
            call.parser_call_id.as_deref() == Some("Agent_1") && call.tool_name == "Agent"
        }));
    }

    #[test]
    fn conflicting_structured_agent_evidence_remains_generic() {
        let mut metadata = state();
        metadata["agents"] = json!({
            "main": { "type": "main", "parentAgentId": null },
            "agent-0": { "type": "sub", "parentAgentId": "main" }
        });
        let root = write_bundle(
            metadata,
            &[
                r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Main"}],"origin":{"kind":"user"}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000010,"event":{"type":"tool.call","toolCallId":"Agent_0","name":"Agent","args":{"agent_id":"agent-0"}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000020,"event":{"type":"tool.result","toolCallId":"Agent_0","result":{"agent_id":"unknown-agent","output":"done"}}}"#,
            ],
        );
        write_agent_journal(
            &root,
            "agent-0",
            &[
                r#"{"type":"turn.prompt","time":1785320000030,"input":[{"type":"text","text":"Child"}],"origin":{"kind":"user"}}"#,
            ],
        );

        let parsed = KimiCodeParser::new(root.path())
            .parse_session_dir(&session_dir(&root))
            .unwrap();

        assert!(parsed.main.subagents.is_empty());
        assert_eq!(parsed.main.tool_calls[0].tool_name, "Agent");
    }

    #[test]
    fn conflicting_structured_and_text_agent_evidence_remains_generic() {
        let mut metadata = state();
        metadata["agents"] = json!({
            "main": { "type": "main", "parentAgentId": null },
            "agent-0": { "type": "sub", "parentAgentId": "main" },
            "agent-1": { "type": "sub", "parentAgentId": "main" }
        });
        let root = write_bundle(
            metadata,
            &[
                r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Main"}],"origin":{"kind":"user"}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000010,"event":{"type":"tool.call","toolCallId":"Agent_0","name":"Agent","args":{"agent_id":"agent-0"}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000020,"event":{"type":"tool.result","toolCallId":"Agent_0","result":{"output":"agent_id: agent-1"}}}"#,
            ],
        );
        write_agent_journal(
            &root,
            "agent-0",
            &[
                r#"{"type":"turn.prompt","time":1785320000030,"input":[{"type":"text","text":"Child 0"}],"origin":{"kind":"user"}}"#,
            ],
        );
        write_agent_journal(
            &root,
            "agent-1",
            &[
                r#"{"type":"turn.prompt","time":1785320000040,"input":[{"type":"text","text":"Child 1"}],"origin":{"kind":"user"}}"#,
            ],
        );

        let parsed = KimiCodeParser::new(root.path())
            .parse_session_dir(&session_dir(&root))
            .unwrap();

        assert!(parsed.main.subagents.is_empty());
        assert_eq!(parsed.main.tool_calls[0].tool_name, "Agent");
    }

    #[test]
    fn null_structured_and_valid_text_agent_evidence_remains_generic() {
        let parsed = parse_single_agent_call(&[
            r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Main"}],"origin":{"kind":"user"}}"#,
            r#"{"type":"context.append_loop_event","time":1785320000010,"event":{"type":"tool.call","toolCallId":"Agent_0","name":"Agent","args":{"agent_id":null}}}"#,
            r#"{"type":"context.append_loop_event","time":1785320000020,"event":{"type":"tool.result","toolCallId":"Agent_0","result":{"output":"agent_id: agent-0"}}}"#,
        ]);

        assert!(parsed.subagents.is_empty());
        assert_eq!(parsed.tool_calls[0].tool_name, "Agent");
    }

    #[test]
    fn blank_structured_and_valid_text_agent_evidence_remains_generic() {
        let parsed = parse_single_agent_call(&[
            r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Main"}],"origin":{"kind":"user"}}"#,
            r#"{"type":"context.append_loop_event","time":1785320000010,"event":{"type":"tool.call","toolCallId":"Agent_0","name":"Agent","args":{"agent_id":"  "}}}"#,
            r#"{"type":"context.append_loop_event","time":1785320000020,"event":{"type":"tool.result","toolCallId":"Agent_0","result":{"output":"agent_id: agent-0"}}}"#,
        ]);

        assert!(parsed.subagents.is_empty());
        assert_eq!(parsed.tool_calls[0].tool_name, "Agent");
    }

    #[test]
    fn unknown_and_valid_agent_evidence_remains_generic() {
        let parsed = parse_single_agent_call(&[
            r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Main"}],"origin":{"kind":"user"}}"#,
            r#"{"type":"context.append_loop_event","time":1785320000010,"event":{"type":"tool.call","toolCallId":"Agent_0","name":"Agent","args":{"agent_id":"unknown-agent"}}}"#,
            r#"{"type":"context.append_loop_event","time":1785320000020,"event":{"type":"tool.result","toolCallId":"Agent_0","result":{"output":"agent_id: agent-0"}}}"#,
        ]);

        assert!(parsed.subagents.is_empty());
        assert_eq!(parsed.tool_calls[0].tool_name, "Agent");
    }

    #[test]
    fn all_valid_same_agent_evidence_matches_child() {
        let parsed = parse_single_agent_call(&[
            r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Main"}],"origin":{"kind":"user"}}"#,
            r#"{"type":"context.append_loop_event","time":1785320000010,"event":{"type":"tool.call","toolCallId":"Agent_0","name":"Agent","args":{"agent_id":"agent-0"}}}"#,
            r#"{"type":"context.append_loop_event","time":1785320000020,"event":{"type":"tool.result","toolCallId":"Agent_0","result":{"agentId":"agent-0","output":"agent_id: agent-0"}}}"#,
        ]);

        assert_eq!(parsed.subagents.len(), 1);
        assert_eq!(parsed.subagents[0].agent_id.as_deref(), Some("agent-0"));
        assert!(parsed.tool_calls.is_empty());
    }
}
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Duration, Utc};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Component, Path, PathBuf};

use crate::models::{
    AiAssistant, Message, Role, Session, Subagent, TokenUsage, ToolCall, ToolCallStatus,
    TranscriptItem, TranscriptItemKind,
};
use crate::parsers::{ParsedSession, PendingReasoning, extract_first_prompt};

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("Session contains no user messages")]
    NoUserMessages,
    #[error("Kimi bundle path is not a {expected}: {path}")]
    InvalidBundlePath {
        path: PathBuf,
        expected: &'static str,
    },
}

impl ParseError {
    pub(crate) fn invalid_path(&self) -> Option<&Path> {
        match self {
            Self::InvalidBundlePath { path, .. } => Some(path),
            Self::NoUserMessages => None,
        }
    }
}

#[derive(Debug)]
pub struct KimiParsedBundle {
    pub main: ParsedSession,
    pub children: Vec<ParsedSession>,
    pub session_ids: HashSet<String>,
}

#[derive(Debug, Default)]
pub struct KimiCodeParser {
    kimi_home: PathBuf,
    session_work_dirs: HashMap<String, String>,
    workspace_roots: HashMap<String, String>,
}

#[derive(Debug, Default)]
struct StateMetadata {
    id: Option<String>,
    title: Option<String>,
    work_dir: Option<String>,
    cwd: Option<String>,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
    agents: BTreeMap<String, AgentMetadata>,
}

#[derive(Debug, Default)]
struct AgentMetadata {
    kind: Option<String>,
    parent_agent_id: Option<String>,
}

#[derive(Debug)]
struct JournalScan {
    has_real_turn_prompt: bool,
    earliest_time: Option<DateTime<Utc>>,
    latest_time: Option<DateTime<Utc>>,
}

#[derive(Debug)]
struct ParsedJournal {
    parsed: ParsedSession,
    first_wire_time_ms: Option<i64>,
    agent_calls: Vec<AgentCallCandidate>,
}

#[derive(Debug, Clone)]
struct AgentCallCandidate {
    raw_call_id: String,
    transcript_item_index: i64,
    call_time_ms: Option<i64>,
    args: Option<Value>,
    result: Option<Value>,
    result_text: Option<String>,
}

#[derive(Debug, Clone)]
struct ResolvedAgent {
    id: String,
    parent_id: String,
    depth: usize,
    journal_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VisitState {
    Visiting,
    Resolved,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AgentEvidence {
    Absent,
    Valid(String),
    Invalid,
}

#[derive(Debug, Clone)]
struct StepState {
    turn_step: Option<String>,
    model: Option<String>,
}

#[derive(Debug, Default, Clone, Copy)]
struct RawUsage {
    input: i64,
    output: i64,
    cache_read: i64,
    cache_write: i64,
}

#[derive(Debug, Default)]
struct UsageAccumulator {
    total: RawUsage,
    seen: bool,
}

#[derive(Debug, Default)]
struct JournalState {
    pending_calls: HashMap<String, usize>,
    call_start_times: HashMap<String, i64>,
    step_by_uuid: HashMap<String, StepState>,
    step_uuid_by_turn_step: HashMap<String, String>,
    pending_models_by_turn_step: HashMap<String, VecDeque<String>>,
    unscoped_models: VecDeque<String>,
    active_model_hint: Option<String>,
    turn_usage: UsageAccumulator,
    step_usage: UsageAccumulator,
    agent_calls: Vec<AgentCallCandidate>,
}

impl KimiCodeParser {
    pub fn new(kimi_home: &Path) -> Self {
        let mut parser = Self {
            kimi_home: kimi_home.to_path_buf(),
            ..Self::default()
        };
        if !fs::symlink_metadata(kimi_home)
            .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
        {
            return parser;
        }
        parser.load_session_index(&kimi_home.join("session_index.jsonl"));
        parser.load_workspaces(&kimi_home.join("workspaces.json"));
        parser
    }

    pub fn dependency_paths(&self, session_dir: &Path) -> Result<Vec<PathBuf>> {
        let state_path = session_dir.join("state.json");
        validate_bundle_path(&self.kimi_home, session_dir, &state_path)?;
        validate_expected_file(&state_path)?;
        let state = load_state(&state_path)?;
        let mut files = vec![state_path, session_dir.join("agents/main/wire.jsonl")];
        let mut directories = vec![session_dir.join("agents"), session_dir.join("agents/main")];

        let resolved_agents = resolve_agents(&state, session_dir);
        if !state.agents.contains_key("main") {
            tracing::warn!(path = %session_dir.join("state.json").display(), "Kimi session state has no main agent");
        }
        for agent in resolved_agents {
            let Some(agent_dir) = agent.journal_path.parent() else {
                continue;
            };
            directories.push(agent_dir.to_path_buf());
            files.push(agent.journal_path);
        }

        for path in &directories {
            validate_expected_directory(path)?;
            validate_bundle_path(&self.kimi_home, session_dir, path)?;
        }
        for path in &files {
            validate_expected_file(path)?;
            validate_bundle_path(&self.kimi_home, session_dir, path)?;
        }
        let mut paths = directories;
        paths.extend(files);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }

    pub fn parse_session_dir(&self, session_dir: &Path) -> Result<KimiParsedBundle> {
        let session_id = canonical_session_id(session_dir)?;
        let state_path = session_dir.join("state.json");
        validate_bundle_path(&self.kimi_home, session_dir, &state_path)?;
        validate_expected_file(&state_path)?;
        let state = load_state(&state_path)?;
        let resolved_agents = resolve_agents(&state, session_dir);
        for path in [session_dir.join("agents"), session_dir.join("agents/main")] {
            validate_expected_directory(&path)?;
            validate_bundle_path(&self.kimi_home, session_dir, &path)?;
        }
        if state.id.as_deref().is_some_and(|id| id != session_id) {
            tracing::warn!(path = %state_path.display(), "Kimi state identity differs from directory identity");
        }
        if state.work_dir.is_some() && state.cwd.is_some() && state.work_dir != state.cwd {
            tracing::warn!(path = %state_path.display(), "Kimi state work directories differ");
        }

        let project_path = state
            .work_dir
            .clone()
            .or_else(|| state.cwd.clone())
            .or_else(|| self.session_work_dirs.get(&session_id).cloned())
            .or_else(|| {
                workspace_key(session_dir)
                    .and_then(|key| self.workspace_roots.get(key))
                    .cloned()
            });

        let journal_path = session_dir.join("agents/main/wire.jsonl");
        validate_expected_file(&journal_path)?;
        validate_bundle_path(&self.kimi_home, session_dir, &journal_path)?;
        let scan = scan_journal(&journal_path)?;
        let start_time = state
            .created_at
            .or(scan.earliest_time)
            .unwrap_or_else(|| DateTime::<Utc>::from_timestamp(0, 0).expect("epoch is valid"));
        let last_updated = state
            .updated_at
            .max(scan.latest_time)
            .unwrap_or(start_time)
            .max(start_time);
        let ParsedJournal {
            mut parsed,
            first_wire_time_ms: _,
            agent_calls: main_agent_calls,
        } = parse_journal(
            &journal_path,
            &session_id,
            start_time,
            scan.has_real_turn_prompt,
        )?;
        let title = state
            .title
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty() && *title != "New Session")
            .map(str::to_string)
            .or_else(|| extract_first_prompt(&parsed.messages));

        parsed.session = Session {
            id: session_id.clone(),
            tool: AiAssistant::KimiCode,
            project_path,
            project_id: None,
            start_time,
            message_count: parsed.messages.len(),
            file_path: session_dir.display().to_string(),
            last_updated,
            pinned_at: None,
            first_prompt: title,
            parent_session_id: None,
            is_subagent: false,
            token_usage: None,
            edit_count: 0,
            read_count: 0,
            command_count: 0,
            ending_status: crate::models::SessionEndingStatus::Unknown,
        };

        struct ParsedChild {
            agent: ResolvedAgent,
            parsed: ParsedSession,
            first_wire_time_ms: Option<i64>,
            agent_calls: Vec<AgentCallCandidate>,
        }

        let mut children = Vec::new();
        let mut omitted_agents = HashSet::new();
        for agent in resolved_agents {
            if agent.parent_id != "main" && omitted_agents.contains(&agent.parent_id) {
                omitted_agents.insert(agent.id);
                continue;
            }
            if let Some(agent_dir) = agent.journal_path.parent() {
                validate_expected_directory(agent_dir)?;
                validate_bundle_path(&self.kimi_home, session_dir, agent_dir)?;
            }
            validate_expected_file(&agent.journal_path)?;
            validate_bundle_path(&self.kimi_home, session_dir, &agent.journal_path)?;
            let scan = scan_journal(&agent.journal_path)?;
            let child_start = scan
                .earliest_time
                .or(state.created_at)
                .unwrap_or(start_time);
            let ParsedJournal {
                parsed: mut child,
                first_wire_time_ms,
                agent_calls,
            } = match parse_journal(
                &agent.journal_path,
                &child_session_id(&session_id, &agent.id),
                child_start,
                scan.has_real_turn_prompt,
            ) {
                Ok(parsed) => parsed,
                Err(error)
                    if matches!(
                        error.downcast_ref::<ParseError>(),
                        Some(ParseError::NoUserMessages)
                    ) =>
                {
                    omitted_agents.insert(agent.id);
                    continue;
                }
                Err(error) => return Err(error),
            };
            let child_id = child_session_id(&session_id, &agent.id);
            let parent_session_id = if agent.parent_id == "main" {
                session_id.clone()
            } else {
                child_session_id(&session_id, &agent.parent_id)
            };
            child.session = Session {
                id: child_id,
                tool: AiAssistant::KimiCode,
                project_path: parsed.session.project_path.clone(),
                project_id: None,
                start_time: child_start,
                message_count: child.messages.len(),
                file_path: agent.journal_path.display().to_string(),
                last_updated: scan
                    .latest_time
                    .unwrap_or(child_start)
                    .max(state.updated_at.unwrap_or(child_start))
                    .max(child_start),
                pinned_at: None,
                first_prompt: extract_first_prompt(&child.messages),
                parent_session_id: Some(parent_session_id),
                is_subagent: true,
                token_usage: None,
                edit_count: 0,
                read_count: 0,
                command_count: 0,
                ending_status: crate::models::SessionEndingStatus::Unknown,
            };
            children.push(ParsedChild {
                agent,
                parsed: child,
                first_wire_time_ms,
                agent_calls,
            });
        }

        let child_infos: Vec<_> = children
            .iter()
            .map(|child| {
                (
                    child.agent.id.clone(),
                    child.agent.parent_id.clone(),
                    child.parsed.session.id.clone(),
                    child.first_wire_time_ms,
                )
            })
            .collect();
        let main_children: Vec<_> = child_infos
            .iter()
            .filter(|(_, parent_id, _, _)| parent_id == "main")
            .map(|(id, _, session_id, time)| (id.as_str(), session_id.as_str(), *time))
            .collect();
        match_agent_calls(&mut parsed, &main_agent_calls, &main_children);
        for child in &mut children {
            let parent_id = child.agent.id.clone();
            let agent_calls = child.agent_calls.clone();
            let immediate_children: Vec<_> = child_infos
                .iter()
                .filter(|(_, child_parent_id, _, _)| child_parent_id == &parent_id)
                .map(|(id, _, session_id, time)| (id.as_str(), session_id.as_str(), *time))
                .collect();
            match_agent_calls(&mut child.parsed, &agent_calls, &immediate_children);
        }
        let children: Vec<_> = children.into_iter().map(|child| child.parsed).collect();
        let session_ids = std::iter::once(session_id.clone())
            .chain(children.iter().map(|child| child.session.id.clone()))
            .collect();

        Ok(KimiParsedBundle {
            main: parsed,
            children,
            session_ids,
        })
    }

    fn load_session_index(&mut self, path: &Path) {
        if !validate_optional_metadata_file(path) {
            return;
        }
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(_) => {
                tracing::warn!(path = %path.display(), "failed to read optional Kimi metadata");
                return;
            }
        };
        for line in BufReader::new(file).lines() {
            let Ok(line) = line else {
                tracing::warn!(path = %path.display(), "failed to read Kimi session index line");
                continue;
            };
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                tracing::warn!(path = %path.display(), "skipping malformed Kimi session index line");
                continue;
            };
            let Some(id) = value
                .get("sessionId")
                .and_then(Value::as_str)
                .filter(|id| !id.trim().is_empty())
            else {
                continue;
            };
            if value.get("deleted").and_then(Value::as_bool) == Some(true) {
                self.session_work_dirs.remove(id);
                continue;
            }
            if let Some(work_dir) = nonblank(value.get("workDir")) {
                self.session_work_dirs.insert(id.to_string(), work_dir);
            }
        }
    }

    fn load_workspaces(&mut self, path: &Path) {
        if !validate_optional_metadata_file(path) {
            return;
        }
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(_) => {
                tracing::warn!(path = %path.display(), "failed to read optional Kimi metadata");
                return;
            }
        };
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
            tracing::warn!(path = %path.display(), "ignoring malformed Kimi workspaces metadata");
            return;
        };
        let Some(workspaces) = value.get("workspaces").and_then(Value::as_object) else {
            return;
        };
        for (key, workspace) in workspaces {
            if let Some(root) = nonblank(workspace.get("root")) {
                self.workspace_roots.insert(key.clone(), root);
            }
        }
    }
}

fn validate_optional_metadata_file(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Ok(_) => {
            tracing::warn!(path = %path.display(), "ignoring non-regular optional Kimi metadata");
            false
        }
        Err(_) => {
            tracing::warn!(path = %path.display(), "failed to inspect optional Kimi metadata");
            false
        }
    }
}

pub(crate) fn validate_bundle_path(
    kimi_home: &Path,
    session_dir: &Path,
    path: &Path,
) -> Result<()> {
    let session_relative = session_dir.strip_prefix(kimi_home).with_context(|| {
        format!(
            "Kimi bundle is outside configured home {}",
            kimi_home.display()
        )
    })?;
    let relative = path
        .strip_prefix(session_dir)
        .with_context(|| format!("path is outside Kimi bundle {}", session_dir.display()))?;
    let home_metadata = fs::symlink_metadata(kimi_home)
        .with_context(|| format!("failed to inspect {}", kimi_home.display()))?;
    if home_metadata.file_type().is_symlink() {
        bail!(
            "Kimi bundle path contains a symlink: {}",
            kimi_home.display()
        );
    }

    let mut current = kimi_home.to_path_buf();
    for component in session_relative.components().chain(relative.components()) {
        let Component::Normal(component) = component else {
            bail!("Kimi bundle path is unsafe: {}", path.display());
        };
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!("Kimi bundle path contains a symlink: {}", current.display());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to inspect {}", current.display()));
            }
        }
    }
    Ok(())
}

fn validate_expected_file(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| anyhow::anyhow!("failed to inspect {}: {error}", path.display()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(ParseError::InvalidBundlePath {
            path: path.to_path_buf(),
            expected: "regular file",
        }
        .into());
    }
    Ok(())
}

fn validate_expected_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| anyhow::anyhow!("failed to inspect {}: {error}", path.display()))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(ParseError::InvalidBundlePath {
            path: path.to_path_buf(),
            expected: "directory",
        }
        .into());
    }
    Ok(())
}

fn nonblank(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn normalized_model(value: Option<&Value>) -> Option<String> {
    let raw = value?.as_str()?;
    crate::parsers::model::normalize_model(Some(&Value::String(raw.to_string())))
}

fn request_model(record: &Value) -> Option<String> {
    normalized_model(record.get("modelAlias")).or_else(|| normalized_model(record.get("model")))
}

fn normalize_tool_output(value: Option<&Value>) -> Option<String> {
    let value = value?;
    if let Some(text) = value.as_str() {
        return (!text.trim().is_empty()).then(|| text.to_string());
    }
    normalize_content_parts(Some(value), true)
}

fn parse_usage(value: Option<&Value>) -> Option<RawUsage> {
    let usage = value?;
    let required = |name: &str| {
        usage
            .get(name)
            .and_then(Value::as_i64)
            .filter(|value| *value >= 0)
    };
    let optional = |name: &str| match usage.get(name) {
        None => Some(0),
        Some(value) => value.as_i64().filter(|value| *value >= 0),
    };
    Some(RawUsage {
        input: required("inputOther")?,
        output: required("output")?,
        cache_read: optional("inputCacheRead")?,
        cache_write: optional("inputCacheCreation")?,
    })
}

impl UsageAccumulator {
    fn add(&mut self, value: RawUsage) {
        let Some(input) = self.total.input.checked_add(value.input) else {
            return;
        };
        let Some(output) = self.total.output.checked_add(value.output) else {
            return;
        };
        let Some(cache_read) = self.total.cache_read.checked_add(value.cache_read) else {
            return;
        };
        let Some(cache_write) = self.total.cache_write.checked_add(value.cache_write) else {
            return;
        };
        self.total = RawUsage {
            input,
            output,
            cache_read,
            cache_write,
        };
        self.seen = true;
    }

    fn into_token_usage(self) -> Option<TokenUsage> {
        self.seen.then_some(TokenUsage {
            input_tokens: self.total.input,
            output_tokens: self.total.output,
            cache_read_tokens: Some(self.total.cache_read),
            cache_write_tokens: Some(self.total.cache_write),
            reasoning_tokens: None,
        })
    }
}

fn parse_metadata_timestamp(value: Option<&Value>) -> Option<DateTime<Utc>> {
    let value = value?;
    if let Some(raw) = value.as_str() {
        return DateTime::parse_from_rfc3339(raw)
            .ok()
            .map(|time| time.with_timezone(&Utc));
    }
    value
        .as_i64()
        .and_then(DateTime::<Utc>::from_timestamp_millis)
}

fn parse_wire_time(value: Option<&Value>) -> Option<(i64, DateTime<Utc>)> {
    let millis = value?.as_i64()?;
    DateTime::<Utc>::from_timestamp_millis(millis).map(|time| (millis, time))
}

fn canonical_session_id(session_dir: &Path) -> Result<String> {
    let id = session_dir
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|id| id.starts_with("session_") && id.len() > "session_".len())
        .context("Kimi session directory has no valid session_ identity")?;
    Ok(id.to_string())
}

fn workspace_key(session_dir: &Path) -> Option<&str> {
    session_dir.parent()?.file_name()?.to_str()
}

fn valid_agent_id(agent_id: &str) -> bool {
    !agent_id.is_empty()
        && !agent_id.contains("::")
        && !agent_id.contains('\0')
        && Path::new(agent_id).components().count() == 1
        && matches!(
            Path::new(agent_id).components().next(),
            Some(Component::Normal(_))
        )
}

fn child_session_id(main_session_id: &str, agent_id: &str) -> String {
    format!("kimi-subagent::{main_session_id}::{agent_id}")
}

fn resolve_agents(state: &StateMetadata, session_dir: &Path) -> Vec<ResolvedAgent> {
    let mut visits = HashMap::new();
    let mut resolved = HashMap::new();
    for agent_id in state.agents.keys() {
        resolve_agent(agent_id, state, session_dir, &mut visits, &mut resolved, 0);
    }
    let mut agents: Vec<_> = resolved.into_values().collect();
    agents.sort_by(|left, right| (left.depth, &left.id).cmp(&(right.depth, &right.id)));
    agents
}

fn resolve_agent(
    agent_id: &str,
    state: &StateMetadata,
    session_dir: &Path,
    visits: &mut HashMap<String, VisitState>,
    resolved: &mut HashMap<String, ResolvedAgent>,
    traversed: usize,
) -> Option<usize> {
    if traversed >= state.agents.len() {
        visits.insert(agent_id.to_string(), VisitState::Rejected);
        return None;
    }
    match visits.get(agent_id) {
        Some(VisitState::Resolved) => return resolved.get(agent_id).map(|agent| agent.depth),
        Some(VisitState::Rejected) => return None,
        Some(VisitState::Visiting) => {
            visits.insert(agent_id.to_string(), VisitState::Rejected);
            return None;
        }
        None => {}
    }
    let agent = state.agents.get(agent_id)?;
    if agent.kind.as_deref() != Some("sub") {
        visits.insert(agent_id.to_string(), VisitState::Rejected);
        return None;
    }
    let parent_id = agent.parent_agent_id.as_deref()?;
    visits.insert(agent_id.to_string(), VisitState::Visiting);
    let parent_depth = if parent_id == "main" {
        match state.agents.get("main") {
            Some(main) if main.kind.as_deref() == Some("main") => Some(0),
            _ => None,
        }
    } else {
        resolve_agent(
            parent_id,
            state,
            session_dir,
            visits,
            resolved,
            traversed + 1,
        )
        .map(|depth| depth + 1)
    };
    let Some(depth) = parent_depth else {
        visits.insert(agent_id.to_string(), VisitState::Rejected);
        return None;
    };
    let resolved_agent = ResolvedAgent {
        id: agent_id.to_string(),
        parent_id: parent_id.to_string(),
        depth,
        journal_path: session_dir.join("agents").join(agent_id).join("wire.jsonl"),
    };
    visits.insert(agent_id.to_string(), VisitState::Resolved);
    resolved.insert(agent_id.to_string(), resolved_agent);
    Some(depth)
}

fn merge_agent_evidence(evidence: &mut AgentEvidence, id: Option<&str>) {
    let Some(id) = id.map(str::trim).filter(|id| !id.is_empty()) else {
        *evidence = AgentEvidence::Invalid;
        return;
    };
    match evidence {
        AgentEvidence::Absent => *evidence = AgentEvidence::Valid(id.to_string()),
        AgentEvidence::Valid(existing) if existing == id => {}
        AgentEvidence::Valid(_) | AgentEvidence::Invalid => *evidence = AgentEvidence::Invalid,
    }
}

fn agent_evidence(candidate: &AgentCallCandidate) -> AgentEvidence {
    let mut evidence = AgentEvidence::Absent;
    for (value, keys) in [
        (
            candidate.args.as_ref(),
            &["agent_id", "agentId", "resume"][..],
        ),
        (candidate.result.as_ref(), &["agent_id", "agentId"][..]),
    ] {
        let Some(value) = value else {
            continue;
        };
        for key in keys {
            if let Some(raw) = value.get(key) {
                merge_agent_evidence(&mut evidence, raw.as_str());
            }
        }
    }
    if let Some(text) = candidate.result_text.as_deref() {
        for line in text.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            if matches!(key, "agent_id" | "agentId") {
                let value = value.trim();
                if valid_agent_id(value) {
                    merge_agent_evidence(&mut evidence, Some(value));
                } else {
                    evidence = AgentEvidence::Invalid;
                }
            }
        }
    }
    evidence
}

fn match_agent_calls(
    parent: &mut ParsedSession,
    candidates: &[AgentCallCandidate],
    children: &[(&str, &str, Option<i64>)],
) {
    let mut matched = HashMap::new();
    let mut used_children = HashSet::new();
    let evidence: HashMap<_, _> = candidates
        .iter()
        .map(|candidate| (candidate.raw_call_id.as_str(), agent_evidence(candidate)))
        .collect();
    for candidate in candidates {
        if let AgentEvidence::Valid(id) = &evidence[candidate.raw_call_id.as_str()]
            && children.iter().any(|(child_id, _, _)| *child_id == id)
            && used_children.insert(id.clone())
        {
            matched.insert(candidate.raw_call_id.clone(), id.clone());
        }
    }
    let remaining_calls: Vec<_> = candidates
        .iter()
        .filter(|candidate| {
            !matched.contains_key(&candidate.raw_call_id)
                && evidence[candidate.raw_call_id.as_str()] == AgentEvidence::Absent
        })
        .collect();
    let mut remaining_children: Vec<_> = children
        .iter()
        .filter(|(id, _, _)| !used_children.contains(*id))
        .copied()
        .collect();
    if remaining_calls.len() == remaining_children.len()
        && remaining_calls
            .iter()
            .all(|candidate| candidate.call_time_ms.is_some())
        && remaining_children.iter().all(|(_, _, time)| time.is_some())
    {
        let mut ordered_calls = remaining_calls;
        ordered_calls
            .sort_by_key(|candidate| (candidate.call_time_ms, candidate.raw_call_id.as_str()));
        remaining_children.sort_by_key(|(id, _, time)| (*time, *id));
        if ordered_calls
            .iter()
            .zip(&remaining_children)
            .all(|(candidate, (_, _, child_time))| {
                child_time.expect("verified") >= candidate.call_time_ms.expect("verified")
            })
        {
            for (candidate, (agent_id, _, _)) in ordered_calls.into_iter().zip(remaining_children) {
                matched.insert(candidate.raw_call_id.clone(), agent_id.to_string());
            }
        }
    }

    let mut removed_tool_ids = HashSet::new();
    for candidate in candidates {
        let Some(agent_id) = matched.get(&candidate.raw_call_id) else {
            continue;
        };
        let Some((_, child_session_id, _)) = children.iter().find(|(id, _, _)| *id == agent_id)
        else {
            continue;
        };
        let subagent_id = format!(
            "kimi-agent-call::{}::{}",
            parent.session.id, candidate.raw_call_id
        );
        if let Some(item) = parent
            .transcript_items
            .iter_mut()
            .find(|item| item.item_index == candidate.transcript_item_index)
        {
            item.kind = TranscriptItemKind::Subagent;
            item.tool_call_id = None;
            item.subagent_id = Some(subagent_id.clone());
        }
        parent.subagents.push(Subagent {
            id: subagent_id,
            agent_id: Some(agent_id.clone()),
            agent_name: candidate
                .args
                .as_ref()
                .and_then(|value| value.get("subagent_type"))
                .and_then(Value::as_str)
                .map(str::to_string),
            session_id: parent.session.id.clone(),
            title: agent_id.clone(),
            prompt: candidate
                .args
                .as_ref()
                .and_then(|value| value.get("prompt"))
                .and_then(Value::as_str)
                .map(str::to_string),
            result_summary: candidate.result_text.clone(),
            child_session_id: Some((*child_session_id).to_string()),
            parser_ref: Some(candidate.raw_call_id.clone()),
        });
        removed_tool_ids.insert(format!(
            "kimi-tool::{}::{}",
            parent.session.id, candidate.raw_call_id
        ));
    }
    parent
        .tool_calls
        .retain(|call| !removed_tool_ids.contains(&call.id));
}

fn load_state(path: &Path) -> Result<StateMetadata> {
    let bytes =
        std::fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let value = serde_json::from_slice::<Value>(&bytes)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    let mut state = StateMetadata {
        id: nonblank(value.get("id")),
        title: nonblank(value.get("title")),
        work_dir: nonblank(value.get("workDir")),
        cwd: nonblank(value.get("cwd")),
        created_at: parse_metadata_timestamp(value.get("createdAt")),
        updated_at: parse_metadata_timestamp(value.get("updatedAt")),
        agents: BTreeMap::new(),
    };
    if let Some(agents) = value.get("agents").and_then(Value::as_object) {
        for (agent_id, agent) in agents {
            if !valid_agent_id(agent_id) {
                tracing::warn!(path = %path.display(), "ignoring invalid Kimi agent identifier");
                continue;
            }
            state.agents.insert(
                agent_id.clone(),
                AgentMetadata {
                    kind: nonblank(agent.get("type")),
                    parent_agent_id: nonblank(agent.get("parentAgentId")),
                },
            );
        }
    }
    Ok(state)
}

fn scan_journal(path: &Path) -> Result<JournalScan> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut scan = JournalScan {
        has_real_turn_prompt: false,
        earliest_time: None,
        latest_time: None,
    };
    for line in BufReader::new(file).lines() {
        let line = line.with_context(|| format!("failed to read {}", path.display()))?;
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            tracing::warn!(path = %path.display(), "skipping malformed Kimi journal line");
            continue;
        };
        if let Some((_, time)) = parse_wire_time(value.get("time")) {
            scan.earliest_time = Some(scan.earliest_time.map_or(time, |current| current.min(time)));
            scan.latest_time = Some(scan.latest_time.map_or(time, |current| current.max(time)));
        }
        scan.has_real_turn_prompt |= is_real_turn_prompt(&value);
    }
    Ok(scan)
}

fn parse_journal(
    path: &Path,
    session_id: &str,
    start_time: DateTime<Utc>,
    has_real_turn_prompt: bool,
) -> Result<ParsedJournal> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut messages = Vec::new();
    let mut transcript_items = Vec::new();
    let mut reasoning = PendingReasoning::default();
    let mut reasoning_attachments = Vec::new();
    let mut first_wire_time_ms = None;
    let mut tool_calls = Vec::new();
    let mut state = JournalState::default();

    for (line_number, line) in BufReader::new(file).lines().enumerate() {
        let line = line.with_context(|| format!("failed to read {}", path.display()))?;
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            tracing::warn!(path = %path.display(), "skipping malformed Kimi journal line");
            continue;
        };
        let wire_time = parse_wire_time(value.get("time"));
        let timestamp = match wire_time {
            Some((millis, time)) => {
                first_wire_time_ms.get_or_insert(millis);
                time
            }
            None => fallback_timestamp(path, start_time, line_number),
        };
        match value.get("type").and_then(Value::as_str) {
            Some("llm.request") => {
                let Some(model) = request_model(&value) else {
                    continue;
                };
                if let Some(turn_step) = nonblank(value.get("turnStep")) {
                    if let Some(step_uuid) = state.step_uuid_by_turn_step.get(&turn_step)
                        && let Some(step) = state.step_by_uuid.get_mut(step_uuid)
                    {
                        debug_assert_eq!(step.turn_step.as_deref(), Some(turn_step.as_str()));
                        step.model = Some(model);
                    } else {
                        state
                            .pending_models_by_turn_step
                            .entry(turn_step)
                            .or_default()
                            .push_back(model);
                    }
                } else {
                    state.unscoped_models.push_back(model);
                }
            }
            Some("config.update") => {
                state.active_model_hint = normalized_model(value.get("modelAlias"))
            }
            Some("usage.record")
                if value.get("usageScope").and_then(Value::as_str) == Some("turn") =>
            {
                if let Some(usage) = parse_usage(value.get("usage")) {
                    state.turn_usage.add(usage);
                }
            }
            Some("turn.prompt") if has_real_turn_prompt && is_real_turn_prompt(&value) => {
                if let Some(content) = normalize_content_parts(value.get("input"), true) {
                    push_message(
                        &mut messages,
                        &mut transcript_items,
                        &mut reasoning,
                        &mut reasoning_attachments,
                        session_id,
                        Role::User,
                        content,
                        timestamp,
                        None,
                    );
                }
            }
            Some("context.append_message") if !has_real_turn_prompt => {
                let message = value.get("message");
                if message
                    .and_then(|message| message.get("role"))
                    .and_then(Value::as_str)
                    == Some("user")
                    && message
                        .and_then(|message| message.get("origin"))
                        .and_then(|origin| origin.get("kind"))
                        .and_then(Value::as_str)
                        == Some("user")
                    && let Some(content) = normalize_content_parts(
                        message.and_then(|message| message.get("content")),
                        true,
                    )
                {
                    push_message(
                        &mut messages,
                        &mut transcript_items,
                        &mut reasoning,
                        &mut reasoning_attachments,
                        session_id,
                        Role::User,
                        content,
                        timestamp,
                        None,
                    );
                }
            }
            Some("context.append_loop_event") => {
                let Some(event) = value.get("event") else {
                    continue;
                };
                match event.get("type").and_then(Value::as_str) {
                    Some("step.begin") => {
                        let (Some(uuid), Some(turn_id), Some(step)) = (
                            nonblank(event.get("uuid")),
                            event.get("turnId").and_then(Value::as_str),
                            event.get("step").and_then(Value::as_i64),
                        ) else {
                            continue;
                        };
                        let turn_step = format!("{turn_id}.{step}");
                        let model = state
                            .pending_models_by_turn_step
                            .get_mut(&turn_step)
                            .and_then(VecDeque::pop_front)
                            .or_else(|| state.unscoped_models.pop_front())
                            .or_else(|| state.active_model_hint.clone());
                        state
                            .step_uuid_by_turn_step
                            .insert(turn_step.clone(), uuid.clone());
                        state.step_by_uuid.insert(
                            uuid,
                            StepState {
                                turn_step: Some(turn_step),
                                model,
                            },
                        );
                    }
                    Some("step.end") => {
                        if let Some(usage) = parse_usage(event.get("usage")) {
                            state.step_usage.add(usage);
                        }
                    }
                    Some("tool.call") => {
                        let Some(raw_id) = nonblank(event.get("toolCallId")) else {
                            continue;
                        };
                        let name =
                            nonblank(event.get("name")).unwrap_or_else(|| "unknown".to_string());
                        if state.pending_calls.contains_key(&raw_id) {
                            continue;
                        }
                        let tool_index = push_tool_call(
                            &mut tool_calls,
                            &mut transcript_items,
                            &mut reasoning,
                            &mut reasoning_attachments,
                            session_id,
                            &raw_id,
                            &name,
                            event.get("args"),
                            wire_time.map(|(millis, _)| millis),
                        );
                        if name == "Agent" {
                            state.agent_calls.push(AgentCallCandidate {
                                raw_call_id: raw_id.clone(),
                                transcript_item_index: transcript_items
                                    .last()
                                    .expect("tool call added a transcript item")
                                    .item_index,
                                call_time_ms: wire_time.map(|(millis, _)| millis),
                                args: event.get("args").cloned(),
                                result: None,
                                result_text: None,
                            });
                        }
                        if let Some((millis, _)) = wire_time {
                            state.call_start_times.insert(raw_id.clone(), millis);
                        }
                        state.pending_calls.insert(raw_id, tool_index);
                    }
                    Some("tool.result") => {
                        let Some(raw_id) = nonblank(event.get("toolCallId")) else {
                            continue;
                        };
                        let Some(&tool_index) = state.pending_calls.get(&raw_id) else {
                            continue;
                        };
                        let tool_call = &mut tool_calls[tool_index];
                        if tool_call.status != ToolCallStatus::Pending {
                            continue;
                        }
                        let result = event.get("result");
                        let output =
                            normalize_tool_output(result.and_then(|result| result.get("output")));
                        if result
                            .and_then(|result| result.get("isError"))
                            .and_then(Value::as_bool)
                            == Some(true)
                        {
                            tool_call.status = ToolCallStatus::Error;
                            tool_call.error_text = output;
                        } else {
                            tool_call.status = ToolCallStatus::Completed;
                            tool_call.output_text = output;
                        }
                        if let Some((end, _)) = wire_time {
                            tool_call.ended_at = Some(end.div_euclid(1000));
                            tool_call.duration_ms = state
                                .call_start_times
                                .get(&raw_id)
                                .and_then(|start| end.checked_sub(*start))
                                .filter(|duration| *duration >= 0);
                        }
                        if let Some(candidate) = state
                            .agent_calls
                            .iter_mut()
                            .find(|candidate| candidate.raw_call_id == raw_id)
                        {
                            candidate.result = result.cloned();
                            candidate.result_text = normalize_tool_output(
                                result.and_then(|result| result.get("output")),
                            );
                        }
                    }
                    Some("content.part") => {
                        let part = event.get("part");
                        let model = event
                            .get("stepUuid")
                            .and_then(Value::as_str)
                            .and_then(|uuid| state.step_by_uuid.get(uuid))
                            .and_then(|step| step.model.clone());
                        match part
                            .and_then(|part| part.get("type"))
                            .and_then(Value::as_str)
                        {
                            Some("text") => {
                                if let Some(content) =
                                    nonblank(part.and_then(|part| part.get("text")))
                                {
                                    push_message(
                                        &mut messages,
                                        &mut transcript_items,
                                        &mut reasoning,
                                        &mut reasoning_attachments,
                                        session_id,
                                        Role::Assistant,
                                        content,
                                        timestamp,
                                        model,
                                    );
                                }
                            }
                            Some("think") => {
                                if let Some(think) =
                                    nonblank(part.and_then(|part| part.get("think")))
                                {
                                    reasoning.merge(PendingReasoning {
                                        visible_text: Some(think),
                                        source_model: model,
                                        source_timestamp: Some(timestamp),
                                        ..Default::default()
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    if !messages.iter().any(|message| message.role == Role::User) {
        return Err(ParseError::NoUserMessages.into());
    }
    if !reasoning.is_empty() {
        tracing::debug!(path = %path.display(), "dropping orphan Kimi reasoning");
    }
    Ok(ParsedJournal {
        parsed: ParsedSession {
            session: placeholder_session(session_id, start_time),
            messages,
            tool_calls,
            subagents: Vec::new(),
            transcript_items,
            reasoning_attachments,
            token_usage: if state.turn_usage.seen {
                state.turn_usage.into_token_usage()
            } else {
                state.step_usage.into_token_usage()
            },
        },
        first_wire_time_ms,
        agent_calls: state.agent_calls,
    })
}

fn is_real_turn_prompt(value: &Value) -> bool {
    value.get("type").and_then(Value::as_str) == Some("turn.prompt")
        && value
            .get("origin")
            .and_then(|origin| origin.get("kind"))
            .and_then(Value::as_str)
            == Some("user")
        && normalize_content_parts(value.get("input"), true).is_some()
}

fn normalize_content_parts(value: Option<&Value>, include_media: bool) -> Option<String> {
    let parts = value?.as_array()?;
    let mut output = Vec::new();
    for part in parts {
        match part.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = part.get("text").and_then(Value::as_str)
                    && !text.trim().is_empty()
                {
                    output.push(text.to_string());
                }
            }
            Some("image_url") if include_media => output.push("[image]".to_string()),
            Some("audio_url") if include_media => output.push("[audio]".to_string()),
            Some("video_url") if include_media => output.push("[video]".to_string()),
            _ => {}
        }
    }
    let joined = output.join("\n");
    (!joined.trim().is_empty()).then_some(joined)
}

fn fallback_timestamp(path: &Path, start_time: DateTime<Utc>, line_number: usize) -> DateTime<Utc> {
    let Some(milliseconds) = i64::try_from(line_number).ok() else {
        tracing::warn!(path = %path.display(), "Kimi journal line number overflowed timestamp fallback");
        return start_time;
    };
    let Some(timestamp) = start_time.checked_add_signed(Duration::milliseconds(milliseconds))
    else {
        tracing::warn!(path = %path.display(), "Kimi journal timestamp fallback overflowed");
        return start_time;
    };
    timestamp
}

#[allow(clippy::too_many_arguments)] // Keeps transcript and reasoning indexes in one invariant-preserving helper.
fn push_message(
    messages: &mut Vec<Message>,
    transcript_items: &mut Vec<TranscriptItem>,
    reasoning: &mut PendingReasoning,
    reasoning_attachments: &mut Vec<crate::models::ReasoningAttachment>,
    session_id: &str,
    role: Role,
    content: String,
    timestamp: DateTime<Utc>,
    model: Option<String>,
) {
    let message_index = messages.len();
    let item_index = transcript_items.len() as i64;
    messages.push(Message {
        session_id: session_id.to_string(),
        index: message_index,
        role,
        content,
        timestamp,
        model,
    });
    transcript_items.push(TranscriptItem {
        session_id: session_id.to_string(),
        item_index,
        kind: TranscriptItemKind::Message,
        message_index: Some(message_index as i64),
        tool_call_id: None,
        subagent_id: None,
    });
    if reasoning.is_empty() {
        return;
    }
    if role == Role::Assistant {
        reasoning_attachments
            .push(std::mem::take(reasoning).into_attachment(session_id, item_index));
    } else {
        // A user prompt closes the turn that produced this reasoning, so the
        // reasoning has no visible item to attach to: keeping it pending would
        // move it onto the next turn's assistant output instead.
        *reasoning = PendingReasoning::default();
        tracing::debug!("dropping Kimi reasoning orphaned by a user message");
    }
}

#[allow(clippy::too_many_arguments)] // Keeps transcript and reasoning indexes in one invariant-preserving helper.
fn push_tool_call(
    tool_calls: &mut Vec<ToolCall>,
    transcript_items: &mut Vec<TranscriptItem>,
    reasoning: &mut PendingReasoning,
    reasoning_attachments: &mut Vec<crate::models::ReasoningAttachment>,
    session_id: &str,
    raw_id: &str,
    name: &str,
    args: Option<&Value>,
    time_ms: Option<i64>,
) -> usize {
    let local_id = format!("kimi-tool::{session_id}::{raw_id}");
    let item_index = transcript_items.len() as i64;
    let tool_index = tool_calls.len();
    tool_calls.push(ToolCall {
        id: local_id.clone(),
        session_id: session_id.to_string(),
        subagent_id: None,
        tool_name: name.to_string(),
        status: ToolCallStatus::Pending,
        title: None,
        summary: None,
        input_json: args.and_then(|value| serde_json::to_string(value).ok()),
        output_text: None,
        error_text: None,
        started_at: time_ms.map(|millis| millis.div_euclid(1000)),
        ended_at: None,
        duration_ms: None,
        parser_call_id: Some(raw_id.to_string()),
    });
    transcript_items.push(TranscriptItem {
        session_id: session_id.to_string(),
        item_index,
        kind: TranscriptItemKind::ToolCall,
        message_index: None,
        tool_call_id: Some(local_id),
        subagent_id: None,
    });
    if !reasoning.is_empty() {
        reasoning_attachments
            .push(std::mem::take(reasoning).into_attachment(session_id, item_index));
    }
    tool_index
}

fn placeholder_session(session_id: &str, timestamp: DateTime<Utc>) -> Session {
    Session {
        id: session_id.to_string(),
        tool: AiAssistant::KimiCode,
        project_path: None,
        project_id: None,
        start_time: timestamp,
        message_count: 0,
        file_path: String::new(),
        last_updated: timestamp,
        pinned_at: None,
        first_prompt: None,
        parent_session_id: None,
        is_subagent: false,
        token_usage: None,
        edit_count: 0,
        read_count: 0,
        command_count: 0,
        ending_status: crate::models::SessionEndingStatus::Unknown,
    }
}
