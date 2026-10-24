use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SwarmBlock {
    pub agent_id: String,
    pub item: Option<String>,
    pub outcome: Option<String>,
    pub body: String,
}

pub(super) struct SwarmChild<'a> {
    pub agent_id: &'a str,
    pub swarm_item: Option<&'a str>,
    pub first_wire_time_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SwarmMember {
    pub agent_id: String,
    pub item: Option<String>,
    pub summary: Option<String>,
}

pub(super) struct SwarmCall<'a> {
    pub items: Option<&'a [String]>,
    pub call_time_ms: Option<i64>,
    pub result_output: Option<&'a str>,
    pub result_is_error: bool,
}

pub(super) fn parse_swarm_result(output: &str) -> Option<Vec<SwarmBlock>> {
    const RESULT_START: &str = "<agent_swarm_result>";
    const RESULT_END: &str = "</agent_swarm_result>";
    const SUBAGENT_END: &str = "</subagent>";

    let result_start = output.find(RESULT_START)? + RESULT_START.len();
    let result_end = output[result_start..].find(RESULT_END)? + result_start;
    let contents = &output[result_start..result_end];

    let mut starts = Vec::new();
    let mut offset = 0;
    for line in contents.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("<subagent") {
            let tag_name_end = "<subagent".len();
            if trimmed
                .as_bytes()
                .get(tag_name_end)
                .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b'>')
                && let Some(tag_end) = opening_tag_end(trimmed)
            {
                let leading = line.len() - trimmed.len();
                let start = offset + leading;
                starts.push((start, start + tag_end + 1));
            }
        }
        offset += line.len();
    }

    let mut blocks = Vec::with_capacity(starts.len());
    for (index, &(tag_start, body_start)) in starts.iter().enumerate() {
        let tag = &contents[tag_start..body_start];
        let attributes = parse_tag_attributes(tag)?;
        let agent_id = attributes.get("agent_id")?.to_string();
        let item = attributes.get("item").map(|value| (*value).to_string());
        let outcome = attributes.get("outcome").map(|value| (*value).to_string());

        let segment_end = starts
            .get(index + 1)
            .map_or(contents.len(), |(next_start, _)| *next_start);
        let body_segment = &contents[body_start..segment_end];
        let body_end = body_segment.rfind(SUBAGENT_END)?;
        let body = body_segment[..body_end].trim().to_string();

        blocks.push(SwarmBlock {
            agent_id,
            item,
            outcome,
            body,
        });
    }

    Some(blocks)
}

fn opening_tag_end(tag: &str) -> Option<usize> {
    let mut in_quotes = false;
    for (index, byte) in tag.bytes().enumerate() {
        match byte {
            b'"' => in_quotes = !in_quotes,
            b'>' if !in_quotes => return Some(index),
            _ => {}
        }
    }
    None
}

fn parse_tag_attributes(tag: &str) -> Option<HashMap<&str, &str>> {
    let mut attributes = HashMap::new();
    let bytes = tag.as_bytes();
    let mut position = "<subagent".len();

    while position < bytes.len() {
        while bytes.get(position).is_some_and(u8::is_ascii_whitespace) {
            position += 1;
        }
        if position >= bytes.len() || bytes[position] == b'>' {
            break;
        }

        let key_start = position;
        while bytes
            .get(position)
            .is_some_and(|byte| !byte.is_ascii_whitespace() && *byte != b'=' && *byte != b'>')
        {
            position += 1;
        }
        if key_start == position {
            return None;
        }
        let key = tag.get(key_start..position)?;

        while bytes.get(position).is_some_and(u8::is_ascii_whitespace) {
            position += 1;
        }
        if bytes.get(position) != Some(&b'=') {
            return None;
        }
        position += 1;
        while bytes.get(position).is_some_and(u8::is_ascii_whitespace) {
            position += 1;
        }
        if bytes.get(position) != Some(&b'"') {
            return None;
        }
        position += 1;
        let value_start = position;
        while bytes.get(position).is_some_and(|byte| *byte != b'"') {
            position += 1;
        }
        if position == bytes.len() {
            return None;
        }
        let value = tag.get(value_start..position)?;
        attributes.insert(key, value);
        position += 1;
    }

    Some(attributes)
}

