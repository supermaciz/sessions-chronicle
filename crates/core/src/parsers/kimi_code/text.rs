pub(super) fn strip_git_context(text: &str) -> &str {
    let trimmed = text.trim_start();
    let Some(after_opening_tag) = trimmed.strip_prefix("<git-context>") else {
        return text;
    };
    let Some(closing_tag) = after_opening_tag.find("</git-context>") else {
        return text;
    };

    let remainder = after_opening_tag[closing_tag + "</git-context>".len()..].trim_start();
    if remainder.is_empty() {
        text
    } else {
        remainder
    }
}

pub(super) struct AgentResultText<'a> {
    pub actual_subagent_type: Option<&'a str>,
    pub summary: &'a str,
}

pub(super) fn split_agent_result(text: &str) -> AgentResultText<'_> {
    if !text.starts_with("agent_id:") {
        return AgentResultText {
            actual_subagent_type: None,
            summary: text,
        };
    }

    let mut header_end = text.len();
    let mut remainder_start = text.len();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let line_content = line.strip_suffix('\n').unwrap_or(line);
        if line_content.trim().is_empty() {
            header_end = offset;
            remainder_start = offset + line.len();
            break;
        }
        offset += line.len();
    }

    let header = &text[..header_end];
    let remainder = &text[remainder_start..];
    let actual_subagent_type = header.lines().find_map(|line| {
        line.trim()
            .strip_prefix("actual_subagent_type:")
            .map(str::trim)
    });
    let summary = if remainder
        .lines()
        .next()
        .is_some_and(|line| line.trim() == "[summary]")
    {
        remainder
            .find('\n')
            .map_or("", |newline| &remainder[newline + 1..])
    } else {
        remainder
    };

    AgentResultText {
        actual_subagent_type,
        summary: summary.trim(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_leading_git_context_block() {
        let text = "  <git-context>\nWorking directory: /p\nBranch: main\n</git-context>\n\nThoroughness: thorough. Do X";
        assert_eq!(strip_git_context(text), "Thoroughness: thorough. Do X");
    }

    #[test]
    fn keeps_text_when_git_context_is_unclosed() {
        let text = "<git-context>\nWorking directory: /p\nDo X";
        assert_eq!(strip_git_context(text), text);
    }

    #[test]
    fn keeps_text_when_nothing_follows_git_context() {
        let text = "<git-context>\nBranch: main\n</git-context>\n  ";
        assert_eq!(strip_git_context(text), text);
    }

    #[test]
    fn ignores_git_context_not_at_start() {
        let text = "Do X\n<git-context>a</git-context>";
        assert_eq!(strip_git_context(text), text);
    }

    #[test]
    fn splits_completed_agent_result() {
        let r = split_agent_result(
            "agent_id: agent-0\nactual_subagent_type: explore\nstatus: completed\n\n[summary]\nAll checks complete.\n",
        );
        assert_eq!(r.actual_subagent_type, Some("explore"));
        assert_eq!(r.summary, "All checks complete.");
    }

    #[test]
    fn keeps_failed_agent_error_body() {
        let r = split_agent_result(
            "agent_id: agent-1\nactual_subagent_type: explore\nstatus: failed\n\nsubagent error: The subagent was stopped before it finished by user.",
        );
        assert_eq!(
            r.summary,
            "subagent error: The subagent was stopped before it finished by user."
        );
    }

    #[test]
    fn text_without_header_is_unchanged() {
        let r = split_agent_result("done");
        assert_eq!(r.actual_subagent_type, None);
        assert_eq!(r.summary, "done");
    }

    #[test]
    fn header_only_result_has_empty_summary() {
        assert_eq!(
            split_agent_result("agent_id: agent-0\nstatus: completed").summary,
            ""
        );
    }
}
