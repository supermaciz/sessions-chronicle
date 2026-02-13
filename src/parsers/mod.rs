pub mod claude_code;
pub mod codex;
pub mod mistral_vibe;
pub mod opencode;

use crate::models::{Message, Role};

const FIRST_PROMPT_MAX_CHARS: usize = 200;

pub(crate) fn extract_first_prompt(messages: &[Message]) -> Option<String> {
    messages
        .iter()
        .filter(|message| message.role == Role::User)
        .map(|message| normalize_prompt(&message.content))
        .find(|prompt| !prompt.is_empty())
}

fn normalize_prompt(content: &str) -> String {
    let normalized = content.split_whitespace().collect::<Vec<_>>().join(" ");
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
}