pub(super) fn resolve_swarm(
    call: &SwarmCall<'_>,
    children: &[SwarmChild<'_>],
    taken: &HashSet<String>,
) -> Option<Vec<SwarmMember>> {
    if let Some(output) = call.result_output
        && let Some(blocks) = parse_swarm_result(output)
        && let Some(members) = resolve_result_blocks(call, children, taken, &blocks)
    {
        return Some(members);
    }

    resolve_swarm_fallback(call, children, taken)
}

fn resolve_result_blocks(
    call: &SwarmCall<'_>,
    children: &[SwarmChild<'_>],
    taken: &HashSet<String>,
    blocks: &[SwarmBlock],
) -> Option<Vec<SwarmMember>> {
    let mut seen_agents = HashSet::new();
    let mut members = Vec::with_capacity(blocks.len());

    for block in blocks {
        if !seen_agents.insert(block.agent_id.as_str()) || taken.contains(&block.agent_id) {
            return None;
        }
        let mut matching_children = children
            .iter()
            .filter(|child| child.agent_id == block.agent_id);
        let child = matching_children.next()?;
        if matching_children.next().is_some()
            || child
                .swarm_item
                .is_some_and(|swarm_item| Some(swarm_item) != block.item.as_deref())
        {
            return None;
        }

        members.push(SwarmMember {
            agent_id: block.agent_id.clone(),
            item: block.item.clone(),
            summary: nonempty_body(&block.body).or_else(|| error_summary(call)),
        });
    }

    if let Some(items) = call.items {
        let mut seen_items = HashSet::new();
        let mut by_item = HashMap::with_capacity(members.len());
        for member in members {
            let item = member.item.clone()?;
            if !seen_items.insert(item.clone()) {
                return None;
            }
            by_item.insert(item, member);
        }
        if items.len() != by_item.len() || items.iter().any(|item| !by_item.contains_key(item)) {
            return None;
        }
        items.iter().map(|item| by_item.remove(item)).collect()
    } else {
        Some(members)
    }
}

fn resolve_swarm_fallback(
    call: &SwarmCall<'_>,
    children: &[SwarmChild<'_>],
    taken: &HashSet<String>,
) -> Option<Vec<SwarmMember>> {
    let items = call.items.filter(|items| !items.is_empty())?;
    let call_time = call.call_time_ms?;
    let mut seen_items = HashSet::new();
    let mut seen_agents = HashSet::new();
    let summary = error_summary(call);
    let mut members = Vec::with_capacity(items.len());

    for item in items {
        if !seen_items.insert(item.as_str()) {
            return None;
        }
        let mut matches = children.iter().filter(|child| {
            child.swarm_item == Some(item.as_str())
                && !taken.contains(child.agent_id)
                && child
                    .first_wire_time_ms
                    .is_some_and(|first_wire_time_ms| first_wire_time_ms >= call_time)
        });
        let child = matches.next()?;
        if matches.next().is_some() || !seen_agents.insert(child.agent_id) {
            return None;
        }
        members.push(SwarmMember {
            agent_id: child.agent_id.to_string(),
            item: Some(item.clone()),
            summary: summary.clone(),
        });
    }

    Some(members)
}

fn nonempty_body(body: &str) -> Option<String> {
    (!body.is_empty()).then(|| body.to_string())
}

fn error_summary(call: &SwarmCall<'_>) -> Option<String> {
    if call.result_is_error {
        call.result_output
            .map(str::trim)
            .filter(|output| !output.is_empty())
            .map(str::to_string)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn parses_blocks_in_order() {
        let output = "<agent_swarm_result>\n\
            <summary>completed: 2</summary>\n\
            <subagent agent_id=\"agent-0\" item=\"A\" outcome=\"completed\">\n\
            ## First\n\
            Body one\n\
            </subagent>\n\
            <subagent agent_id=\"agent-1\" item=\"B\" outcome=\"completed\">\n\
            ## Second\n\
            Body two\n\
            </subagent>\n\
            </agent_swarm_result>";

        assert_eq!(
            parse_swarm_result(output),
            Some(vec![
                SwarmBlock {
                    agent_id: "agent-0".into(),
                    item: Some("A".into()),
                    outcome: Some("completed".into()),
                    body: "## First\nBody one".into(),
                },
                SwarmBlock {
                    agent_id: "agent-1".into(),
                    item: Some("B".into()),
                    outcome: Some("completed".into()),
                    body: "## Second\nBody two".into(),
                },
            ])
        );
    }

    #[test]
    fn body_with_quoted_tags_is_kept_whole() {
        let output = "<agent_swarm_result>\n\
            <subagent agent_id=\"agent-0\" item=\"A\">\n\
            A quoted line: 204\t   <subagent agent_id=\"quoted\">\n\
            This sentence contains `</subagent>` inline.\n\
            </subagent>\n\
            <subagent agent_id=\"agent-1\" item=\"B\">second</subagent>\n\
            </agent_swarm_result>";

        let blocks = parse_swarm_result(output).unwrap();
        assert_eq!(blocks.len(), 2);
        assert!(
            blocks[0]
                .body
                .contains("204\t   <subagent agent_id=\"quoted\">")
        );
        assert!(blocks[0].body.contains("`</subagent>`"));
        assert_eq!(blocks[0].body.matches("</subagent>").count(), 1);
        assert_eq!(blocks[1].agent_id, "agent-1");
    }

    #[test]
    fn opening_tag_split_across_lines_is_not_a_block() {
        let output = "<agent_swarm_result>\n\
            <subagent agent_id=\"x\"\n>\nbody\n</subagent>\n\
            </agent_swarm_result>";

        assert_eq!(parse_swarm_result(output), Some(Vec::new()));
    }

    #[test]
    fn missing_closing_tag_is_unusable() {
        assert_eq!(
            parse_swarm_result(
                "<agent_swarm_result>\n<subagent agent_id=\"x\">body\n</agent_swarm_result>"
            ),
            None
        );
    }

    #[test]
    fn missing_agent_id_is_unusable() {
        assert_eq!(
            parse_swarm_result(
                "<agent_swarm_result>\n<subagent item=\"A\">body</subagent>\n</agent_swarm_result>"
            ),
            None
        );
    }

    #[test]
    fn missing_result_end_is_unusable() {
        assert_eq!(
            parse_swarm_result("<agent_swarm_result>\n<subagent agent_id=\"x\">body</subagent>"),
            None
        );
    }

    #[test]
    fn no_entity_decoding() {
        let output = "<agent_swarm_result>\n\
            <subagent agent_id=\"agent-0\" item=\"a > &amp; b\">body</subagent>\n\
            </agent_swarm_result>";

        assert_eq!(
            parse_swarm_result(output).unwrap()[0].item.as_deref(),
            Some("a > &amp; b")
        );
    }

    fn child(
        agent_id: &'static str,
        swarm_item: Option<&'static str>,
        time: Option<i64>,
    ) -> SwarmChild<'static> {
        SwarmChild {
            agent_id,
            swarm_item,
            first_wire_time_ms: time,
        }
    }

    fn call<'a>(
        items: Option<&'a [String]>,
        output: Option<&'a str>,
        is_error: bool,
        time: Option<i64>,
    ) -> SwarmCall<'a> {
        SwarmCall {
            items,
            call_time_ms: time,
            result_output: output,
            result_is_error: is_error,
        }
    }

    #[test]
    fn result_path_orders_by_items() {
        let items = ["B".to_string(), "A".to_string()];
        let output = "<agent_swarm_result>\n\
            <subagent agent_id=\"agent-0\" item=\"A\">summary A</subagent>\n\
            <subagent agent_id=\"agent-1\" item=\"B\">summary B</subagent>\n\
            </agent_swarm_result>";
        let children = [
            child("agent-0", Some("A"), Some(11)),
            child("agent-1", Some("B"), Some(12)),
        ];

        assert_eq!(
            resolve_swarm(
                &call(Some(&items), Some(output), false, Some(10)),
                &children,
                &HashSet::new()
            ),
            Some(vec![
                SwarmMember {
                    agent_id: "agent-1".into(),
                    item: Some("B".into()),
                    summary: Some("summary B".into()),
                },
                SwarmMember {
                    agent_id: "agent-0".into(),
                    item: Some("A".into()),
                    summary: Some("summary A".into()),
                },
            ])
        );
    }

    #[test]
    fn result_missing_an_item_falls_back() {
        let items = ["A".to_string(), "B".to_string()];
        let output = "<agent_swarm_result>\n<subagent agent_id=\"agent-0\" item=\"A\">summary</subagent>\n</agent_swarm_result>";
        let children = [
            child("agent-0", Some("A"), Some(11)),
            child("agent-1", Some("B"), Some(12)),
        ];

        let members = resolve_swarm(
            &call(Some(&items), Some(output), false, Some(10)),
            &children,
            &HashSet::new(),
        )
        .unwrap();
        assert_eq!(
            members
                .iter()
                .map(|member| member.agent_id.as_str())
                .collect::<Vec<_>>(),
            ["agent-0", "agent-1"]
        );
        assert!(members.iter().all(|member| member.summary.is_none()));
    }

    #[test]
    fn result_swarm_item_mismatch_falls_back() {
        let items = ["A".to_string()];
        let output = "<agent_swarm_result>\n<subagent agent_id=\"agent-0\" item=\"wrong\">summary</subagent>\n</agent_swarm_result>";
        let children = [child("agent-0", Some("A"), Some(11))];

        let members = resolve_swarm(
            &call(Some(&items), Some(output), false, Some(10)),
            &children,
            &HashSet::new(),
        )
        .unwrap();
        assert_eq!(members[0].item.as_deref(), Some("A"));
        assert_eq!(members[0].summary, None);
    }

    #[test]
    fn fallback_uses_error_output_as_summary() {
        let items = ["A".to_string(), "B".to_string()];
        let children = [
            child("agent-0", Some("A"), Some(11)),
            child("agent-1", Some("B"), Some(12)),
        ];

        let members = resolve_swarm(
            &call(Some(&items), Some("  Session closed  "), true, Some(10)),
            &children,
            &HashSet::new(),
        )
        .unwrap();
        assert!(
            members
                .iter()
                .all(|member| member.summary.as_deref() == Some("Session closed"))
        );
    }

    #[test]
    fn fallback_rejects_duplicate_items() {
        let items = ["A".to_string(), "A".to_string()];
        let children = [child("agent-0", Some("A"), Some(11))];

        assert_eq!(
            resolve_swarm(
                &call(Some(&items), None, false, Some(10)),
                &children,
                &HashSet::new()
            ),
            None
        );
    }

    #[test]
    fn fallback_rejects_child_started_before_call() {
        let items = ["A".to_string()];
        let children = [child("agent-0", Some("A"), Some(9))];

        assert_eq!(
            resolve_swarm(
                &call(Some(&items), None, false, Some(10)),
                &children,
                &HashSet::new()
            ),
            None
        );
    }

    #[test]
    fn fallback_requires_both_times() {
        let items = ["A".to_string()];
        let children = [child("agent-0", Some("A"), Some(11))];

        assert_eq!(
            resolve_swarm(
                &call(Some(&items), None, false, None),
                &children,
                &HashSet::new()
            ),
            None
        );

        let children = [child("agent-0", Some("A"), None)];
        assert_eq!(
            resolve_swarm(
                &call(Some(&items), None, false, Some(10)),
                &children,
                &HashSet::new()
            ),
            None
        );
    }

    #[test]
    fn fallback_rejects_two_children_for_one_item() {
        let items = ["A".to_string()];
        let children = [
            child("agent-0", Some("A"), Some(11)),
            child("agent-1", Some("A"), Some(12)),
        ];

        assert_eq!(
            resolve_swarm(
                &call(Some(&items), None, false, Some(10)),
                &children,
                &HashSet::new()
            ),
            None
        );
    }

    #[test]
    fn fallback_rejects_taken_child() {
        let items = ["A".to_string()];
        let children = [child("agent-0", Some("A"), Some(11))];
        let taken = HashSet::from(["agent-0".to_string()]);

        assert_eq!(
            resolve_swarm(
                &call(Some(&items), None, false, Some(10)),
                &children,
                &taken
            ),
            None
        );
    }

    #[test]
    fn result_path_rejects_unknown_or_taken_agent() {
        let items = ["A".to_string()];
        let output = "<agent_swarm_result>\n<subagent agent_id=\"unknown\" item=\"A\">summary</subagent>\n</agent_swarm_result>";
        let children = [child("agent-0", Some("A"), Some(11))];

        let fallback = resolve_swarm(
            &call(Some(&items), Some(output), false, Some(10)),
            &children,
            &HashSet::new(),
        )
        .unwrap();
        assert_eq!(fallback[0].agent_id, "agent-0");
        assert_eq!(fallback[0].summary, None);

        let no_item_match = [child("agent-0", Some("other"), Some(11))];
        assert_eq!(
            resolve_swarm(
                &call(Some(&items), Some(output), false, Some(10)),
                &no_item_match,
                &HashSet::new()
            ),
            None
        );

        let output = "<agent_swarm_result>\n<subagent agent_id=\"agent-0\" item=\"A\">summary</subagent>\n</agent_swarm_result>";
        let taken = HashSet::from(["agent-0".to_string()]);
        assert_eq!(
            resolve_swarm(
                &call(Some(&items), Some(output), false, Some(10)),
                &children,
                &taken
            ),
            None
        );
    }

    #[test]
    fn no_items_uses_block_order() {
        let output = "<agent_swarm_result>\n\
            <subagent agent_id=\"agent-1\" item=\"B\">second</subagent>\n\
            <subagent agent_id=\"agent-0\" item=\"A\">first</subagent>\n\
            </agent_swarm_result>";
        let children = [
            child("agent-0", Some("A"), None),
            child("agent-1", Some("B"), None),
        ];
        let members = resolve_swarm(
            &call(None, Some(output), false, None),
            &children,
            &HashSet::new(),
        )
        .unwrap();
        assert_eq!(
            members
                .iter()
                .map(|member| member.agent_id.as_str())
                .collect::<Vec<_>>(),
            ["agent-1", "agent-0"]
        );
        assert_eq!(
            resolve_swarm(&call(None, None, false, None), &children, &HashSet::new()),
            None
        );
    }
}
