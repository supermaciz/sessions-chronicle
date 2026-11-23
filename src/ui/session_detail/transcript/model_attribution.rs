use std::collections::BTreeMap;

use crate::database::SessionModels;
use relm4::gtk;

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
