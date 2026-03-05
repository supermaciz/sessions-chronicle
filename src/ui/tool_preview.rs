use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

const MAX_PREVIEW_CHARS: usize = 60;

pub fn extract_preview(
    tool_name: &str,
    input_json: &str,
    output_text: Option<&str>,
) -> Option<String> {
    let normalized_tool = tool_name.to_ascii_lowercase();
    let parsed = serde_json::from_str::<Value>(input_json).ok();

    let preview = match normalized_tool.as_str() {
        "bash" | "shell" | "exec_command" => parsed.as_ref().and_then(bash_preview),
        "read" => parsed.as_ref().and_then(read_preview),
        "edit" => parsed.as_ref().and_then(edit_preview),
        "apply_patch" => parsed.as_ref().and_then(apply_patch_preview),
        "grep" | "search" => parsed
            .as_ref()
            .and_then(|value| grep_preview(value, output_text)),
        "agent" | "task" => parsed
            .as_ref()
            .and_then(agent_preview)
            .or_else(|| output_preview(output_text)),
        _ => None,
    }
    .or_else(|| parsed.as_ref().and_then(first_meaningful_string))
    .or_else(|| output_preview(output_text));

    preview.map(|text| truncate_preview(&text, MAX_PREVIEW_CHARS))
}

fn bash_preview(value: &Value) -> Option<String> {
    let command = string_field(value, &["command", "cmd"])?;
    let segment = first_command_segment(command)?;
    Some(format!("$ {segment}"))
}

fn read_preview(value: &Value) -> Option<String> {
    let path = string_field(value, &["file_path", "filePath", "path"])?;
    let offset = integer_field(value, &["offset", "start"]);
    let limit = integer_field(value, &["limit", "count"]);

    let compact = compact_path(path);
    match (offset, limit) {
        (Some(start), Some(count)) if count > 0 => {
            let end = start.saturating_add(count.saturating_sub(1));
            Some(format!("{compact}:{start}-{end}"))
        }
        (Some(start), _) => Some(format!("{compact}:{start}")),
        _ => Some(compact),
    }
}

fn edit_preview(value: &Value) -> Option<String> {
    let path = string_field(value, &["file_path", "filePath", "path"])?;
    let old_text = string_field(value, &["old_string", "oldString", "before"]).unwrap_or_default();
    let new_text = string_field(value, &["new_string", "newString", "after"]).unwrap_or_default();
    let (added, removed) = line_delta_counts(old_text, new_text);
    let compact = compact_path(path);

    Some(format!("{compact} +{added} -{removed}"))
}

fn grep_preview(value: &Value, output_text: Option<&str>) -> Option<String> {
    let pattern = string_field(value, &["pattern", "query", "regex"])?;
    let escaped = pattern.replace('"', "\\\"");
    match infer_match_count(output_text) {
        Some(count) => Some(format!("pattern=\"{escaped}\" -> {count} matches")),
        None => Some(format!("pattern=\"{escaped}\"")),
    }
}

