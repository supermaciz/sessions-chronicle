use sessions_chronicle::database::TranscriptItemRow;
use sessions_chronicle::models::{ToolCallStatus, TranscriptItemKind};
use sessions_chronicle::ui::transcript_row::{TranscriptItemInit, transcript_item_init_from_row};
use std::path::PathBuf;
use std::sync::Arc;

fn tool_row(
    item_index: i64,
    tool_name: &str,
    input_json: &str,
    output_text: Option<&str>,
    summary: Option<&str>,
) -> TranscriptItemRow {
    TranscriptItemRow {
        item_index,
        kind: TranscriptItemKind::ToolCall,
        message_index: None,
        role: None,
        content_preview: None,
        content_len: None,
        timestamp: None,
        model: None,
        tool_call_id: Some(format!("call-{item_index}")),
        tool_name: Some(tool_name.to_string()),
        tool_status: Some(ToolCallStatus::Completed),
        tool_summary: summary.map(str::to_string),
        tool_input_json: Some(input_json.to_string()),
        tool_output_text: output_text.map(str::to_string),
        duration_ms: Some(25),
        subagent_id: None,
        subagent_title: None,
        subagent_prompt: None,
    }
}

fn preview_from_row(row: &TranscriptItemRow) -> Option<String> {
    let init = transcript_item_init_from_row(
        row,
        "session-1",
        None,
        Arc::new(PathBuf::from("/tmp/sessions-chronicle-test.db")),
    );
    let TranscriptItemInit::ToolCall(tool_init) = init else {
        panic!("expected tool call init");
    };
    tool_init.preview
}

#[test]
fn transcript_tool_items_emit_contextual_preview_strings() {
    let bash_row = tool_row(
        1,
        "bash",
        r#"{"command":"cargo test --all && cargo clippy --all"}"#,
        Some("Process exited with code 0"),
        Some("bash summary"),
    );
    let read_row = tool_row(
        2,
        "read",
        r#"{"file_path":"src/ui/transcript_row.rs","offset":42,"limit":20}"#,
        None,
        Some("read summary"),
    );
    let edit_row = tool_row(
        3,
        "edit",
        r#"{"file_path":"src/ui/tool_preview.rs","old_string":"a\nb","new_string":"a\nb\nc"}"#,
        None,
        Some("edit summary"),
    );
    let grep_row = tool_row(
        4,
        "grep",
        r#"{"pattern":"transcript_item_init_from_row"}"#,
        Some("Found 4 matches"),
        Some("grep summary"),
    );

    let bash_preview = preview_from_row(&bash_row).expect("bash preview should exist");
    assert!(!bash_preview.is_empty());
    assert!(bash_preview.starts_with("$ cargo test --all"));

    let read_preview = preview_from_row(&read_row).expect("read preview should exist");
    assert!(!read_preview.is_empty());
    assert!(read_preview.contains("transcript_row.rs:42-61"));

    let edit_preview = preview_from_row(&edit_row).expect("edit preview should exist");
    assert!(!edit_preview.is_empty());
    assert!(edit_preview.contains("tool_preview.rs +1 -0"));

    let grep_preview = preview_from_row(&grep_row).expect("grep preview should exist");
    assert!(!grep_preview.is_empty());
    assert!(grep_preview.contains("pattern=\"transcript_item_init_from_row\""));
    assert!(grep_preview.contains("4 matches"));
}

#[test]
fn transcript_tool_item_preview_falls_back_to_summary_when_extractor_returns_none() {
    let row = tool_row(
        5,
        "bash",
        "{not-json}",
        None,
        Some("fallback summary from db"),
    );

    assert_eq!(
        preview_from_row(&row).as_deref(),
        Some("fallback summary from db")
    );
}
