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
