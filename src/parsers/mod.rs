pub mod claude_code;
pub mod codex;
pub mod mistral_vibe;
pub mod model;
pub mod opencode;

use crate::models::{Message, Role, Subagent, TokenUsage, ToolCall, TranscriptItem};
use regex::Regex;
use std::sync::LazyLock;

const FIRST_PROMPT_MAX_CHARS: usize = 200;

/// All data produced by parsing a single session file.
#[derive(Debug)]
pub struct ParsedSession {
    pub session: crate::models::Session,
    pub messages: Vec<Message>,
    pub tool_calls: Vec<ToolCall>,
    pub subagents: Vec<Subagent>,
    pub transcript_items: Vec<TranscriptItem>,
    pub token_usage: Option<TokenUsage>,
}

static RE_COMMAND_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<command-name>(.*?)</command-name>").unwrap());
static RE_COMMAND_MESSAGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<command-message>(.*?)</command-message>").unwrap());
static RE_COMMAND_ARGS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<command-args>(.*?)</command-args>").unwrap());
static RE_COMMAND_FRAGMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"</?command-(name|message|args)>").unwrap());

fn strip_command_tags(content: &str) -> String {
    // Fast path: no command tags present.
    if !content.contains("<command-name>") {
        return content.to_string();
    }

    // Require exactly one <command-name> block.
    let name_matches: Vec<_> = RE_COMMAND_NAME.find_iter(content).collect();
    if name_matches.len() != 1 {
        return content.to_string();
    }

    // Extract the command name (e.g. "/brainstorming").
    let name_cap = RE_COMMAND_NAME.captures(content).unwrap();
    let command = name_cap[1].to_string();

    // Extract optional args (trimmed, may be empty).
    let args = RE_COMMAND_ARGS
        .captures(content)
        .map(|cap| cap[1].trim().to_string())
        .filter(|a| !a.is_empty());

    // Remove all fully matched tag blocks to find residual text.
    let residual = RE_COMMAND_NAME.replace_all(content, "");
    let residual = RE_COMMAND_MESSAGE.replace_all(&residual, "");
    let residual = RE_COMMAND_ARGS.replace_all(&residual, "");

    // If unmatched tag fragments remain, the input is malformed — return unchanged.
    if RE_COMMAND_FRAGMENT.is_match(&residual) {
        return content.to_string();
    }

    // Collapse whitespace in residual and treat non-empty result as trailing text.
    let trailing: String = residual.split_whitespace().collect::<Vec<_>>().join(" ");

    // Rebuild the clean title.
    let mut title = command;
    if let Some(a) = args {
        title.push(' ');
        title.push_str(&a);
    }
    if !trailing.is_empty() {
        title.push_str(" \u{2014} ");
        title.push_str(&trailing);
    }
    title
}

pub(crate) fn extract_first_prompt(messages: &[Message]) -> Option<String> {
    messages
        .iter()
        .filter(|message| message.role == Role::User)
        .filter(|message| !is_system_injected(&message.content))
        .map(|message| normalize_prompt(&message.content))
        .find(|prompt| !prompt.is_empty())
}

/// Returns `true` when the message content is system-injected noise rather
/// than something the user actually typed.
fn is_system_injected(content: &str) -> bool {
    let trimmed = content.trim();
    trimmed.starts_with("<local-command-")
        || trimmed.starts_with("<system-reminder>")
        || trimmed.starts_with("<command-output>")
}

