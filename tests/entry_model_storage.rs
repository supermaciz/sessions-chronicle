mod helpers;

use helpers::TempDatabase;
use sessions_chronicle::database::{
    insert_subagent, insert_tool_call, load_subagent, load_tool_call,
};
use sessions_chronicle::models::{Subagent, ToolCall, ToolCallStatus};

fn tool_call(id: &str, model: Option<&str>) -> ToolCall {
    ToolCall {
        id: id.into(),
        session_id: "s".into(),
        subagent_id: None,
        tool_name: "Read".into(),
        status: ToolCallStatus::Completed,
        title: None,
        summary: None,
        input_json: None,
        output_text: None,
        error_text: None,
        started_at: None,
        ended_at: None,
        duration_ms: None,
        parser_call_id: None,
        model: model.map(str::to_owned),
    }
}

fn subagent(id: &str, model: Option<&str>) -> Subagent {
    Subagent {
        id: id.into(),
        agent_id: None,
        agent_name: None,
        session_id: "s".into(),
        title: "agent".into(),
        prompt: None,
        result_summary: None,
        child_session_id: None,
        parser_ref: None,
        source_model: model.map(str::to_owned),
    }
}

#[test]
fn tool_call_model_round_trips() {
    let db = TempDatabase::new("entry-model");
    for (id, model) in [("a", Some("gpt-5")), ("b", None)] {
        insert_tool_call(&db.connection, &tool_call(id, model), "s").unwrap();
        assert_eq!(
            load_tool_call(&db.path, "s", id)
                .unwrap()
                .unwrap()
                .model
                .as_deref(),
            model
        );
    }
}

#[test]
fn subagent_source_model_round_trips() {
    let db = TempDatabase::new("entry-model");
    for (id, model) in [("a", Some("gpt-5")), ("b", None)] {
        insert_subagent(&db.connection, &subagent(id, model), "s").unwrap();
        assert_eq!(
            load_subagent(&db.path, "s", id)
                .unwrap()
                .unwrap()
                .source_model
                .as_deref(),
            model
        );
    }
}

#[test]
fn insert_replaces_model_with_null() {
    let db = TempDatabase::new("entry-model");
    for model in [Some("gpt-5"), None, Some("o3")] {
        insert_tool_call(&db.connection, &tool_call("a", model), "s").unwrap();
        insert_subagent(&db.connection, &subagent("a", model), "s").unwrap();
        assert_eq!(
            load_tool_call(&db.path, "s", "a")
                .unwrap()
                .unwrap()
                .model
                .as_deref(),
            model
        );
        assert_eq!(
            load_subagent(&db.path, "s", "a")
                .unwrap()
                .unwrap()
                .source_model
                .as_deref(),
            model
        );
    }
}
