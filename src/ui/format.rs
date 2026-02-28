use crate::models::TokenUsage;
use crate::models::ToolCallStatus;

/// Format a millisecond duration as a human-readable string.
///
/// - `< 1 s`  → `"Nms"`
/// - `< 1 min` → `"N.Ns"`
/// - `≥ 1 min` → `"Nm Ns"`
pub fn format_duration_ms(ms: i64) -> String {
    if ms < 1_000 {
        format!("{}ms", ms)
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1_000.0)
    } else {
        let secs = ms / 1_000;
        format!("{}m {}s", secs / 60, secs % 60)
    }
}

/// Icon name for a tool-call status, suitable for `gtk::Image::from_icon_name`.
pub fn status_icon_name(status: ToolCallStatus) -> &'static str {
    match status {
        ToolCallStatus::Completed => "emblem-ok-symbolic",
        ToolCallStatus::Error => "dialog-error-symbolic",
        ToolCallStatus::Running => "emblem-synchronizing-symbolic",
        ToolCallStatus::Pending => "content-loading-symbolic",
        ToolCallStatus::Unknown => "dialog-question-symbolic",
    }
}

/// Short human-readable label for a tool-call status.
pub fn tool_status_label(status: ToolCallStatus) -> &'static str {
    match status {
        ToolCallStatus::Pending => "pending",
        ToolCallStatus::Running => "running",
        ToolCallStatus::Completed => "done",
        ToolCallStatus::Error => "error",
        ToolCallStatus::Unknown => "unknown",
    }
}

/// CSS class for a tool-call status badge.
pub fn tool_status_css_class(status: ToolCallStatus) -> &'static str {
    match status {
        ToolCallStatus::Completed => "status-completed",
        ToolCallStatus::Error => "status-error",
        ToolCallStatus::Running => "status-running",
        ToolCallStatus::Pending => "status-pending",
        ToolCallStatus::Unknown => "status-unknown",
    }
}

/// Format an integer with thin-space (U+2009) thousands grouping: 12 345 678
pub fn format_token_count(n: i64) -> String {
    let negative = n < 0;
    let abs = if negative { (-n) as u64 } else { n as u64 };
    let digits = abs.to_string();
    let mut result = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            result.push('\u{2009}');
        }
        result.push(ch);
    }
    if negative {
        format!("-{}", result)
    } else {
        result
    }
}

/// Build the total label text from TokenUsage
pub fn format_token_total(usage: &TokenUsage) -> String {
    format_token_count(usage.display_total_tokens())
}

/// Build the tooltip breakdown text from TokenUsage
pub fn format_token_tooltip(usage: &TokenUsage) -> String {
    let mut parts = vec![
        format!("{} input", format_token_count(usage.input_tokens)),
        format!("{} output", format_token_count(usage.output_tokens)),
    ];
    if let Some(reasoning) = usage.reasoning_tokens {
        parts.push(format!("{} reasoning", format_token_count(reasoning)));
    }
    let mut lines = vec![parts.join(" \u{00b7} ")];
    let mut cache_parts = Vec::new();
    if let Some(read) = usage.cache_read_tokens {
        cache_parts.push(format!("{} read", format_token_count(read)));
    }
    if let Some(write) = usage.cache_write_tokens {
        cache_parts.push(format!("{} write", format_token_count(write)));
    }
    if !cache_parts.is_empty() {
        lines.push(format!("Cache: {}", cache_parts.join(" \u{00b7} ")));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::TokenUsage;

    #[test]
    fn format_token_count_zero() {
        assert_eq!(format_token_count(0), "0");
    }

    #[test]
    fn format_token_count_under_thousand() {
        assert_eq!(format_token_count(999), "999");
    }

    #[test]
    fn format_token_count_thousands() {
        assert_eq!(format_token_count(1000), "1\u{2009}000");
        assert_eq!(format_token_count(12345), "12\u{2009}345");
    }

    #[test]
    fn format_token_count_millions() {
        assert_eq!(format_token_count(1234567), "1\u{2009}234\u{2009}567");
    }

    #[test]
    fn format_token_count_negative() {
        assert_eq!(format_token_count(-1234), "-1\u{2009}234");
    }

    #[test]
    fn format_token_total_basic() {
        let usage = TokenUsage {
            input_tokens: 10000,
            output_tokens: 3000,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: Some(479),
        };
        assert_eq!(format_token_total(&usage), "13\u{2009}479");
    }

    #[test]
    fn format_token_tooltip_input_output_only() {
        let usage = TokenUsage {
            input_tokens: 12345,
            output_tokens: 678,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
        };
        assert_eq!(
            format_token_tooltip(&usage),
            "12\u{2009}345 input \u{00b7} 678 output"
        );
    }

    #[test]
    fn format_token_tooltip_with_reasoning() {
        let usage = TokenUsage {
            input_tokens: 12345,
            output_tokens: 678,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: Some(456),
        };
        assert_eq!(
            format_token_tooltip(&usage),
            "12\u{2009}345 input \u{00b7} 678 output \u{00b7} 456 reasoning"
        );
    }

    #[test]
    fn format_token_tooltip_with_cache() {
        let usage = TokenUsage {
            input_tokens: 12345,
            output_tokens: 678,
            cache_read_tokens: Some(9012),
            cache_write_tokens: Some(234),
            reasoning_tokens: None,
        };
        assert_eq!(
            format_token_tooltip(&usage),
            "12\u{2009}345 input \u{00b7} 678 output\nCache: 9\u{2009}012 read \u{00b7} 234 write"
        );
    }

    #[test]
    fn format_token_tooltip_full() {
        let usage = TokenUsage {
            input_tokens: 12345,
            output_tokens: 678,
            cache_read_tokens: Some(9012),
            cache_write_tokens: Some(234),
            reasoning_tokens: Some(456),
        };
        assert_eq!(
            format_token_tooltip(&usage),
            "12\u{2009}345 input \u{00b7} 678 output \u{00b7} 456 reasoning\nCache: 9\u{2009}012 read \u{00b7} 234 write"
        );
    }

    #[test]
    fn format_token_tooltip_cache_read_only() {
        let usage = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: Some(200),
            cache_write_tokens: None,
            reasoning_tokens: None,
        };
        assert_eq!(
            format_token_tooltip(&usage),
            "100 input \u{00b7} 50 output\nCache: 200 read"
        );
    }
}
