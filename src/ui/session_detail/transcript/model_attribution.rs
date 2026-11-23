use std::collections::BTreeMap;

use crate::database::{SessionModels, TranscriptItemRow};
use crate::models::{Role, TranscriptItemKind};
use relm4::gtk;

const MODEL_NOT_RECORDED: &str = "model not recorded";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelRunMarker {
    pub model: Option<String>,
    pub previous: Option<String>,
}

impl ModelRunMarker {
    fn caption(&self) -> &str {
        self.model.as_deref().unwrap_or(MODEL_NOT_RECORDED)
    }
}

/// Display text and style prepared before a row is bound.
#[derive(Clone, Debug)]
pub struct ModelRunText {
    pub caption: String,
    pub monospace: bool,
    pub previous_markup: Option<String>,
    pub description: String,
}

impl From<&ModelRunMarker> for ModelRunText {
    fn from(marker: &ModelRunMarker) -> Self {
        let caption = marker.caption();
        Self {
            caption: caption.to_string(),
            monospace: marker.model.is_some(),
            previous_markup: marker
                .previous
                .as_deref()
                .map(|model| format!("from <tt>{}</tt>", gtk::glib::markup_escape_text(model))),
            description: match &marker.previous {
                Some(previous) => format!("Model changed to {caption} from {previous}"),
                None => format!("Model: {caption}"),
            },
        }
    }
}

#[derive(Clone, Debug)]
pub struct CollapsedModelHint {
    pub caption: String,
    pub tooltip: String,
}

impl CollapsedModelHint {
    pub fn from_changes(changes: &[ModelRunMarker]) -> Option<Self> {
        let last = changes.last()?;
        Some(Self {
            caption: format!("→ {}", last.caption()),
            tooltip: changes
                .iter()
                .map(ModelRunMarker::caption)
                .collect::<Vec<_>>()
                .join("\n"),
        })
    }
}

fn recorded_model(value: Option<&str>) -> Option<&str> {
    value.filter(|model| !model.trim().is_empty())
}

/// Compute markers over all loaded rows, ordered by `item_index`.
pub fn model_run_markers(rows: &[TranscriptItemRow]) -> BTreeMap<i64, ModelRunMarker> {
    let mut known_assistant_message = false;
    let mut saw_call_or_delegation = false;
    let mut known_call_or_delegation = false;
    for row in rows {
        match row.kind {
            TranscriptItemKind::Message if row.role == Some(Role::Assistant) => {
                known_assistant_message |= recorded_model(row.model.as_deref()).is_some();
            }
            TranscriptItemKind::ToolCall => {
                saw_call_or_delegation = true;
                known_call_or_delegation |=
                    recorded_model(row.tool_call_model.as_deref()).is_some();
            }
            TranscriptItemKind::Subagent => {
                saw_call_or_delegation = true;
                known_call_or_delegation |=
                    recorded_model(row.subagent_source_model.as_deref()).is_some();
            }
            _ => {}
        }
    }
    if !known_assistant_message && !known_call_or_delegation {
        return BTreeMap::new();
    }
    let pre_attribution =
        known_assistant_message && saw_call_or_delegation && !known_call_or_delegation;

    let mut markers = BTreeMap::new();
    // The outer option tracks whether a run has started; the inner option is
    // its model, which can be NULL.
    let mut run = None;
    for row in rows {
        let model = recorded_model(match row.kind {
            TranscriptItemKind::Message if row.role == Some(Role::Assistant) => {
                row.model.as_deref()
            }
            TranscriptItemKind::ToolCall if !pre_attribution => row.tool_call_model.as_deref(),
            TranscriptItemKind::Subagent if !pre_attribution => {
                row.subagent_source_model.as_deref()
            }
            _ => continue,
        });
        if run == Some(model) {
            continue;
        }
        if run.is_some() || model.is_some() {
            markers.insert(
                row.item_index,
                ModelRunMarker {
                    model: model.map(str::to_string),
                    previous: run.flatten().map(str::to_string),
                },
            );
        }
        run = Some(model);
    }
    markers
}

#[derive(Clone, Debug)]
pub enum ChildModels {
    NotIndexed,
    Indexed(SessionModels),
    Unavailable,
}

#[derive(Clone, Debug)]
pub struct HandoffCaption {
    pub markup: String,
    pub plain: String,
    pub tooltip: String,
}

pub fn collect_child_models<'a>(
    ids: impl Iterator<Item = &'a str>,
    mut load: impl FnMut(&str) -> ChildModels,
) -> BTreeMap<String, ChildModels> {
    let mut children = BTreeMap::new();
    for id in ids {
        children.entry(id.to_string()).or_insert_with(|| load(id));
    }
    children
}

pub fn handoff_caption(
    source: Option<&str>,
    child_id: Option<&str>,
    children: &BTreeMap<String, ChildModels>,
) -> HandoffCaption {
    let source = source.filter(|value| !value.trim().is_empty());
    let left = source.map_or_else(
        || "requested by: not recorded".to_string(),
        |s| format!("requested by {s}"),
    );
    let left_markup = source.map_or_else(
        || left.clone(),
        |s| format!("requested by <tt>{}</tt>", gtk::glib::markup_escape_text(s)),
    );
    let (right, right_markup, right_tooltip) = match child_id {
        None => ("no linked child session".to_string(), None, None),
        Some(id) => match children.get(id) {
            Some(ChildModels::NotIndexed) => ("child session not indexed".to_string(), None, None),
            Some(ChildModels::Indexed(models)) if models.models.is_empty() => {
                ("child model not recorded".to_string(), None, None)
            }
            Some(ChildModels::Indexed(models)) => {
                let first = &models.models[0].model;
                let suffix = if models.models.len() > 1 {
                    format!(" +{}", models.models.len() - 1)
                } else {
                    String::new()
                };
                (
                    format!("ran {first}{suffix}"),
                    Some(format!(
                        "ran <tt>{}</tt>{suffix}",
                        gtk::glib::markup_escape_text(first)
                    )),
                    Some(format!(
                        "ran {}",
                        models
                            .models
                            .iter()
                            .map(|m| m.model.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )),
                )
            }
            _ => ("child model unavailable".to_string(), None, None),
        },
    };
    HandoffCaption {
        markup: format!(
            "{left_markup} · {}",
            right_markup.unwrap_or_else(|| right.clone())
        ),
        plain: format!("{left} · {right}"),
        tooltip: format!("{left} · {}", right_tooltip.unwrap_or(right)),
    }
}