fn normalize_prompt(content: &str) -> String {
    let cleaned = strip_command_tags(content);
    let normalized = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_chars(&normalized, FIRST_PROMPT_MAX_CHARS)
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }

    let mut truncated: String = value.chars().take(max_chars).collect();
    truncated.push('\u{2026}');
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn message(index: usize, role: Role, content: &str) -> Message {
        Message {
            session_id: "session-1".to_string(),
            index,
            role,
            content: content.to_string(),
            timestamp: Utc::now(),
            model: None,
        }
    }

    #[test]
    fn extract_first_prompt_skips_whitespace_only_user_message() {
        let messages = vec![
            message(0, Role::User, "   \n\t   "),
            message(1, Role::User, "  second   prompt  "),
        ];

        assert_eq!(
            extract_first_prompt(&messages),
            Some("second prompt".to_string())
        );
    }

    #[test]
    fn normalize_prompt_collapses_whitespace() {
        let messages = vec![message(
            0,
            Role::User,
            "  hello\n\n   world\tfrom   parser  ",
        )];

        assert_eq!(
            extract_first_prompt(&messages),
            Some("hello world from parser".to_string())
        );
    }

    #[test]
    fn normalize_prompt_truncates_at_200_and_201_char_boundaries() {
        let exactly_200 = "a".repeat(200);
        let exactly_201 = "b".repeat(201);

        assert_eq!(normalize_prompt(&exactly_200), exactly_200);

        let mut expected = "b".repeat(200);
        expected.push('\u{2026}');
        assert_eq!(normalize_prompt(&exactly_201), expected);
    }

    #[test]
    fn normalize_prompt_truncates_multibyte_chars_safely() {
        let multibyte = "é".repeat(201);

        let truncated = normalize_prompt(&multibyte);
        let mut expected = "é".repeat(200);
        expected.push('\u{2026}');
        assert_eq!(truncated, expected);
        assert_eq!(truncated.chars().count(), 201); // 200 chars + ellipsis
    }

    #[test]
    fn extract_first_prompt_skips_system_injected_messages() {
        let messages = vec![
            message(
                0,
                Role::User,
                "<local-command-caveat>Caveat: The messages below were generated by the user while running local commands.</local-command-caveat>",
            ),
            message(
                1,
                Role::User,
                "<local-command-stdout>Set model to opus</local-command-stdout>",
            ),
            message(
                2,
                Role::User,
                "<system-reminder>Some system context here.</system-reminder>",
            ),
            message(3, Role::User, "Review last commit"),
        ];

        assert_eq!(
            extract_first_prompt(&messages),
            Some("Review last commit".to_string())
        );
    }

    #[test]
    fn strip_command_tags_command_only() {
        let input = "<command-message>brainstorming</command-message>\
                      <command-name>/brainstorming</command-name>";
        assert_eq!(strip_command_tags(input), "/brainstorming");
    }

    #[test]
    fn strip_command_tags_command_with_args() {
        let input = "<command-name>/learn-rust</command-name>\
                      <command-message>learn-rust</command-message>\
                      <command-args>PATH B</command-args>";
        assert_eq!(strip_command_tags(input), "/learn-rust PATH B");
    }

    #[test]
    fn strip_command_tags_command_with_empty_args() {
        let input = "<command-name>/model</command-name>\
                      <command-message>model</command-message>\
                      <command-args></command-args>";
        assert_eq!(strip_command_tags(input), "/model");
    }

    #[test]
    fn strip_command_tags_command_with_trailing_text() {
        let input = "<command-message>review</command-message>\
                      <command-name>/review</command-name> fix the auth bug";
        assert_eq!(
            strip_command_tags(input),
            "/review \u{2014} fix the auth bug"
        );
    }

    #[test]
    fn strip_command_tags_whitespace_variation() {
        let input = "  <command-name>/review</command-name>  \n\
                      <command-message>review</command-message>  \n\
                      <command-args>  #36  </command-args>  ";
        assert_eq!(strip_command_tags(input), "/review #36");
    }

    #[test]
    fn strip_command_tags_no_tags_passthrough() {
        let input = "just a normal user message";
        assert_eq!(strip_command_tags(input), "just a normal user message");
    }

    #[test]
    fn strip_command_tags_partial_command_name_tag() {
        let input = "<command-name>/review";
        assert_eq!(strip_command_tags(input), "<command-name>/review");
    }

    #[test]
    fn strip_command_tags_partial_command_args_tag() {
        let input = "<command-name>/review</command-name>\
                      <command-args>some args";
        assert_eq!(
            strip_command_tags(input),
            "<command-name>/review</command-name><command-args>some args"
        );
    }

    #[test]
    fn strip_command_tags_multiple_command_blocks_unchanged() {
        let input = "<command-name>/review</command-name>\
                      <command-name>/model</command-name>";
        assert_eq!(
            strip_command_tags(input),
            "<command-name>/review</command-name><command-name>/model</command-name>"
        );
    }

    #[test]
    fn strip_command_tags_multiline_args() {
        let input = "<command-message>superpowers-extended-cc:brainstorming</command-message>\n\
                      <command-name>/superpowers-extended-cc:brainstorming</command-name>\n\
                      <command-args>\n\nJe veux retravailler sur le plan</command-args>";
        assert_eq!(
            strip_command_tags(input),
            "/superpowers-extended-cc:brainstorming Je veux retravailler sur le plan"
        );
    }

    #[test]
    fn strip_command_tags_qualified_command_name() {
        let input = "<command-name>/superpowers-extended-cc:brainstorming</command-name>";
        assert_eq!(
            strip_command_tags(input),
            "/superpowers-extended-cc:brainstorming"
        );
    }
}
