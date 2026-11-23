use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionModels {
    pub models: Vec<SessionModelUsage>,
    pub unattributed_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionModelUsage {
    pub model: String,
    pub entry_count: usize,
    pub first_item_index: i64,
    pub first_used_at: Option<i64>,
}

pub fn load_session_models(db_path: &Path, session_id: &str) -> Result<SessionModels> {
    let mut result = SessionModels::default();
    if !db_path.exists() {
        return Ok(result);
    }
    let db = super::open_connection(db_path)?;
    let mut stmt = db.prepare(
        "SELECT ti.item_index,
                CASE ti.kind WHEN 'message' THEN m.model
                             WHEN 'tool_call' THEN tc.model
                             WHEN 'subagent' THEN sa.source_model END,
                CASE ti.kind WHEN 'message' THEN m.timestamp
                             WHEN 'tool_call' THEN tc.started_at END
         FROM transcript_items ti
         JOIN sessions s ON s.id = ti.session_id
         LEFT JOIN messages m ON ti.session_id = m.session_id AND ti.message_index = m.message_index
         LEFT JOIN tool_calls tc ON ti.session_id = tc.session_id AND ti.tool_call_id = tc.id
         LEFT JOIN subagents sa ON ti.session_id = sa.session_id AND ti.subagent_id = sa.id
         WHERE ti.session_id = ?1 AND
               ((ti.kind = 'message' AND m.role = 'assistant')
                 OR ti.kind IN ('tool_call', 'subagent'))
         ORDER BY ti.item_index",
    )?;
    let mut rows = stmt.query([session_id])?;
    let mut indices = HashMap::<String, usize>::new();
    while let Some(row) = rows.next()? {
        let model: Option<String> = row.get(1)?;
        match model.filter(|model| !model.trim().is_empty()) {
            Some(model) => {
                if let Some(&index) = indices.get(&model) {
                    result.models[index].entry_count += 1;
                } else {
                    indices.insert(model.clone(), result.models.len());
                    result.models.push(SessionModelUsage {
                        model,
                        entry_count: 1,
                        first_item_index: row.get(0)?,
                        first_used_at: row.get(2)?,
                    });
                }
            }
            None => result.unattributed_count += 1,
        }
    }
    Ok(result)
}
