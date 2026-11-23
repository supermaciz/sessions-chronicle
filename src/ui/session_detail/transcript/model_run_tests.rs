use super::super::tests::{
    transcript_context_row, transcript_message_row, transcript_subagent_row, transcript_tool_row,
};
use super::model_attribution::{ModelRunMarker, ModelRunText, model_run_markers};
use crate::database::TranscriptItemRow;
use crate::models::{ContextKind, Role};

fn assistant(index: i64, model: Option<&str>) -> TranscriptItemRow {
    let mut row = transcript_message_row(index, Role::Assistant, "response");
    row.model = model.map(str::to_string);
    row
}

fn markers(rows: &[TranscriptItemRow]) -> Vec<(i64, Option<String>, Option<String>)> {
    model_run_markers(rows)
        .into_iter()
        .map(|(index, marker)| (index, marker.model, marker.previous))
        .collect()
}

#[test]
fn model_run_markers_cover_switch_back_and_null_runs() {
    let (a, b) = (Some("A"), Some("B"));
    for (models, expected) in [
        ([a, a, b, a], [(0, a, None), (2, b, a), (3, a, b)]),
        (
            [None, a, None, a],
            [(1, a, None), (2, None, a), (3, a, None)],
        ),
    ] {
        let rows: Vec<_> = models
            .into_iter()
            .enumerate()
            .map(|(index, model)| assistant(index as i64, model))
            .collect();
        assert_eq!(
            markers(&rows),
            expected.map(|(i, model, previous)| (
                i,
                model.map(str::to_string),
                previous.map(str::to_string),
            )),
        );
    }
    assert!(markers(&[assistant(0, None), assistant(1, None)]).is_empty());
    assert!(markers(&[assistant(0, Some("")), assistant(1, Some(" \t"))]).is_empty());
}

#[test]
fn model_run_markers_continue_across_transparent_rows() {
    let rows = [
        assistant(0, Some("A")),
        transcript_message_row(1, Role::User, "question"),
        transcript_message_row(2, Role::ToolResult, "result"),
        transcript_context_row(3, "ctx-1", ContextKind::System),
        assistant(4, Some("A")),
    ];
    assert_eq!(markers(&rows), [(0, Some("A".into()), None)]);
}

#[test]
fn model_run_markers_skip_legacy_null_calls_and_sole_spawn_end() {
    let legacy_null_calls = [
        assistant(0, Some("A")),
        transcript_tool_row(1, "Read"),
        transcript_tool_row(2, "Edit"),
        assistant(3, Some("A")),
    ];
    assert_eq!(markers(&legacy_null_calls), [(0, Some("A".into()), None)]);

    let sole_spawn_end_between_a_messages = [
        assistant(0, Some("A")),
        transcript_subagent_row(1, "Delegation"),
        assistant(2, Some("A")),
    ];
    assert_eq!(
        markers(&sole_spawn_end_between_a_messages),
        [(0, Some("A".into()), None)]
    );
}

#[test]
fn model_run_markers_use_attributed_delegation_to_disable_legacy_heuristic() {
    let mut delegation = transcript_subagent_row(3, "Delegation");
    delegation.subagent_source_model = Some(" A ".into());
    let rows = [
        assistant(0, Some("A")),
        transcript_tool_row(1, "Read"),
        assistant(2, Some("A")),
        delegation,
    ];
    assert_eq!(
        markers(&rows),
        [
            (0, Some("A".into()), None),
            (1, None, Some("A".into())),
            (2, Some("A".into()), None),
            (3, Some(" A ".into()), Some("A".into())),
        ],
    );
}

#[test]
fn model_run_presentation_prepares_caption_and_style_for_known_and_null_models() {
    let known = ModelRunText::from(&ModelRunMarker {
        model: Some("A<&".into()),
        previous: None,
    });
    assert_eq!(known.caption, "A<&");
    assert!(known.monospace);
    assert!(known.previous_markup.is_none());
    assert_eq!(known.description, "Model: A<&");

    let unknown = ModelRunText::from(&ModelRunMarker {
        model: None,
        previous: Some("A<&".into()),
    });
    assert_eq!(unknown.caption, "model not recorded");
    assert!(!unknown.monospace);
    assert_eq!(
        unknown.previous_markup.as_deref(),
        Some("from <tt>A&lt;&amp;</tt>")
    );
    assert_eq!(
        unknown.description,
        "Model changed to model not recorded from A<&"
    );
}
