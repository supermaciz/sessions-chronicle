use std::path::Path;

use sessions_chronicle::models::{Role, ToolCallStatus, TranscriptItemKind};
use sessions_chronicle::parsers::kimi_code::KimiCodeParser;

#[test]
fn rich_fixture_bundle_round_trips_normalized_content() {
    let home = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/kimi_home");
    let session_dir =
        home.join("sessions/wd_primary_aaaaaaaaaaaa/session_00000000-0000-4000-8000-000000000001");
    let bundle = KimiCodeParser::new(&home)
        .parse_session_dir(&session_dir)
        .unwrap();

    assert_eq!(bundle.main.session.tool.to_storage(), "kimi_code");
    assert_eq!(
        bundle.main.session.project_path.as_deref(),
        Some("/tmp/kimi-fixture-primary")
    );
    assert_eq!(
        bundle
            .main
            .messages
            .iter()
            .filter(|m| m.role == Role::User)
            .count(),
        1
    );
    assert!(
        bundle
            .main
            .tool_calls
            .iter()
            .any(|call| call.status == ToolCallStatus::Error)
    );
    assert!(
        bundle
            .main
            .tool_calls
            .iter()
            .any(|call| call.status == ToolCallStatus::Pending)
    );
    assert!(
        bundle
            .main
            .transcript_items
            .iter()
            .any(|item| item.kind == TranscriptItemKind::Subagent)
    );
    assert_eq!(bundle.children.len(), 3);
    assert!(
        bundle
            .session_ids
            .contains("kimi-subagent::session_00000000-0000-4000-8000-000000000001::agent-nested")
    );
}

#[test]
fn project_fallback_and_directory_identity_fixtures_follow_precedence() {
    let home = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/kimi_home");
    let parser = KimiCodeParser::new(&home);
    let parse = |bucket: &str, id: &str| {
        parser
            .parse_session_dir(&home.join("sessions").join(bucket).join(id))
            .unwrap()
    };

    assert_eq!(
        parse(
            "wd_index_cccccccccc",
            "session_00000000-0000-4000-8000-000000000003"
        )
        .main
        .session
        .project_path
        .as_deref(),
        Some("/tmp/kimi-fixture-index")
    );
    assert_eq!(
        parse(
            "wd_workspace_dddddddddddd",
            "session_00000000-0000-4000-8000-000000000004"
        )
        .main
        .session
        .project_path
        .as_deref(),
        Some("/tmp/kimi-fixture-workspace")
    );
    let conflict = parse(
        "wd_conflict_eeeeeeeeeeee",
        "session_00000000-0000-4000-8000-000000000005",
    );
    assert_eq!(
        conflict.main.session.id,
        "session_00000000-0000-4000-8000-000000000005"
    );
    assert_eq!(
        conflict.main.session.project_path.as_deref(),
        Some("/tmp/kimi-fixture-workdir")
    );
}
