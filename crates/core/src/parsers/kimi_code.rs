#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Role, TokenUsage, ToolCallStatus, TranscriptItemKind};
    use serde_json::{Value, json};
    use std::fs;
    use std::path::PathBuf;

    const GIT_CONTEXT: &str =
        r"<git-context>\nWorking directory: /tmp/project\nBranch: main\n</git-context>\n\n";

    fn child_prompt(time: i64, text: &str) -> String {
        format!(
            r#"{{"type":"turn.prompt","time":{time},"input":[{{"type":"text","text":"{GIT_CONTEXT}{text}"}}],"origin":{{"kind":"system_trigger","name":"subagent"}}}}"#
        )
    }

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

    fn swarm_state() -> Value {
        let mut metadata = state();
        for (agent_id, item) in [("agent-0", "A"), ("agent-1", "B")] {
            metadata["agents"][agent_id] = json!({
                "type": "sub",
                "labels": {"parentAgentId": "main", "swarmItem": item, "profileName": "explore"}
            });
        }
        metadata
    }

    fn swarm_records(result: Option<Value>) -> Vec<Value> {
        let mut records = vec![
            json!({"type":"turn.prompt", "time":1785320000000_i64, "input":[{"type":"text", "text":"Review both"}], "origin":{"kind":"user"}}),
            json!({"type":"context.append_loop_event", "event":{"type":"content.part", "part":{"type":"text", "text":"Starting review"}}}),
            json!({"type":"context.append_loop_event", "time":1785320000010_i64, "event":{"type":"tool.call", "toolCallId":"AgentSwarm_0", "name":"AgentSwarm", "args":{"description":"Review items", "subagent_type":"explore", "prompt_template":"Review {{item}} now", "items":["A", "B"]}}}),
        ];
        if let Some(result) = result {
            records.push(json!({"type":"context.append_loop_event", "time":1785320000040_i64, "event":{"type":"tool.result", "toolCallId":"AgentSwarm_0", "result":result}}));
        }
        records.push(json!({"type":"context.append_loop_event", "event":{"type":"content.part", "part":{"type":"text", "text":"Review complete"}}}));
        records
    }

    fn parse_swarm_records(metadata: Value, records: &[Value]) -> KimiParsedBundle {
        let records: Vec<_> = records.iter().map(Value::to_string).collect();
        let root = write_bundle(
            metadata,
            &records.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        for (agent_id, prompt) in [("agent-0", "Review A now"), ("agent-1", "Review B now")] {
            write_agent_journal(&root, agent_id, &[&child_prompt(1785320000030, prompt)]);
        }
        KimiCodeParser::new(root.path())
            .parse_session_dir(&session_dir(&root))
            .unwrap()
    }

    fn sub_state() -> Value {
        let mut metadata = state();
        metadata["agents"]["agent-0"] = json!({"type": "sub", "parentAgentId": "main"});
        metadata
    }

    fn agents_state(agents: Value) -> Value {
        let mut metadata = state();
        metadata["agents"] = agents;
        metadata
    }

    fn try_parse_agents_bundle(
        metadata: Value,
        main_records: &[&str],
        journals: &[(&str, &[&str])],
    ) -> Result<KimiParsedBundle> {
        let root = write_bundle(metadata, main_records);
        for (agent_id, records) in journals {
            write_agent_journal(&root, agent_id, records);
        }
        KimiCodeParser::new(root.path()).parse_session_dir(&session_dir(&root))
    }

    fn parse_agents_bundle(
        metadata: Value,
        main_records: &[&str],
        journals: &[(&str, &[&str])],
    ) -> KimiParsedBundle {
        try_parse_agents_bundle(metadata, main_records, journals).unwrap()
    }

    fn assert_no_user_messages(result: Result<KimiParsedBundle>) {
        let error = result.unwrap_err();
        assert!(matches!(
            error.downcast_ref::<ParseError>(),
            Some(ParseError::NoUserMessages)
        ));
    }

    // Assumes the `swarm_records` layout: the user prompt and the "Starting review"
    // text part precede the swarm rows, so row `index` sits at transcript item `index + 2`.
    fn assert_linked_swarm_row(
        parsed: &KimiParsedBundle,
        index: usize,
        agent_id: &str,
        title: &str,
        prompt: &str,
        summary: Option<&str>,
    ) {
        let row = &parsed.main.subagents[index];
        assert_eq!(row.agent_id.as_deref(), Some(agent_id));
        assert_eq!(row.title, title);
        assert_eq!(row.prompt.as_deref(), Some(prompt));
        assert_eq!(row.result_summary.as_deref(), summary);
        assert_eq!(
            row.child_session_id.as_deref(),
            Some(parsed.children[index].session.id.as_str())
        );
        assert_eq!(
            parsed.main.transcript_items[index + 2]
                .subagent_id
                .as_deref(),
            Some(row.id.as_str())
        );
    }

    #[test]
    fn swarm_call_expands_into_one_item_per_child() {
        // Result order differs from items: transcript order must follow args.items.
        let records = swarm_records(Some(
            json!({"output":"<agent_swarm_result>\n<summary>completed: 2</summary>\n<subagent agent_id=\"agent-1\" item=\"B\" outcome=\"completed\">Body B</subagent>\n<subagent agent_id=\"agent-0\" item=\"A\" outcome=\"completed\">Body A</subagent>\n</agent_swarm_result>"}),
        ));
        let parsed = parse_swarm_records(swarm_state(), &records);
        assert_eq!(
            parsed
                .main
                .transcript_items
                .iter()
                .map(|item| item.kind.clone())
                .collect::<Vec<_>>(),
            [
                TranscriptItemKind::Message,
                TranscriptItemKind::Message,
                TranscriptItemKind::Subagent,
                TranscriptItemKind::Subagent,
                TranscriptItemKind::Message
            ]
        );
        assert!(parsed.main.tool_calls.is_empty());
        assert_eq!(parsed.main.subagents.len(), 2);
        for (index, (agent_id, title, prompt, summary)) in [
            ("agent-0", "A", "Review A now", "Body A"),
            ("agent-1", "B", "Review B now", "Body B"),
        ]
        .into_iter()
        .enumerate()
        {
            let row = &parsed.main.subagents[index];
            assert_eq!(
                row.id,
                format!(
                    "kimi-agent-call::session_00000000-0000-4000-8000-000000000001::AgentSwarm_0::{agent_id}"
                )
            );
            assert_eq!(row.agent_name.as_deref(), Some("explore"));
            assert_eq!(row.parser_ref.as_deref(), Some("AgentSwarm_0"));
            assert_linked_swarm_row(&parsed, index, agent_id, title, prompt, Some(summary));
        }
    }

    #[test]
    fn swarm_retry_explicit_result_reserves_child_before_earlier_fallback() {
        let mut records = swarm_records(Some(json!({"output":"Session closed", "isError":true})));
        records[2]["event"]["args"]["items"] = json!(["A"]);
        records.push(json!({"type":"context.append_loop_event", "time":1785320000020_i64, "event":{"type":"tool.call", "toolCallId":"AgentSwarm_retry", "name":"AgentSwarm", "args":{"items":["A"]}}}));
        records.push(json!({"type":"context.append_loop_event", "event":{"type":"tool.result", "toolCallId":"AgentSwarm_retry", "result":{"output":"<agent_swarm_result>\n<subagent agent_id=\"agent-0\" item=\"A\">Retry body</subagent>\n</agent_swarm_result>"}}}));

        let parsed = parse_swarm_records(swarm_state(), &records).main;
        assert_eq!(parsed.subagents.len(), 1);
        let row = &parsed.subagents[0];
        assert_eq!(row.parser_ref.as_deref(), Some("AgentSwarm_retry"));
        assert_eq!(row.agent_id.as_deref(), Some("agent-0"));
        assert_eq!(row.result_summary.as_deref(), Some("Retry body"));
        assert_eq!(parsed.tool_calls.len(), 1);
        assert!(parsed.tool_calls[0].id.ends_with("::AgentSwarm_0"));
        assert_eq!(
            parsed.transcript_items[2].kind,
            TranscriptItemKind::ToolCall
        );
        assert_eq!(
            parsed.transcript_items[4].subagent_id.as_deref(),
            Some(row.id.as_str())
        );
    }

    #[test]
    fn swarm_shared_fallback_child_keeps_both_attempts_generic() {
        let mut records = swarm_records(None);
        records[2]["event"]["args"]["items"] = json!(["A"]);
        records.push(json!({"type":"context.append_loop_event", "time":1785320000020_i64, "event":{"type":"tool.call", "toolCallId":"AgentSwarm_retry", "name":"AgentSwarm", "args":{"items":["A"]}}}));
        records.push(json!({"type":"context.append_loop_event", "time":1785320000020_i64, "event":{"type":"tool.call", "toolCallId":"AgentSwarm_independent", "name":"AgentSwarm", "args":{"items":["B"]}}}));

        let parsed = parse_swarm_records(swarm_state(), &records).main;
        assert_eq!(parsed.subagents.len(), 1);
        assert_eq!(
            parsed.subagents[0].parser_ref.as_deref(),
            Some("AgentSwarm_independent")
        );
        assert_eq!(parsed.subagents[0].agent_id.as_deref(), Some("agent-1"));
        assert_eq!(parsed.tool_calls.len(), 2);
        assert!(
            parsed
                .tool_calls
                .iter()
                .any(|call| call.id.ends_with("::AgentSwarm_0"))
        );
        assert!(
            parsed
                .tool_calls
                .iter()
                .any(|call| call.id.ends_with("::AgentSwarm_retry"))
        );
    }

    #[test]
    fn swarm_incomplete_fallback_claim_prevents_wrong_link_from_other_attempt() {
        let mut records = swarm_records(None);
        records[2]["event"]["args"]["items"] = json!(["A"]);
        records.push(json!({"type":"context.append_loop_event", "time":1785320000020_i64, "event":{"type":"tool.call", "toolCallId":"AgentSwarm_retry", "name":"AgentSwarm", "args":{"items":["A", "missing"]}}}));

        let parsed = parse_swarm_records(swarm_state(), &records).main;
        assert!(parsed.subagents.is_empty());
        assert_eq!(parsed.tool_calls.len(), 2);
    }

    #[test]
    fn swarm_duplicate_explicit_claims_stay_generic_and_reserve_children() {
        let output = "<agent_swarm_result>\n<subagent agent_id=\"agent-0\" item=\"A\">Body A</subagent>\n<subagent agent_id=\"agent-1\" item=\"B\">Body B</subagent>\n</agent_swarm_result>";
        let mut records = swarm_records(Some(json!({"output":output})));
        // Overlap on A invalidates both whole explicit swarms, including B.
        records.push(json!({"type":"context.append_loop_event", "time":1785320000020_i64, "event":{"type":"tool.call", "toolCallId":"AgentSwarm_duplicate", "name":"AgentSwarm", "args":{"items":["A"]}}}));
        records.push(json!({"type":"context.append_loop_event", "event":{"type":"tool.result", "toolCallId":"AgentSwarm_duplicate", "result":{"output":"<agent_swarm_result>\n<subagent agent_id=\"agent-0\" item=\"A\">Duplicate A</subagent>\n</agent_swarm_result>"}}}));
        records.push(json!({"type":"context.append_loop_event", "time":1785320000020_i64, "event":{"type":"tool.call", "toolCallId":"AgentSwarm_fallback", "name":"AgentSwarm", "args":{"items":["B"]}}}));

        let parsed = parse_swarm_records(swarm_state(), &records).main;
        assert!(parsed.subagents.is_empty());
        assert_eq!(parsed.tool_calls.len(), 3);
        assert_eq!(
            parsed
                .transcript_items
                .iter()
                .filter(|item| item.kind == TranscriptItemKind::ToolCall)
                .count(),
            3
        );
    }

    #[test]
    fn swarm_label_items_preserve_whitespace_in_results() {
        assert_swarm_preserves_whitespace_identity(false, true);
    }

    #[test]
    fn swarm_legacy_items_preserve_whitespace_in_results() {
        assert_swarm_preserves_whitespace_identity(true, true);
    }

    #[test]
    fn swarm_label_items_preserve_whitespace_in_pending_fallback() {
        assert_swarm_preserves_whitespace_identity(false, false);
    }

    #[test]
    fn swarm_legacy_items_preserve_whitespace_in_pending_fallback() {
        assert_swarm_preserves_whitespace_identity(true, false);
    }

    fn assert_swarm_preserves_whitespace_identity(legacy: bool, with_result: bool) {
        let mut metadata = swarm_state();
        for (agent_id, item) in [("agent-0", " A "), ("agent-1", "A")] {
            if legacy {
                metadata["agents"][agent_id]["labels"]
                    .as_object_mut()
                    .unwrap()
                    .remove("swarmItem");
                metadata["agents"][agent_id]["swarmItem"] = json!(item);
            } else {
                metadata["agents"][agent_id]["labels"]["swarmItem"] = json!(item);
            }
        }
        let result = with_result.then(|| json!({"output":"<agent_swarm_result>\n<subagent agent_id=\"agent-1\" item=\"A\">Plain body</subagent>\n<subagent agent_id=\"agent-0\" item=\" A \">Spaced body</subagent>\n</agent_swarm_result>"}));
        let mut records = swarm_records(result);
        records[2]["event"]["args"]["items"] = json!([" A ", "A"]);
        let parsed = parse_swarm_records(metadata, &records);
        assert_eq!(parsed.main.subagents.len(), 2);
        assert!(parsed.main.tool_calls.is_empty());
        for (index, (agent_id, title, prompt, summary)) in [
            ("agent-0", " A ", "Review  A  now", "Spaced body"),
            ("agent-1", "A", "Review A now", "Plain body"),
        ]
        .into_iter()
        .enumerate()
        {
            assert_linked_swarm_row(
                &parsed,
                index,
                agent_id,
                title,
                prompt,
                with_result.then_some(summary),
            );
        }
    }

    #[test]
    fn swarm_null_items_stay_generic_despite_valid_result() {
        assert_invalid_swarm_items_stay_generic(Value::Null);
    }

    #[test]
    fn swarm_non_array_items_stay_generic_despite_valid_result() {
        assert_invalid_swarm_items_stay_generic(json!("A"));
    }

    #[test]
    fn swarm_mixed_items_stay_generic_despite_valid_result() {
        assert_invalid_swarm_items_stay_generic(json!(["A", null]));
    }

    fn assert_invalid_swarm_items_stay_generic(items: Value) {
        const OUTPUT: &str = "<agent_swarm_result>\n<subagent agent_id=\"agent-0\" item=\"A\">Body A</subagent>\n</agent_swarm_result>";
        let mut records = swarm_records(Some(json!({"output":OUTPUT})));
        records[2]["event"]["args"]
            .as_object_mut()
            .unwrap()
            .remove("items");
        // Absence still permits result-order resolution through the real parser.
        let absent = parse_swarm_records(swarm_state(), &records).main;
        assert_eq!(absent.subagents.len(), 1);
        assert_eq!(absent.subagents[0].agent_id.as_deref(), Some("agent-0"));
        assert_eq!(
            absent.subagents[0].result_summary.as_deref(),
            Some("Body A")
        );
        assert!(absent.tool_calls.is_empty());

        records[2]["event"]["args"]["items"] = items.clone();
        let parsed = parse_swarm_records(swarm_state(), &records);
        assert_eq!(parsed.children.len(), 2);
        assert!(parsed.main.subagents.is_empty());
        assert_eq!(parsed.main.tool_calls.len(), 1);
        let call = &parsed.main.tool_calls[0];
        assert_eq!(call.tool_name, "AgentSwarm");
        assert_eq!(call.output_text.as_deref(), Some(OUTPUT));
        let input: Value = serde_json::from_str(call.input_json.as_deref().unwrap()).unwrap();
        assert_eq!(input["items"], items);
        assert_eq!(
            parsed.main.transcript_items[2].kind,
            TranscriptItemKind::ToolCall
        );
        assert_eq!(
            parsed.main.transcript_items[2].tool_call_id.as_deref(),
            Some(call.id.as_str())
        );
    }

    #[test]
    fn swarm_reasoning_moves_to_first_item() {
        let mut records = swarm_records(None);
        records.insert(2, json!({"type":"context.append_loop_event", "event":{"type":"content.part", "part":{"type":"think", "think":"Fan out"}}}));
        records.insert(4, json!({"type":"context.append_loop_event", "event":{"type":"content.part", "part":{"type":"think", "think":"Summarize"}}}));
        records.push(json!({"type":"context.append_loop_event", "event":{"type":"content.part", "part":{"type":"think", "think":"Read next"}}}));
        records.push(json!({"type":"context.append_loop_event", "event":{"type":"tool.call", "toolCallId":"Read_0", "name":"Read", "args":{"path":"README.md"}}}));
        let parsed = parse_swarm_records(swarm_state(), &records).main;
        assert_eq!(
            parsed
                .reasoning_attachments
                .iter()
                .map(|attachment| (
                    attachment.transcript_item_index,
                    attachment.visible_text.as_deref()
                ))
                .collect::<Vec<_>>(),
            [
                (2, Some("Fan out")),
                (4, Some("Summarize")),
                (5, Some("Read next"))
            ]
        );
        assert_eq!(
            parsed.transcript_items[2].kind,
            TranscriptItemKind::Subagent
        );
        assert_eq!(
            parsed.transcript_items[3].kind,
            TranscriptItemKind::Subagent
        );
        assert_eq!(
            parsed.transcript_items[5].kind,
            TranscriptItemKind::ToolCall
        );
    }

    #[test]
    fn pending_swarm_expands_through_items() {
        let mut metadata = swarm_state();
        metadata["agents"]["agent-1"]["labels"]
            .as_object_mut()
            .unwrap()
            .remove("profileName");
        let parsed = parse_swarm_records(metadata, &swarm_records(None)).main;
        assert_eq!(parsed.subagents.len(), 2);
        assert!(
            parsed
                .subagents
                .iter()
                .all(|row| row.result_summary.is_none())
        );
        assert_eq!(parsed.subagents[1].agent_name.as_deref(), Some("explore"));
        assert!(parsed.tool_calls.is_empty());
    }

    #[test]
    fn failed_swarm_uses_error_output_summary() {
        let mut metadata = swarm_state();
        for (agent_id, item) in [("agent-0", "A"), ("agent-1", "B")] {
            metadata["agents"][agent_id]["labels"]
                .as_object_mut()
                .unwrap()
                .remove("swarmItem");
            metadata["agents"][agent_id]["swarmItem"] = json!(item);
        }
        let parsed = parse_swarm_records(
            metadata,
            &swarm_records(Some(json!({"output":"Session closed", "isError":true}))),
        )
        .main;
        assert_eq!(parsed.subagents.len(), 2);
        assert!(
            parsed
                .subagents
                .iter()
                .all(|row| row.result_summary.as_deref() == Some("Session closed"))
        );
        assert!(parsed.tool_calls.is_empty());
    }

    #[test]
    fn unresolvable_swarm_stays_generic() {
        let mut records = swarm_records(None);
        records[2]["event"]["args"]["items"] = json!(["A", "missing"]);
        let parsed = parse_swarm_records(swarm_state(), &records).main;
        assert!(parsed.subagents.is_empty());
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].tool_name, "AgentSwarm");
        assert_eq!(
            parsed.transcript_items[2].kind,
            TranscriptItemKind::ToolCall
        );
    }

    #[test]
    fn later_resume_can_target_a_swarm_child() {
        let mut records = swarm_records(None);
        // Journal order wins over a backwards wall clock for an explicit resume.
        records.push(json!({"type":"context.append_loop_event", "time":1785320000005_i64, "event":{"type":"tool.call", "toolCallId":"Agent_resume", "name":"Agent", "args":{"resume":"agent-0"}}}));
        let parsed = parse_swarm_records(swarm_state(), &records).main;
        assert_eq!(parsed.subagents.len(), 3);
        assert_eq!(
            parsed.subagents[2].parser_ref.as_deref(),
            Some("Agent_resume")
        );
        assert_eq!(
            parsed.subagents[2].child_session_id,
            parsed.subagents[0].child_session_id
        );
        assert!(parsed.tool_calls.is_empty());
        assert_eq!(
            parsed.transcript_items[5].subagent_id.as_deref(),
            Some(parsed.subagents[2].id.as_str())
        );
    }

    #[test]
    fn synthetic_swarm_consumption_rejects_earlier_and_non_resume_agent_evidence() {
        for (earlier, args, result) in [
            (true, json!({"resume":"agent-0"}), None),
            (true, json!({"agent_id":"agent-0"}), None),
            (false, json!({"agent_id":"agent-0"}), None),
            (
                false,
                json!({}),
                Some(json!({"output":"agent_id: agent-0\nstatus: completed\n\nDone."})),
            ),
            (false, json!({}), None),
        ] {
            let mut records = swarm_records(Some(
                json!({"output":"<agent_swarm_result>\n<subagent agent_id=\"agent-0\" item=\"A\">Body A</subagent>\n<subagent agent_id=\"agent-1\" item=\"B\">Body B</subagent>\n</agent_swarm_result>"}),
            ));
            // No swarmItem label: consumed children must also be excluded from chronology.
            let mut metadata = swarm_state();
            for agent_id in ["agent-0", "agent-1"] {
                metadata["agents"][agent_id]["labels"]
                    .as_object_mut()
                    .unwrap()
                    .remove("swarmItem");
            }
            // Deliberately inverted clock: call order, not timestamp, defines a later resume.
            let call = json!({"type":"context.append_loop_event", "time":1785320000020_i64, "event":{"type":"tool.call", "toolCallId":"Agent_rejected", "name":"Agent", "args":args}});
            if earlier {
                records.insert(2, call);
            } else {
                records.push(call);
            }
            let chronological = args == json!({}) && result.is_none();
            if chronological {
                // Match cardinalities so excluding consumed children is what prevents links.
                records.push(json!({"type":"context.append_loop_event", "time":1785320000021_i64, "event":{"type":"tool.call", "toolCallId":"Agent_also_rejected", "name":"Agent", "args":{}}}));
            }
            if let Some(result) = result {
                records.push(json!({"type":"context.append_loop_event", "event":{"type":"tool.result", "toolCallId":"Agent_rejected", "result":result}}));
            }
            let parsed = parse_swarm_records(metadata, &records).main;
            assert_eq!(
                parsed.tool_calls.len(),
                if chronological { 2 } else { 1 },
                "earlier={earlier}, args={args}"
            );
            assert!(
                parsed
                    .tool_calls
                    .iter()
                    .all(|call| call.tool_name == "Agent")
            );
            assert_eq!(parsed.subagents.len(), 2);
            assert!(
                parsed
                    .subagents
                    .iter()
                    .all(|row| row.parser_ref.as_deref() == Some("AgentSwarm_0"))
            );
        }
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
        assert_no_user_messages(
            KimiCodeParser::new(injected.path()).parse_session_dir(&session_dir(&injected)),
        );
    }

    #[test]
    fn child_journal_accepts_subagent_trigger_prompt() {
        let parsed = parse_agents_bundle(
            sub_state(),
            &[
                r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Main prompt"}],"origin":{"kind":"user"}}"#,
            ],
            &[(
                "agent-0",
                &[&child_prompt(1785320000010, "Inspect parser safety")],
            )],
        );

        assert_eq!(parsed.children.len(), 1);
        assert_eq!(
            parsed.children[0].messages[0].content,
            "Inspect parser safety"
        );
        assert_eq!(
            parsed.children[0].session.first_prompt.as_deref(),
            Some("Inspect parser safety")
        );
    }

    #[test]
    fn main_journal_rejects_subagent_trigger_prompt() {
        assert_no_user_messages(try_parse_agents_bundle(
            state(),
            &[&child_prompt(1785320000000, "Subagent task")],
            &[],
        ));
    }

    #[test]
    fn child_rejects_other_system_triggers_and_injections() {
        let parsed = parse_agents_bundle(
            sub_state(),
            &[
                r#"{"type":"turn.prompt","input":[{"type":"text","text":"Main prompt"}],"origin":{"kind":"user"}}"#,
            ],
            &[(
                "agent-0",
                &[
                    r#"{"type":"turn.prompt","input":[{"type":"text","text":"Cron"}],"origin":{"kind":"system_trigger","name":"cron"}}"#,
                    r#"{"type":"turn.prompt","input":[{"type":"text","text":"Injection"}],"origin":{"kind":"injection"}}"#,
                ],
            )],
        );

        assert!(parsed.children.is_empty());
    }

    #[test]
    fn child_resume_prompts_become_user_messages() {
        let parsed = parse_agents_bundle(
            sub_state(),
            &[
                r#"{"type":"turn.prompt","input":[{"type":"text","text":"Main prompt"}],"origin":{"kind":"user"}}"#,
            ],
            &[(
                "agent-0",
                &[
                    &child_prompt(1785320000010, "First task"),
                    r#"{"type":"turn.prompt","time":1785320000020,"input":[{"type":"text","text":"Second task"}],"origin":{"kind":"system_trigger","name":"subagent"}}"#,
                    r#"{"type":"turn.prompt","time":1785320000030,"input":[{"type":"text","text":"Third task"}],"origin":{"kind":"system_trigger","name":"subagent"}}"#,
                ],
            )],
        );
        let child = &parsed.children[0];

        assert_eq!(child.messages.len(), 3);
        assert!(
            child
                .messages
                .iter()
                .all(|message| message.role == Role::User)
        );
        assert_eq!(
            child
                .messages
                .iter()
                .map(|message| message.content.as_str())
                .collect::<Vec<_>>(),
            ["First task", "Second task", "Third task"]
        );
    }

    #[test]
    fn child_unclosed_git_context_is_kept() {
        let unclosed = "<git-context>\\nWorking directory: /tmp/project\\nTask remains here";
        let prompt = format!(
            r#"{{"type":"turn.prompt","input":[{{"type":"text","text":"{unclosed}"}}],"origin":{{"kind":"system_trigger","name":"subagent"}}}}"#
        );
        let parsed = parse_agents_bundle(
            sub_state(),
            &[
                r#"{"type":"turn.prompt","input":[{"type":"text","text":"Main prompt"}],"origin":{"kind":"user"}}"#,
            ],
            &[("agent-0", &[&prompt])],
        );

        assert_eq!(parsed.children.len(), 1);
        assert_eq!(
            parsed.children[0].messages[0].content,
            unclosed.replace("\\n", "\n")
        );
    }

    #[test]
    fn child_with_only_git_context_remains_indexed() {
        let parsed = parse_agents_bundle(
            sub_state(),
            &[
                r#"{"type":"turn.prompt","input":[{"type":"text","text":"Main prompt"}],"origin":{"kind":"user"}}"#,
            ],
            &[("agent-0", &[&child_prompt(1785320000010, "")])],
        );

        assert_eq!(parsed.children.len(), 1);
        assert_eq!(
            parsed.children[0].messages[0].content,
            GIT_CONTEXT.replace("\\n", "\n")
        );
    }

    #[test]
    fn forked_child_hides_copied_context() {
        let parsed = parse_agents_bundle(
            sub_state(),
            &[
                r#"{"type":"turn.prompt","input":[{"type":"text","text":"Main prompt"}],"origin":{"kind":"user"}}"#,
            ],
            &[(
                "agent-0",
                &[
                    r#"{"type":"context.append_message","message":{"role":"user","content":[{"type":"text","text":"Copied parent prompt"}],"origin":{"kind":"user"}}}"#,
                    r#"{"type":"context.append_message","message":{"role":"user","content":[{"type":"text","text":"Copied parent follow-up"}],"origin":{"kind":"user"}}}"#,
                    &child_prompt(1785320000010, "Actual child task"),
                ],
            )],
        );

        assert_eq!(parsed.children.len(), 1);
        assert_eq!(parsed.children[0].messages.len(), 1);
        assert_eq!(parsed.children[0].messages[0].content, "Actual child task");
    }

    #[test]
    fn labels_supply_profile_swarm_item_and_parent_fallback() {
        let mut metadata = state();
        metadata["agents"]["agent-0"] = json!({
            "type": "sub",
            "labels": {
                "parentAgentId": "main",
                "profileName": "explore",
                "swarmItem": "A"
            }
        });
        metadata["agents"]["agent-1"] = json!({
            "type": "sub",
            "parentAgentId": "main",
            "swarmItem": "B"
        });
        let root = write_bundle(
            metadata,
            &[
                r#"{"type":"turn.prompt","input":[{"type":"text","text":"Main prompt"}],"origin":{"kind":"user"}}"#,
            ],
        );
        write_agent_journal(&root, "agent-0", &[&child_prompt(1785320000010, "Task A")]);
        write_agent_journal(&root, "agent-1", &[&child_prompt(1785320000020, "Task B")]);

        let parsed = KimiCodeParser::new(root.path())
            .parse_session_dir(&session_dir(&root))
            .unwrap();
        let loaded = load_state(&session_dir(&root).join("state.json")).unwrap();

        assert_eq!(parsed.children.len(), 2);
        assert_eq!(
            loaded.agents["agent-0"].parent_agent_id.as_deref(),
            Some("main")
        );
        assert_eq!(
            loaded.agents["agent-0"].profile_name.as_deref(),
            Some("explore")
        );
        assert_eq!(loaded.agents["agent-0"].swarm_item.as_deref(), Some("A"));
        assert_eq!(loaded.agents["agent-1"].swarm_item.as_deref(), Some("B"));
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
    fn models_follow_steps_and_usage_records_win_without_double_counting() {
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
                r#"{"type":"usage.record","usageScope":"session","usage":{"inputOther":50,"output":5,"inputCacheRead":0,"inputCacheCreation":0}}"#,
                r#"{"type":"usage.record","usage":{"inputOther":1,"output":0}}"#,
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
                input_tokens: 151,
                output_tokens: 25,
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
        let parsed = parse_agents_bundle(
            agents_state(json!({
                "main": { "type": "main", "parentAgentId": null },
                "agent-0": { "type": "sub", "parentAgentId": "main" },
                "agent-1": { "type": "sub", "parentAgentId": "main" },
                "agent-nested": { "type": "sub", "parentAgentId": "agent-0" },
                "independent-0": { "type": "independent", "parentAgentId": null },
                "unknown-parent": { "type": "sub", "parentAgentId": "unknown" },
                "cycle-a": { "type": "sub", "parentAgentId": "cycle-b" },
                "cycle-b": { "type": "sub", "parentAgentId": "cycle-a" }
            })),
            &[
                r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Main"}],"origin":{"kind":"user"}}"#,
            ],
            &[
                (
                    "agent-0",
                    &[
                        r#"{"type":"turn.prompt","time":1785320000100,"input":[{"type":"text","text":"Child 0"}],"origin":{"kind":"user"}}"#,
                    ],
                ),
                (
                    "agent-1",
                    &[
                        r#"{"type":"turn.prompt","time":1785320000200,"input":[{"type":"text","text":"Child 1"}],"origin":{"kind":"user"}}"#,
                    ],
                ),
                (
                    "agent-nested",
                    &[
                        r#"{"type":"turn.prompt","time":1785320000300,"input":[{"type":"text","text":"Nested"}],"origin":{"kind":"user"}}"#,
                    ],
                ),
            ],
        );

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
        let matched = parse_agents_bundle(
            agents_state(json!({
                "main": { "type": "main", "parentAgentId": null },
                "agent-0": { "type": "sub", "parentAgentId": "main" }
            })),
            &[
                r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Main"}],"origin":{"kind":"user"}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000010,"event":{"type":"content.part","part":{"type":"text","text":"Planning"}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000020,"event":{"type":"tool.call","toolCallId":"Agent_0","name":"Agent","args":{"agent_id":"agent-0","subagent_type":"explore","prompt":"Inspect parser safety"}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000030,"event":{"type":"tool.result","toolCallId":"Agent_0","result":{"output":"done"}}}"#,
            ],
            &[(
                "agent-0",
                &[r#"{"type":"turn.prompt","time":1785320000025,"input":[{"type":"text","text":"Child"}],"origin":{"kind":"user"}}"#],
            )],
        )
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
        let parsed = parse_agents_bundle(
            agents_state(json!({
                "main": { "type": "main", "parentAgentId": null },
                "agent-0": { "type": "sub", "parentAgentId": "main" },
                "agent-1": { "type": "sub", "parentAgentId": "main" }
            })),
            &[
                r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Main"}],"origin":{"kind":"user"}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000010,"event":{"type":"tool.call","toolCallId":"Agent_0","name":"Agent","args":{}}}"#,
                r#"{"type":"context.append_loop_event","event":{"type":"tool.call","toolCallId":"Agent_1","name":"Agent","args":{}}}"#,
            ],
            &[
                (
                    "agent-0",
                    &[
                        r#"{"type":"turn.prompt","time":1785320000020,"input":[{"type":"text","text":"Child 0"}],"origin":{"kind":"user"}}"#,
                    ],
                ),
                (
                    "agent-1",
                    &[
                        r#"{"type":"turn.prompt","time":1785320000030,"input":[{"type":"text","text":"Child 1"}],"origin":{"kind":"user"}}"#,
                    ],
                ),
            ],
        );

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
    fn chronological_agent_matching_rejects_conflicting_competing_owner() {
        let parsed = parse_single_agent_call(&[
            r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Main"}],"origin":{"kind":"user"}}"#,
            r#"{"type":"context.append_loop_event","time":1785320000005,"event":{"type":"tool.call","toolCallId":"Agent_interrupted","name":"Agent","args":{}}}"#,
            r#"{"type":"context.append_loop_event","time":1785320000006,"event":{"type":"tool.result","toolCallId":"Agent_interrupted","result":{"output":"Interrupted by user","isError":true}}}"#,
            r#"{"type":"context.append_loop_event","time":1785320000010,"event":{"type":"tool.call","toolCallId":"Agent_conflict","name":"Agent","args":{"resume":"agent-other"}}}"#,
            r#"{"type":"context.append_loop_event","time":1785320000012,"event":{"type":"tool.result","toolCallId":"Agent_conflict","result":{"output":"agent_id: agent-0\nstatus: completed\n\nDone."}}}"#,
        ]);

        assert!(parsed.subagents.is_empty());
        assert_eq!(parsed.tool_calls.len(), 2);
        assert!(
            parsed
                .tool_calls
                .iter()
                .all(|call| call.tool_name == "Agent")
        );
    }

    #[test]
    fn chronological_agent_matching_rejects_generic_swarm_competing_owner() {
        let parsed = parse_single_agent_call(&[
            r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Main"}],"origin":{"kind":"user"}}"#,
            r#"{"type":"context.append_loop_event","time":1785320000005,"event":{"type":"tool.call","toolCallId":"Agent_interrupted","name":"Agent","args":{}}}"#,
            r#"{"type":"context.append_loop_event","time":1785320000006,"event":{"type":"tool.result","toolCallId":"Agent_interrupted","result":{"output":"Interrupted by user","isError":true}}}"#,
            r#"{"type":"context.append_loop_event","time":1785320000010,"event":{"type":"tool.call","toolCallId":"AgentSwarm_unmatched","name":"AgentSwarm","args":{"items":["A"]}}}"#,
        ]);

        assert!(parsed.subagents.is_empty());
        assert_eq!(parsed.tool_calls.len(), 2);
        assert!(
            parsed
                .tool_calls
                .iter()
                .any(|call| call.tool_name == "AgentSwarm")
        );
    }

    #[test]
    fn agent_result_header_ignores_summary_body_ids() {
        for output in [
            "agent_id: agent-0\nstatus: completed\n\n[summary]\nNested result:\nagent_id: agent-other",
            "agentId: agent-0\nstatus: completed\r\n \r\nNested result:\nagentId: agent-other",
            "agent_id: agent-0",
        ] {
            let result = json!({"type":"context.append_loop_event", "event":{"type":"tool.result", "toolCallId":"Agent_0", "result":{"output":output}}}).to_string();
            let parsed = parse_single_agent_call(&[
                r#"{"type":"turn.prompt","input":[{"type":"text","text":"Main"}],"origin":{"kind":"user"}}"#,
                r#"{"type":"context.append_loop_event","event":{"type":"tool.call","toolCallId":"Agent_0","name":"Agent","args":{}}}"#,
                &result,
            ]);

            assert_eq!(parsed.subagents.len(), 1, "{output}");
            assert_eq!(parsed.subagents[0].agent_id.as_deref(), Some("agent-0"));
            assert!(parsed.tool_calls.is_empty());
        }
    }

    #[test]
    fn agent_result_body_only_ids_are_not_explicit_evidence() {
        let parsed = parse_single_agent_call(&[
            r#"{"type":"turn.prompt","input":[{"type":"text","text":"Main"}],"origin":{"kind":"user"}}"#,
            r#"{"type":"context.append_loop_event","event":{"type":"tool.call","toolCallId":"Agent_0","name":"Agent","args":{}}}"#,
            r#"{"type":"context.append_loop_event","event":{"type":"tool.result","toolCallId":"Agent_0","result":{"output":"Quoted result:\nagent_id: agent-0"}}}"#,
        ]);

        assert!(parsed.subagents.is_empty());
        assert_eq!(parsed.tool_calls[0].tool_name, "Agent");
    }

    #[test]
    fn conflicting_agent_result_header_stays_generic() {
        let parsed = parse_single_agent_call(&[
            r#"{"type":"turn.prompt","input":[{"type":"text","text":"Main"}],"origin":{"kind":"user"}}"#,
            r#"{"type":"context.append_loop_event","event":{"type":"tool.call","toolCallId":"Agent_0","name":"Agent","args":{}}}"#,
            r#"{"type":"context.append_loop_event","event":{"type":"tool.result","toolCallId":"Agent_0","result":{"output":"agent_id: agent-0\nagentId: agent-other\n\nDone."}}}"#,
        ]);

        assert!(parsed.subagents.is_empty());
        assert_eq!(parsed.tool_calls[0].tool_name, "Agent");
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
    fn duplicate_explicit_agent_id_links_both_calls_to_one_child() {
        let parsed = parse_agents_bundle(
            agents_state(json!({
                "main": { "type": "main", "parentAgentId": null },
                "agent-0": { "type": "sub", "parentAgentId": "main" },
                "agent-1": { "type": "sub", "parentAgentId": "main" }
            })),
            &[
                r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Main"}],"origin":{"kind":"user"}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000010,"event":{"type":"tool.call","toolCallId":"Agent_0","name":"Agent","args":{"agent_id":"agent-0"}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000020,"event":{"type":"tool.call","toolCallId":"Agent_1","name":"Agent","args":{"agent_id":"agent-0"}}}"#,
            ],
            &[
                (
                    "agent-0",
                    &[
                        r#"{"type":"turn.prompt","time":1785320000030,"input":[{"type":"text","text":"Child 0"}],"origin":{"kind":"user"}}"#,
                    ],
                ),
                (
                    "agent-1",
                    &[
                        r#"{"type":"turn.prompt","time":1785320000040,"input":[{"type":"text","text":"Child 1"}],"origin":{"kind":"user"}}"#,
                    ],
                ),
            ],
        );

        assert_eq!(parsed.main.subagents.len(), 2);
        assert!(parsed.main.subagents.iter().all(|subagent| {
            subagent.agent_id.as_deref() == Some("agent-0")
                && subagent.child_session_id.as_deref()
                    == Some("kimi-subagent::session_00000000-0000-4000-8000-000000000001::agent-0")
        }));
        assert!(
            parsed
                .main
                .tool_calls
                .iter()
                .all(|call| call.tool_name != "Agent")
        );
        assert!(parsed.main.subagents.iter().all(|subagent| {
            subagent.child_session_id.as_deref()
                != Some("kimi-subagent::session_00000000-0000-4000-8000-000000000001::agent-1")
        }));
    }

    #[test]
    fn resumed_subagent_calls_share_one_child() {
        let child_journal = |time: i64| {
            format!(
                r#"{{"type":"turn.prompt","time":{time},"input":[{{"type":"text","text":"Child"}}],"origin":{{"kind":"user"}}}}"#
            )
        };
        let parsed = parse_agents_bundle(
            agents_state(json!({
                "main": { "type": "main", "parentAgentId": null },
                "agent-0": { "type": "sub", "parentAgentId": "main" },
                "agent-1": { "type": "sub", "parentAgentId": "main" }
            })),
            &[
                r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Main"}],"origin":{"kind":"user"}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000010,"event":{"type":"tool.call","toolCallId":"Agent_15","name":"Agent","args":{}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000020,"event":{"type":"tool.result","toolCallId":"Agent_15","result":{"output":"agent_id: agent-0\nactual_subagent_type: explore\nstatus: completed\n\n[summary]\nFirst child finished."}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000030,"event":{"type":"tool.call","toolCallId":"Agent_16","name":"Agent","args":{}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000040,"event":{"type":"tool.result","toolCallId":"Agent_16","result":{"output":"agent_id: agent-1\nactual_subagent_type: explore\nstatus: failed\n\nsubagent error: The subagent stopped.","isError":true}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000050,"event":{"type":"tool.call","toolCallId":"Agent_18","name":"Agent","args":{"resume":"agent-1"}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000060,"event":{"type":"tool.result","toolCallId":"Agent_18","result":{"output":"agent_id: agent-1\nactual_subagent_type: explore\nstatus: failed\n\nsubagent error: The subagent stopped.","isError":true}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000070,"event":{"type":"tool.call","toolCallId":"Agent_20","name":"Agent","args":{}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000080,"event":{"type":"tool.result","toolCallId":"Agent_20","result":{"output":"agent_id: agent-1\nactual_subagent_type: explore\nstatus: completed\n\n[summary]\nResumed child finished."}}}"#,
            ],
            &[
                ("agent-0", &[child_journal(1785320000025).as_str()]),
                ("agent-1", &[child_journal(1785320000035).as_str()]),
            ],
        );

        assert_eq!(parsed.main.subagents.len(), 4);
        let agent_1_session =
            "kimi-subagent::session_00000000-0000-4000-8000-000000000001::agent-1";
        assert_eq!(
            parsed
                .main
                .subagents
                .iter()
                .filter(|subagent| subagent.child_session_id.as_deref() == Some(agent_1_session))
                .count(),
            3
        );
        assert!(parsed.main.subagents.iter().all(|subagent| {
            subagent.parser_ref.as_deref() != Some("Agent_16")
                || subagent
                    .result_summary
                    .as_deref()
                    .is_some_and(|summary| summary.starts_with("subagent error:"))
        }));
        assert_eq!(
            parsed
                .main
                .subagents
                .iter()
                .find(|subagent| subagent.parser_ref.as_deref() == Some("Agent_20"))
                .and_then(|subagent| subagent.result_summary.as_deref()),
            Some("Resumed child finished.")
        );
    }

    #[test]
    fn resume_with_conflicting_evidence_stays_generic() {
        let child: &[&str] = &[
            r#"{"type":"turn.prompt","time":1785320000050,"input":[{"type":"text","text":"Child"}],"origin":{"kind":"user"}}"#,
        ];
        let parsed = parse_agents_bundle(
            agents_state(json!({
                "main": { "type": "main", "parentAgentId": null },
                "agent-0": { "type": "sub", "parentAgentId": "main" },
                "agent-1": { "type": "sub", "parentAgentId": "main" }
            })),
            &[
                r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Main"}],"origin":{"kind":"user"}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000010,"event":{"type":"tool.call","toolCallId":"Agent_conflict","name":"Agent","args":{"resume":"agent-1"}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000020,"event":{"type":"tool.result","toolCallId":"Agent_conflict","result":{"output":"agent_id: agent-0\nstatus: completed\n\nWrong child."}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000030,"event":{"type":"tool.call","toolCallId":"Agent_valid","name":"Agent","args":{"resume":"agent-1"}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000040,"event":{"type":"tool.result","toolCallId":"Agent_valid","result":{"output":"agent_id: agent-1\nstatus: completed\n\nRight child."}}}"#,
            ],
            &[("agent-0", child), ("agent-1", child)],
        );

        assert_eq!(parsed.main.subagents.len(), 1);
        assert_eq!(
            parsed.main.subagents[0].parser_ref.as_deref(),
            Some("Agent_valid")
        );
        assert!(parsed.main.tool_calls.iter().any(|call| {
            call.parser_call_id.as_deref() == Some("Agent_conflict") && call.tool_name == "Agent"
        }));
    }

    #[test]
    fn agent_name_prefers_profile_then_header_then_args() {
        let child: &[&str] = &[
            r#"{"type":"turn.prompt","input":[{"type":"text","text":"Child"}],"origin":{"kind":"user"}}"#,
        ];
        let parsed = parse_agents_bundle(
            agents_state(json!({
                "main": { "type": "main", "parentAgentId": null },
                "agent-profile": { "type": "sub", "parentAgentId": "main", "labels": { "profileName": "profile-name" } },
                "agent-header": { "type": "sub", "parentAgentId": "main" },
                "agent-args": { "type": "sub", "parentAgentId": "main" }
            })),
            &[
                r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Main"}],"origin":{"kind":"user"}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000010,"event":{"type":"tool.call","toolCallId":"Agent_profile","name":"Agent","args":{"agent_id":"agent-profile","subagent_type":"args-name"}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000020,"event":{"type":"tool.result","toolCallId":"Agent_profile","result":{"output":"agent_id: agent-profile\nactual_subagent_type: header-name\nstatus: completed\n\nDone."}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000030,"event":{"type":"tool.call","toolCallId":"Agent_header","name":"Agent","args":{"agent_id":"agent-header","subagent_type":"args-name"}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000040,"event":{"type":"tool.result","toolCallId":"Agent_header","result":{"output":"agent_id: agent-header\nactual_subagent_type: header-name\nstatus: completed\n\nDone."}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000050,"event":{"type":"tool.call","toolCallId":"Agent_args","name":"Agent","args":{"agent_id":"agent-args","subagent_type":"args-name"}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000060,"event":{"type":"tool.result","toolCallId":"Agent_args","result":{"output":"agent_id: agent-args\nstatus: completed\n\nDone."}}}"#,
            ],
            &[
                ("agent-profile", child),
                ("agent-header", child),
                ("agent-args", child),
            ],
        );
        let names: HashMap<_, _> = parsed
            .main
            .subagents
            .iter()
            .map(|subagent| {
                (
                    subagent.parser_ref.as_deref().unwrap(),
                    subagent.agent_name.as_deref(),
                )
            })
            .collect();

        assert_eq!(names["Agent_profile"], Some("profile-name"));
        assert_eq!(names["Agent_header"], Some("header-name"));
        assert_eq!(names["Agent_args"], Some("args-name"));
    }

    #[test]
    fn swarm_children_are_excluded_from_chronological_pairing() {
        let parsed = parse_agents_bundle(
            agents_state(json!({
                "main": { "type": "main", "parentAgentId": null },
                "agent-swarm": { "type": "sub", "parentAgentId": "main", "labels": { "swarmItem": "A" } },
                "agent-ordinary": { "type": "sub", "parentAgentId": "main" }
            })),
            &[
                r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Main"}],"origin":{"kind":"user"}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000010,"event":{"type":"tool.call","toolCallId":"Agent_0","name":"Agent","args":{}}}"#,
            ],
            &[
                (
                    "agent-swarm",
                    &[
                        r#"{"type":"turn.prompt","time":1785320000020,"input":[{"type":"text","text":"Swarm child"}],"origin":{"kind":"user"}}"#,
                    ],
                ),
                (
                    "agent-ordinary",
                    &[
                        r#"{"type":"turn.prompt","time":1785320000030,"input":[{"type":"text","text":"Ordinary child"}],"origin":{"kind":"user"}}"#,
                    ],
                ),
            ],
        );

        assert_eq!(parsed.main.subagents.len(), 1);
        assert_eq!(
            parsed.main.subagents[0].agent_id.as_deref(),
            Some("agent-ordinary")
        );
    }

    #[test]
    fn conflicting_structured_agent_evidence_remains_generic() {
        let parsed = parse_agents_bundle(
            agents_state(json!({
                "main": { "type": "main", "parentAgentId": null },
                "agent-0": { "type": "sub", "parentAgentId": "main" }
            })),
            &[
                r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Main"}],"origin":{"kind":"user"}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000010,"event":{"type":"tool.call","toolCallId":"Agent_0","name":"Agent","args":{"agent_id":"agent-0"}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000020,"event":{"type":"tool.result","toolCallId":"Agent_0","result":{"agent_id":"unknown-agent","output":"done"}}}"#,
            ],
            &[(
                "agent-0",
                &[
                    r#"{"type":"turn.prompt","time":1785320000030,"input":[{"type":"text","text":"Child"}],"origin":{"kind":"user"}}"#,
                ],
            )],
        );

        assert!(parsed.main.subagents.is_empty());
        assert_eq!(parsed.main.tool_calls[0].tool_name, "Agent");
    }

    #[test]
    fn conflicting_structured_and_text_agent_evidence_remains_generic() {
        let parsed = parse_agents_bundle(
            agents_state(json!({
                "main": { "type": "main", "parentAgentId": null },
                "agent-0": { "type": "sub", "parentAgentId": "main" },
                "agent-1": { "type": "sub", "parentAgentId": "main" }
            })),
            &[
                r#"{"type":"turn.prompt","time":1785320000000,"input":[{"type":"text","text":"Main"}],"origin":{"kind":"user"}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000010,"event":{"type":"tool.call","toolCallId":"Agent_0","name":"Agent","args":{"agent_id":"agent-0"}}}"#,
                r#"{"type":"context.append_loop_event","time":1785320000020,"event":{"type":"tool.result","toolCallId":"Agent_0","result":{"output":"agent_id: agent-1"}}}"#,
            ],
            &[
                (
                    "agent-0",
                    &[
                        r#"{"type":"turn.prompt","time":1785320000030,"input":[{"type":"text","text":"Child 0"}],"origin":{"kind":"user"}}"#,
                    ],
                ),
                (
                    "agent-1",
                    &[
                        r#"{"type":"turn.prompt","time":1785320000040,"input":[{"type":"text","text":"Child 1"}],"origin":{"kind":"user"}}"#,
                    ],
                ),
            ],
        );

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
mod expansion;
mod swarm;
mod text;

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
    profile_name: Option<String>,
    swarm_item: Option<String>,
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
    kind: DelegationKind,
    raw_call_id: String,
    transcript_item_index: i64,
    call_time_ms: Option<i64>,
    args: Option<Value>,
    result: Option<Value>,
    result_text: Option<String>,
    result_is_error: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DelegationKind {
    Agent,
    Swarm,
}

struct ChildLink<'a> {
    agent_id: &'a str,
    session_id: &'a str,
    first_wire_time_ms: Option<i64>,
    swarm_item: Option<&'a str>,
    profile_name: Option<&'a str>,
}

struct ChildInfo {
    agent_id: String,
    parent_id: String,
    session_id: String,
    first_wire_time_ms: Option<i64>,
    swarm_item: Option<String>,
    profile_name: Option<String>,
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
    record_usage: UsageAccumulator,
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
        let scan = scan_journal(&journal_path, false)?;
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
            false,
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
            let scan = scan_journal(&agent.journal_path, true)?;
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
                true,
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

        let child_infos: Vec<ChildInfo> = children
            .iter()
            .map(|child| {
                let metadata = state.agents.get(&child.agent.id);
                ChildInfo {
                    agent_id: child.agent.id.clone(),
                    parent_id: child.agent.parent_id.clone(),
                    session_id: child.parsed.session.id.clone(),
                    first_wire_time_ms: child.first_wire_time_ms,
                    swarm_item: metadata.and_then(|metadata| metadata.swarm_item.clone()),
                    profile_name: metadata.and_then(|metadata| metadata.profile_name.clone()),
                }
            })
            .collect();
        let main_children = child_links_for_parent(&child_infos, "main");
        match_agent_calls(&mut parsed, &main_agent_calls, &main_children);
        for child in &mut children {
            let parent_id = child.agent.id.as_str();
            let agent_calls = child.agent_calls.clone();
            let immediate_children = child_links_for_parent(&child_infos, parent_id);
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
        let Some(file) = read_optional_metadata(path, File::open) else {
            return;
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
        let Some(bytes) = read_optional_metadata(path, fs::read) else {
            return;
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

/// Read an optional Kimi metadata file, or `None` when it is absent, is not a
/// regular file, or cannot be read. Callers share this preamble so the
/// not-found handling and the diagnostics stay identical across metadata kinds.
///
/// The lifetime is explicit on purpose: eliding it makes the bound higher-ranked,
/// which `File::open` and `fs::read` cannot satisfy as bare function items.
fn read_optional_metadata<'a, T>(
    path: &'a Path,
    read: impl FnOnce(&'a Path) -> std::io::Result<T>,
) -> Option<T> {
    if !validate_optional_metadata_file(path) {
        return None;
    }
    match read(path) {
        Ok(value) => Some(value),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => {
            tracing::warn!(path = %path.display(), "failed to read optional Kimi metadata");
            None
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

fn nonblank_verbatim(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
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
    if let Some((header, _)) = candidate
        .result_text
        .as_deref()
        .and_then(text::split_agent_result_header)
    {
        for line in header.lines() {
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

fn child_links_for_parent<'a>(child_infos: &'a [ChildInfo], parent_id: &str) -> Vec<ChildLink<'a>> {
    child_infos
        .iter()
        .filter(|info| info.parent_id == parent_id)
        .map(|info| ChildLink {
            agent_id: info.agent_id.as_str(),
            session_id: info.session_id.as_str(),
            first_wire_time_ms: info.first_wire_time_ms,
            swarm_item: info.swarm_item.as_deref(),
            profile_name: info.profile_name.as_deref(),
        })
        .collect()
}

fn match_agent_calls(
    parent: &mut ParsedSession,
    candidates: &[AgentCallCandidate],
    children: &[ChildLink<'_>],
) {
    let swarm_children: Vec<_> = children
        .iter()
        .map(|child| swarm::SwarmChild {
            agent_id: child.agent_id,
            swarm_item: child.swarm_item,
            first_wire_time_ms: child.first_wire_time_ms,
        })
        .collect();
    let mut swarm_candidates: Vec<_> = candidates
        .iter()
        .filter(|candidate| candidate.kind == DelegationKind::Swarm)
        .filter_map(|candidate| {
            // Only absence permits result-order resolution. An invalid present
            // list must retain its generic call, even with an otherwise valid result.
            let items = match candidate.args.as_ref().and_then(|args| args.get("items")) {
                None => None,
                Some(items) => Some(
                    items
                        .as_array()?
                        .iter()
                        .map(|item| item.as_str().map(str::to_string))
                        .collect::<Option<Vec<_>>>()?,
                ),
            };
            Some((candidate, items))
        })
        .collect();
    swarm_candidates.sort_by_key(|(candidate, _)| candidate.transcript_item_index);
    let mut taken = HashSet::new();
    let mut consumed_at = HashMap::new();
    let mut expansions = Vec::new();
    let mut removed_tool_ids = HashSet::new();
    let swarm_calls: Vec<_> = swarm_candidates
        .iter()
        .map(|(candidate, items)| swarm::SwarmCall {
            items: items.as_deref(),
            call_time_ms: candidate.call_time_ms,
            result_output: candidate.result_text.as_deref(),
            result_is_error: candidate.result_is_error,
        })
        .collect();
    let resolved_swarms = swarm::resolve_swarms(&swarm_calls, &swarm_children);
    for ((candidate, _), members) in swarm_candidates.into_iter().zip(resolved_swarms) {
        let Some(members) = members.filter(|members| !members.is_empty()) else {
            continue;
        };
        let args = candidate.args.as_ref();
        let template = args
            .and_then(|args| args.get("prompt_template"))
            .and_then(Value::as_str);
        let mut subagent_ids = Vec::with_capacity(members.len());
        for member in members {
            // Resolution only returns children from this parent's child list.
            let Some(child) = children
                .iter()
                .find(|child| child.agent_id == member.agent_id)
            else {
                continue;
            };
            let id = format!(
                "kimi-agent-call::{}::{}::{}",
                parent.session.id, candidate.raw_call_id, member.agent_id
            );
            subagent_ids.push(id.clone());
            taken.insert(member.agent_id.clone());
            consumed_at.insert(member.agent_id.clone(), candidate.transcript_item_index);
            parent.subagents.push(Subagent {
                id,
                agent_id: Some(member.agent_id.clone()),
                agent_name: child.profile_name.map(str::to_string).or_else(|| {
                    args.and_then(|args| args.get("subagent_type"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                }),
                session_id: parent.session.id.clone(),
                title: member.item.clone().unwrap_or(member.agent_id),
                prompt: template
                    .zip(member.item.as_deref())
                    .map(|(template, item)| template.replace("{{item}}", item)),
                result_summary: member.summary,
                child_session_id: Some(child.session_id.to_string()),
                parser_ref: Some(candidate.raw_call_id.clone()),
            });
        }
        expansions.push(expansion::Expansion {
            item_index: candidate.transcript_item_index,
            subagent_ids,
        });
        removed_tool_ids.insert(format!(
            "kimi-tool::{}::{}",
            parent.session.id, candidate.raw_call_id
        ));
    }
    let mut matched = HashMap::new();
    let mut matched_children = taken;
    let evidence: HashMap<_, _> = candidates
        .iter()
        .map(|candidate| (candidate.raw_call_id.as_str(), agent_evidence(candidate)))
        .collect();
    for candidate in candidates {
        if candidate.kind == DelegationKind::Agent
            && let AgentEvidence::Valid(id) = &evidence[candidate.raw_call_id.as_str()]
            && children.iter().any(|child| child.agent_id == id)
            && consumed_at.get(id).is_none_or(|swarm_index| {
                candidate.transcript_item_index > *swarm_index
                    && candidate
                        .args
                        .as_ref()
                        .and_then(|args| args.get("resume"))
                        .and_then(Value::as_str)
                        .is_some_and(|resume| resume.trim() == id)
            })
        {
            matched.insert(candidate.raw_call_id.clone(), id.clone());
            matched_children.insert(id.clone());
        }
    }
    // Unmatched explicit/invalid evidence and unresolved swarms can own the
    // remaining children too; excluding them from the count would invent links.
    let has_competing_owner = candidates.iter().any(|candidate| match candidate.kind {
        DelegationKind::Agent => {
            !matched.contains_key(&candidate.raw_call_id)
                && evidence[candidate.raw_call_id.as_str()] != AgentEvidence::Absent
        }
        DelegationKind::Swarm => !removed_tool_ids.contains(&format!(
            "kimi-tool::{}::{}",
            parent.session.id, candidate.raw_call_id
        )),
    });
    let remaining_calls: Vec<_> = candidates
        .iter()
        .filter(|candidate| {
            candidate.kind == DelegationKind::Agent
                && !matched.contains_key(&candidate.raw_call_id)
                && evidence[candidate.raw_call_id.as_str()] == AgentEvidence::Absent
        })
        .collect();
    let mut remaining_children: Vec<_> = children
        .iter()
        .filter(|child| !matched_children.contains(child.agent_id) && child.swarm_item.is_none())
        .collect();
    if !has_competing_owner
        && remaining_calls.len() == remaining_children.len()
        && remaining_calls
            .iter()
            .all(|candidate| candidate.call_time_ms.is_some())
        && remaining_children
            .iter()
            .all(|child| child.first_wire_time_ms.is_some())
    {
        let mut ordered_calls = remaining_calls;
        ordered_calls
            .sort_by_key(|candidate| (candidate.call_time_ms, candidate.raw_call_id.as_str()));
        remaining_children.sort_by_key(|child| (child.first_wire_time_ms, child.agent_id));
        if ordered_calls
            .iter()
            .zip(&remaining_children)
            .all(|(candidate, child)| {
                child.first_wire_time_ms.expect("verified")
                    >= candidate.call_time_ms.expect("verified")
            })
        {
            for (candidate, child) in ordered_calls.into_iter().zip(remaining_children) {
                matched.insert(candidate.raw_call_id.clone(), child.agent_id.to_string());
            }
        }
    }

    for candidate in candidates {
        let Some(agent_id) = matched.get(&candidate.raw_call_id) else {
            continue;
        };
        let Some(child) = children.iter().find(|child| child.agent_id == agent_id) else {
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
            agent_name: child
                .profile_name
                .map(str::to_string)
                .or_else(|| {
                    candidate
                        .result_text
                        .as_deref()
                        .and_then(|text| text::split_agent_result(text).actual_subagent_type)
                        .filter(|name| !name.is_empty())
                        .map(str::to_string)
                })
                .or_else(|| {
                    candidate
                        .args
                        .as_ref()
                        .and_then(|value| value.get("subagent_type"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                }),
            session_id: parent.session.id.clone(),
            title: agent_id.clone(),
            prompt: candidate
                .args
                .as_ref()
                .and_then(|value| value.get("prompt"))
                .and_then(Value::as_str)
                .map(str::to_string),
            result_summary: candidate
                .result_text
                .as_deref()
                .map(|text| text::split_agent_result(text).summary)
                .filter(|summary| !summary.is_empty())
                .map(str::to_string),
            child_session_id: Some(child.session_id.to_string()),
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
    expansion::expand_transcript(parent, expansions);
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
                    parent_agent_id: nonblank(agent.get("parentAgentId")).or_else(|| {
                        nonblank(
                            agent
                                .get("labels")
                                .and_then(|labels| labels.get("parentAgentId")),
                        )
                    }),
                    profile_name: nonblank(
                        agent
                            .get("labels")
                            .and_then(|labels| labels.get("profileName")),
                    ),
                    swarm_item: nonblank_verbatim(
                        agent
                            .get("labels")
                            .and_then(|labels| labels.get("swarmItem")),
                    )
                    .or_else(|| nonblank_verbatim(agent.get("swarmItem"))),
                },
            );
        }
    }
    Ok(state)
}

fn scan_journal(path: &Path, is_child: bool) -> Result<JournalScan> {
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
        scan.has_real_turn_prompt |= is_real_turn_prompt(&value, is_child);
    }
    Ok(scan)
}

fn parse_journal(
    path: &Path,
    session_id: &str,
    start_time: DateTime<Utc>,
    has_real_turn_prompt: bool,
    is_child: bool,
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
            Some("usage.record") => {
                if let Some(usage) = parse_usage(value.get("usage")) {
                    state.record_usage.add(usage);
                }
            }
            Some("turn.prompt")
                if has_real_turn_prompt && is_real_turn_prompt(&value, is_child) =>
            {
                if let Some(content) = normalize_content_parts(value.get("input"), true) {
                    let content = if is_child {
                        text::strip_git_context(&content).to_string()
                    } else {
                        content
                    };
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
                        if matches!(name.as_str(), "Agent" | "AgentSwarm") {
                            state.agent_calls.push(AgentCallCandidate {
                                kind: if name == "AgentSwarm" {
                                    DelegationKind::Swarm
                                } else {
                                    DelegationKind::Agent
                                },
                                raw_call_id: raw_id.clone(),
                                transcript_item_index: transcript_items
                                    .last()
                                    .expect("tool call added a transcript item")
                                    .item_index,
                                call_time_ms: wire_time.map(|(millis, _)| millis),
                                args: event.get("args").cloned(),
                                result: None,
                                result_text: None,
                                result_is_error: false,
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
                            candidate.result_is_error = result
                                .and_then(|result| result.get("isError"))
                                .and_then(Value::as_bool)
                                == Some(true);
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
            token_usage: if state.record_usage.seen {
                state.record_usage.into_token_usage()
            } else {
                state.step_usage.into_token_usage()
            },
            agent_path: None,
            context_entries: Vec::new(),
        },
        first_wire_time_ms,
        agent_calls: state.agent_calls,
    })
}

fn is_real_turn_prompt(value: &Value, is_child: bool) -> bool {
    let Some(origin) = value.get("origin") else {
        return false;
    };
    let origin_kind = origin.get("kind").and_then(Value::as_str);
    let has_accepted_origin = origin_kind == Some("user")
        || (is_child
            && origin_kind == Some("system_trigger")
            && origin.get("name").and_then(Value::as_str) == Some("subagent"));
    value.get("type").and_then(Value::as_str) == Some("turn.prompt")
        && has_accepted_origin
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
        context_id: None,
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
        context_id: None,
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
