use std::collections::BTreeMap;

use super::model_attribution::{ChildModels, handoff_caption};
use crate::database::{SessionModelUsage, SessionModels};

#[test]
fn model_handoff_child_lookup_is_deduplicated() {
    let mut child_reads = Vec::new();
    let children = super::model_attribution::collect_child_models(
        [Some("child"), Some("child"), None].into_iter().flatten(),
        |id| {
            child_reads.push(id.to_string());
            ChildModels::Indexed(models(&["B"]))
        },
    );
    assert_eq!(child_reads, ["child"]);
    assert_eq!(children.len(), 1);
}

fn models(names: &[&str]) -> SessionModels {
    SessionModels {
        models: names
            .iter()
            .enumerate()
            .map(|(index, name)| SessionModelUsage {
                model: (*name).to_string(),
                entry_count: 1,
                first_item_index: index as i64,
                first_used_at: None,
            })
            .collect(),
        unattributed_count: 0,
    }
}

#[test]
fn model_handoff_caption_cases_and_escaping() {
    let mut children = BTreeMap::new();
    children.insert(
        "child".to_string(),
        ChildModels::Indexed(models(&["B", "C"])),
    );
    children.insert("single".to_string(), ChildModels::Indexed(models(&["B"])));
    children.insert("empty".to_string(), ChildModels::Indexed(models(&[])));
    children.insert("missing".to_string(), ChildModels::NotIndexed);
    children.insert("error".to_string(), ChildModels::Unavailable);

    let first = handoff_caption(Some("A<&"), Some("child"), &children);
    assert_eq!(first.plain, "requested by A<& · ran B +1");
    assert!(first.markup.contains("<tt>A&lt;&amp;</tt>"));
    let second = handoff_caption(None, Some("child"), &children);
    assert_eq!(second.tooltip, "requested by: not recorded · ran B, C");
    let unlinked = handoff_caption(None, None, &children);
    assert_eq!(
        unlinked.plain,
        "requested by: not recorded · no linked child session"
    );
    let missing = handoff_caption(None, Some("missing"), &children);
    assert!(missing.plain.ends_with("child session not indexed"));
    let empty = handoff_caption(None, Some("empty"), &children);
    assert!(empty.plain.ends_with("child model not recorded"));
    let one = handoff_caption(None, Some("single"), &children);
    assert!(one.plain.ends_with("ran B"));
    let unavailable = handoff_caption(None, Some("error"), &children);
    assert!(unavailable.plain.ends_with("child model unavailable"));
    assert_eq!(
        handoff_caption(Some("  "), None, &children).plain,
        unlinked.plain
    );
}