fn apply_patch_preview(value: &Value) -> Option<String> {
    let patch_text = string_field(value, &["patchText", "patch_text", "patch"])?;
    let mut added = 0usize;
    let mut updated = 0usize;
    let mut deleted = 0usize;
    let mut first_operation: Option<(&'static str, String)> = None;

    for line in patch_text.lines().map(str::trim) {
        let (kind, path) = if let Some(path) = line.strip_prefix("*** Add File:") {
            ("add", path)
        } else if let Some(path) = line.strip_prefix("*** Update File:") {
            ("update", path)
        } else if let Some(path) = line.strip_prefix("*** Delete File:") {
            ("delete", path)
        } else {
            continue;
        };

        let compact = compact_path(path.trim());
        if compact.is_empty() {
            continue;
        }

        if first_operation.is_none() {
            first_operation = Some((kind, compact.clone()));
        }

        match kind {
            "add" => added += 1,
            "update" => updated += 1,
            "delete" => deleted += 1,
            _ => {}
        }
    }

    let total = added + updated + deleted;
    if total == 0 {
        return None;
    }
    if total == 1 {
        return first_operation.map(|(kind, path)| format!("{kind} {path}"));
    }

    Some(format!("{total} files (+{added} ~{updated} -{deleted})"))
}

fn agent_preview(value: &Value) -> Option<String> {
    string_field(
        value,
        &["description", "task", "prompt", "instructions", "summary"],
    )
    .map(ToString::to_string)
}

fn string_field<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    let object = value.as_object()?;
    keys.iter()
        .find_map(|key| object.get(*key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

fn integer_field(value: &Value, keys: &[&str]) -> Option<usize> {
    let object = value.as_object()?;
    keys.iter().find_map(|key| {
        object.get(*key).and_then(|candidate| match candidate {
            Value::Number(number) => number.as_u64().and_then(|n| usize::try_from(n).ok()),
            Value::String(text) => text.trim().parse::<usize>().ok(),
            _ => None,
        })
    })
}

fn first_command_segment(command: &str) -> Option<&str> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut end = trimmed.len();
    for separator in ['\n', ';'] {
        if let Some(index) = trimmed.find(separator) {
            end = end.min(index);
        }
    }
    if let Some(index) = trimmed.find("&&") {
        end = end.min(index);
    }
    if let Some(index) = trimmed.find("||") {
        end = end.min(index);
    }

    let segment = trimmed[..end].trim();
    if segment.is_empty() {
        None
    } else {
        Some(segment)
    }
}

fn line_delta_counts(old_text: &str, new_text: &str) -> (usize, usize) {
    let mut old_counts: HashMap<String, usize> = HashMap::new();
    let mut new_counts: HashMap<String, usize> = HashMap::new();

    for line in old_text.lines() {
        *old_counts.entry(line.to_string()).or_default() += 1;
    }
    for line in new_text.lines() {
        *new_counts.entry(line.to_string()).or_default() += 1;
    }

    let mut removed = 0usize;
    for (line, old_count) in &old_counts {
        let new_count = new_counts.get(line).copied().unwrap_or(0);
        removed += old_count.saturating_sub(new_count);
    }

    let mut added = 0usize;
    for (line, new_count) in &new_counts {
        let old_count = old_counts.get(line).copied().unwrap_or(0);
        added += new_count.saturating_sub(old_count);
    }

    (added, removed)
}

fn infer_match_count(output_text: Option<&str>) -> Option<usize> {
    let text = output_text?.trim();
    if text.is_empty() {
        return None;
    }

    if let Ok(value) = serde_json::from_str::<Value>(text) {
        if let Some(count) = value.as_u64().and_then(|n| usize::try_from(n).ok()) {
            return Some(count);
        }
        if let Some(matches) = value
            .as_object()
            .and_then(|obj| obj.get("matches"))
            .and_then(Value::as_array)
        {
            return Some(matches.len());
        }
        if let Some(count) = value
            .as_object()
            .and_then(|obj| obj.get("count"))
            .and_then(Value::as_u64)
            .and_then(|n| usize::try_from(n).ok())
        {
            return Some(count);
        }
    }

    let tokens: Vec<&str> = text.split_whitespace().collect();
    for (index, token) in tokens.iter().enumerate() {
        let normalized = token.trim_matches(|ch: char| !ch.is_ascii_alphabetic());
        if !normalized.eq_ignore_ascii_case("match") && !normalized.eq_ignore_ascii_case("matches")
        {
            continue;
        }

        let start = index.saturating_sub(2);
        let end = (index + 2).min(tokens.len().saturating_sub(1));
        for candidate in &tokens[start..=end] {
            if let Ok(number) = candidate
                .trim_matches(|ch: char| !ch.is_ascii_digit())
                .parse::<usize>()
            {
                return Some(number);
            }
        }
    }

    let lines = text.lines().filter(|line| !line.trim().is_empty()).count();
    if lines > 0 { Some(lines) } else { None }
}

fn first_meaningful_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Value::Array(items) => items.iter().find_map(first_meaningful_string),
        Value::Object(map) => map.values().find_map(first_meaningful_string),
        _ => None,
    }
}

fn output_preview(output_text: Option<&str>) -> Option<String> {
    let text = output_text?;
    let first_line = text.lines().find(|line| !line.trim().is_empty())?;
    let trimmed = first_line.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn compact_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if trimmed.chars().count() <= MAX_PREVIEW_CHARS {
        return trimmed.to_string();
    }

    let normalized = trimmed.replace('\\', "/");
    let candidate = Path::new(&normalized);
    match (candidate.parent(), candidate.file_name()) {
        (Some(parent), Some(file_name)) => {
            let parent_name = parent
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .unwrap_or("...");
            let file_name = file_name.to_str().unwrap_or("<file>");
            format!("{parent_name}/{file_name}")
        }
        _ => trimmed.to_string(),
    }
}

fn truncate_preview(value: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    let count = trimmed.chars().count();
    if count <= max_chars {
        return trimmed.to_string();
    }

    if max_chars == 0 {
        return String::new();
    }

    let kept = max_chars.saturating_sub(1);
    let mut truncated = String::with_capacity(max_chars);
    for ch in trimmed.chars().take(kept) {
        truncated.push(ch);
    }
    truncated.push('…');
    truncated
}

#[cfg(test)]
mod tests {
    use super::extract_preview;

    #[test]
    fn extract_preview_for_bash_uses_command() {
        let preview = extract_preview("bash", r#"{"command":"ls -la"}"#, None);
        assert_eq!(preview.as_deref(), Some("$ ls -la"));
    }

    #[test]
    fn extract_preview_for_read_uses_path_and_range() {
        let preview = extract_preview(
            "read",
            r#"{"file_path":"/tmp/log.txt","offset":5,"limit":10}"#,
            None,
        );
        assert_eq!(preview.as_deref(), Some("/tmp/log.txt:5-14"));
    }

    #[test]
    fn extract_preview_for_edit_uses_added_removed_counts() {
        let preview = extract_preview(
            "edit",
            r#"{"file_path":"src/main.rs","old_string":"a\nb","new_string":"a\nb\nc"}"#,
            None,
        );
        assert_eq!(preview.as_deref(), Some("src/main.rs +1 -0"));
    }

    #[test]
    fn extract_preview_for_apply_patch_uses_patch_headers() {
        let preview = extract_preview(
            "apply_patch",
            r#"{"patchText":"*** Begin Patch\n*** Update File: src/main.rs\n@@\n-old\n+new\n*** End Patch"}"#,
            None,
        );
        assert_eq!(preview.as_deref(), Some("update src/main.rs"));
    }

    #[test]
    fn extract_preview_for_apply_patch_summarizes_multiple_operations() {
        let preview = extract_preview(
            "apply_patch",
            r#"{"patchText":"*** Begin Patch\n*** Add File: src/new.rs\n+fn main() {}\n*** Update File: src/main.rs\n@@\n-old\n+new\n*** Delete File: src/old.rs\n*** End Patch"}"#,
            None,
        );
        assert_eq!(preview.as_deref(), Some("3 files (+1 ~1 -1)"));
    }

    #[test]
    fn extract_preview_for_grep_uses_numeric_count_with_match_signal() {
        let preview = extract_preview("grep", r#"{"pattern":"TODO"}"#, Some("Found 12 matches"));
        assert_eq!(preview.as_deref(), Some("pattern=\"TODO\" -> 12 matches"));
    }

    #[test]
    fn extract_preview_for_grep_ignores_unrelated_numbers() {
        let preview = extract_preview(
            "grep",
            r#"{"pattern":"TODO"}"#,
            Some("error code 12\nnext line"),
        );
        assert_eq!(preview.as_deref(), Some("pattern=\"TODO\" -> 2 matches"));
    }

    #[test]
    fn extract_preview_truncates_to_sixty_chars() {
        let preview = extract_preview(
            "agent",
            r#"{"description":"abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijk"}"#,
            None,
        );
        assert_eq!(
            preview.as_deref(),
            Some("abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefg…")
        );
    }

    #[test]
    fn extract_preview_falls_back_to_first_meaningful_string() {
        let preview = extract_preview("unknown", r#"{"meta":{"title":"  hello preview  "}}"#, None);
        assert_eq!(preview.as_deref(), Some("hello preview"));
    }

    #[test]
    fn extract_preview_falls_back_to_first_output_line() {
        let preview = extract_preview(
            "unknown",
            "{not-json}",
            Some("\n  first line  \nsecond line"),
        );
        assert_eq!(preview.as_deref(), Some("first line"));
    }

    #[test]
    fn extract_preview_handles_malformed_json() {
        let preview = extract_preview("bash", "{not-json}", None);
        assert_eq!(preview, None);
    }
}
