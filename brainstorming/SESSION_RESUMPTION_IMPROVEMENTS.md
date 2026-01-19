# Session Resumption Improvements - Implementation Plan

**Status**: Reviewed
**Last Updated**: 2026-01-19
**Related**: `NEXT_STEPS.md`, `PROJECT_STATUS.md`

---

## Review Notes

> **Review Date**: 2026-01-19
>
> This document has been reviewed against the current codebase and libadwaita/GTK4 documentation.
> Code examples have been corrected to match the actual libadwaita API.

---

## Overview

This document outlines specific improvements to the session resumption feature that is currently marked as complete but could benefit from enhanced user experience, error handling, and testing.

---

## Priority 1: Visual Feedback During Terminal Launch

### Current State
- User clicks "Resume in Terminal" button
- No visual indication that action is processing
- Success/failure only shown via error dialogs on failure

### Proposed Implementation

#### 1. Add Toast Notifications

**Location**: `src/app.rs` in `AppMsg::ResumeSession` handler

> **Technical Note**: The current `app.rs` does not have a `ToastOverlay`. The main window content
> is wrapped in `adw::ToolbarView` (line 89). To add toast notifications, we need to wrap the
> existing content with `adw::ToastOverlay` and store a reference to it.

**Implementation**:
```rust
// In view! macro - wrap existing ToolbarView content with ToastOverlay
#[name = "toast_overlay"]
adw::ToastOverlay {
    set_child = &adw::ToolbarView { /* existing content */ }
}

// After successful terminal spawn - include terminal name for clarity
let terminal_name = terminal.to_string(); // e.g., "Ptyxis", "GNOME Console"
let toast = adw::Toast::new(&format!("Launched in {}", terminal_name));
toast.set_timeout(3); // Auto-dismiss after 3 seconds
widgets.toast_overlay.add_toast(toast);
```

> **API Note**: `adw::ToastOverlay::add_toast()` takes ownership of the toast and returns nothing.
> To dismiss a toast early, keep a reference and call `toast.dismiss()` on it.
> For simple success notifications, auto-dismiss with `set_timeout()` is preferred.

#### 2. Temporary Button State

**Location**: `src/ui/session_detail.rs` and `src/ui/session_list.rs`

**Implementation**:
- Add `resuming` state to model
- Bind button sensitivity: `#[watch] set_sensitive: !model.resuming`
- Set label to "Launching..." during operation
- Reset on completion/failure

#### 3. Progress Indication

**Optional Enhancement**:
- Add spinner icon during operation
- Show which terminal is being launched

### Benefits
- Immediate user feedback
- Clear action confirmation
- Professional polish

### Estimation
- **Effort**: 2-3 hours
- **Complexity**: Low
- **Impact**: High

---

## Priority 2-3: Claude Installation Check & Enhanced Tooltips

### Improvement 2: Claude Installation Verification

#### Current Issue
- If Claude CLI is not installed, command fails silently
- User sees generic "Failed to launch terminal" error
- No proactive warning

#### Proposed Solution

**Location**: `src/utils/terminal.rs`

**Implementation**:

1. **Add installation check function**:

> **Note**: This reuses the existing `is_flatpak()` function from `terminal.rs`.
> The pattern matches the existing `Terminal::is_available()` implementation.

```rust
pub fn check_claude_installed() -> bool {
    if is_flatpak() {
        Command::new("flatpak-spawn")
            .arg("--host")
            .arg("which")
            .arg("claude")
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else {
        which::which("claude").is_ok()
    }
}
```

2. **Integrate into resume flow**:
```rust
// In build_resume_command
if !check_claude_installed() {
    return Err(anyhow::anyhow!(
        "Claude CLI not found. Please install Claude Code CLI first."
    ));
}
```

3. **Enhanced error message**:
```rust
// In app.rs error handling
"Failed to Launch Terminal" => {
    "Claude CLI is not installed or not in PATH. "
    "Please install Claude Code CLI and try again."
}
```

#### Benefits
- Proactive error prevention
- Clear, actionable error messages
- Better debugging experience

#### Estimation
- **Effort**: 1-2 hours
- **Complexity**: Low
- **Impact**: High

### Improvement 3: Enhanced Tooltips and Accessibility

#### Current State
- Basic tooltips on resume buttons
- No keyboard shortcut indication
- Limited accessibility features

#### Proposed Enhancements

**Location**: `src/ui/session_detail.rs` and `src/ui/session_list.rs`

**Implementation**:

1. **Detailed tooltips**:
```rust
// SessionDetail button
set_tooltip_text: Some(
    "Resume this session in your preferred terminal emulator\n"
    "(Configurable in Preferences → Terminal)"
)

// SessionList button
set_tooltip_text: Some(
    "Resume session in terminal\n"
    "Uses your preferred terminal emulator from settings"
)
```

2. **Keyboard accessibility**:
```rust
// Add mnemonic
resume_button.set_use_underline(true);
resume_button.set_label("_Resume in Terminal");
```

> **Deferred**: `Ctrl+R` accelerator is out of scope for initial implementation.
> Adding an accelerator requires `AccelsPlus` setup in `app.rs` - can be added later.

3. **ARIA labels** (future GTK version):
```rust
// When GTK supports it
resume_button.set_property("accessible-name", "Resume session button");
```

#### Benefits
- Better discoverability
- Improved accessibility
- Professional polish

#### Estimation
- **Effort**: 1 hour
- **Complexity**: Very Low
- **Impact**: Medium

---

## Priority 4: Unit Testing for Terminal Utilities

### Current State
- `build_resume_command()` has basic tests ✓
- `test_terminal_from_str()` - Terminal parsing ✓
- `test_terminal_to_str()` - String conversion ✓
- `spawn_terminal()` has no tests (difficult to test - spawns real processes)
- Limited error case coverage

### Proposed Test Suite

**Location**: `src/utils/terminal.rs` (test module)

**Implementation**:

1. **Test command building**:
```rust
#[test]
fn test_build_resume_command_edge_cases() {
    // Test with special characters in session_id
    let cmd = build_resume_command("session-with spaces", Path::new("/test"));
    assert!(cmd.is_ok());
    
    // Test with complex paths
    let cmd = build_resume_command("test", Path::new("/path/with spaces/project"));
    assert!(cmd.is_ok());
}
```

2. **Test terminal resolution**:
```rust
#[test]
fn test_terminal_resolution() {
    // Test Auto resolution
    let terminal = Terminal::Auto;
    let result = terminal.resolve_auto();
    // Should return Ok if any terminal available, Err otherwise
    
    // Test specific terminals
    assert_eq!(Terminal::Ptyxis.executable(), Some("ptyxis"));
    assert_eq!(Terminal::Auto.executable(), None);
}
```

3. **Mock terminal spawning**:

> **Review Note**: Testing `spawn_terminal()` with invalid commands is tricky because it spawns
> real processes. The `mockall` dependency is heavy for this use case. Consider testing the
> command construction rather than actual spawning. Integration tests should be marked `#[ignore]`
> and run manually.

```rust
#[test]
fn test_spawn_terminal_command_construction() {
    // Test that commands are built correctly before spawning
    // Focus on argument formatting, not actual process execution
    let terminal = Terminal::Ptyxis;
    let args = ["echo", "test"];
    // Verify the command would be constructed correctly
    assert!(terminal.is_available()); // Only if terminal installed
}

#[test]
#[ignore = "spawns real processes - run manually"]
fn test_spawn_terminal_error_handling() {
    // Test with invalid command
    let result = spawn_terminal(Terminal::Ptyxis, &["nonexistent-command"]);
    assert!(result.is_err());
}
```

4. **Integration test** (optional):
```rust
#[test]
#[ignore = "requires actual terminal"]
fn test_full_resume_flow() {
    // This would be a manual test or integration test
    // Tests the full flow from command building to spawning
}
```

### Test Infrastructure Needs
- Consider `tempfile` crate for path testing with temporary directories
- Add test fixtures for different scenarios
- Mark integration tests with `#[ignore]` for manual execution

> **Review Note**: Heavy mocking frameworks (`mockall`, `condvar`) are not recommended.
> Focus on testing command construction and argument formatting instead.

### Benefits
- Prevents regressions
- Documents expected behavior
- Enables safe refactoring
- Improves code quality

### Estimation
- **Effort**: 3-4 hours
- **Complexity**: Medium
- **Impact**: Medium (long-term)

---

## Implementation Roadmap

### Phase 1: Quick Wins (1-2 days)
1. ✅ Add visual feedback (toasts + button states)
2. ✅ Implement Claude installation check
3. ✅ Enhance tooltips and accessibility

### Phase 2: Testing (1 day)
1. ✅ Add unit tests for terminal utilities
2. ✅ Add integration test documentation
3. ✅ Update CI to run new tests

### Phase 3: Polish (Optional)
1. Add spinner animation during launch
2. Add terminal selection to success toast
3. Add metrics for resume attempts

---

## Success Criteria

### For Visual Feedback
- [ ] User sees immediate feedback when clicking resume
- [ ] Success/failure clearly indicated
- [ ] No UI freezing during terminal launch

### For Claude Check
- [ ] Proactive error if Claude not installed
- [ ] Clear installation instructions in error
- [ ] Works in both Flatpak and native modes

### For Tooltips
- [ ] All resume buttons have detailed tooltips
- [ ] Keyboard shortcuts documented
- [ ] Accessibility standards met

### For Testing
- [ ] All public terminal functions have tests
- [ ] Edge cases covered
- [ ] Test coverage > 80% for terminal module

---

## Dependencies

### Crate Additions (for testing)
```toml
[dev-dependencies]
tempfile = "3.3"  # For path testing
```

> **Review Note**: `mockall` and `condvar` are heavy dependencies for this use case.
> Consider testing command construction rather than mocking process spawning.
> Integration tests can be marked `#[ignore]` and run manually when needed.

### Documentation Updates
- Update `README.md` with testing instructions
- Add testing section to `DEVELOPMENT_WORKFLOW.md`
- Update `NEXT_STEPS.md` to reflect completion

---

## Risks and Mitigations

### Risk: Toast implementation complexity
**Mitigation**: Start with simple toast, enhance later

### Risk: Terminal detection false positives
**Mitigation**: Test on multiple systems, add logging

### Risk: Test flakiness with external commands
**Mitigation**: Use mocking for unit tests, manual for integration

---

## Design Decisions (Confirmed)

The following decisions were confirmed during review:

1. **Toast message content**: Include the resolved terminal name for clarity
   - Example: "Launched in Ptyxis" rather than generic "Terminal launched successfully"
   - Helps users understand which terminal was used (especially with Auto selection)

2. **Keyboard shortcut (`Ctrl+R`)**: Deferred to future scope
   - Keep initial implementation focused on visual feedback
   - Can be added later with `AccelsPlus` setup

3. **Claude CLI error handling**: Simple error message only
   - No "Learn More" button or link to installation docs
   - Clear, actionable message: "Claude CLI not found. Please install Claude Code CLI first."

4. **Testing strategy**: Prefer command construction tests over spawn mocking
   - Avoid heavy dependencies like `mockall`
   - Integration tests marked `#[ignore]` for manual execution

---

## Future Enhancements (Out of Scope)

1. **Resume history tracking**
   - Store timestamp of each resume
   - Show "Last resumed X hours ago"

2. **Per-session terminal preferences**
   - Override global preference for specific session
   - Remember last used terminal per session

3. **Resume from command line**
   - `sessions-chronicle --resume SESSION_ID`
   - Direct terminal launch without GUI

4. **Multi-tool resume support**
   - Extend to OpenCode/Codex when implemented
   - Abstract resume command building

---

## Appendix: Reference Code Snippets

### Toast Implementation Example
```rust
// In app.rs view! macro - wrap ToolbarView with ToastOverlay
#[name = "toast_overlay"]
adw::ToastOverlay {
    set_child = &adw::ToolbarView { /* existing content */ }
}

// In resume handler - simple auto-dismissing toast
let terminal_name = resolved_terminal.to_string();
let toast = adw::Toast::new(&format!("Launched in {}", terminal_name));
toast.set_timeout(3); // Auto-dismiss after 3 seconds
widgets.toast_overlay.add_toast(toast);

// Alternative: Keep reference for early dismissal if needed
let toast = adw::Toast::new("Preparing terminal...");
toast.set_timeout(0); // No auto-dismiss
let toast_ref = toast.clone(); // Keep reference
widgets.toast_overlay.add_toast(toast);

// Later, to dismiss early:
toast_ref.dismiss();
```

> **API Clarification**: `adw::ToastOverlay::add_toast()` does not return a toast ID.
> To dismiss a toast programmatically, keep a clone of the `adw::Toast` and call `.dismiss()` on it.

### Button State Management
```rust
// In SessionDetail model
struct SessionDetail {
    // ... existing fields
    is_resuming: bool,
}

// In view macro
#[watch]
set_sensitive: !model.is_resuming,
set_label: if model.is_resuming {
    "Launching..."
} else {
    "Resume in Terminal"
}
```

---

**Next Steps**:
1. Implement Priority 1 improvements (visual feedback)
2. Add Priority 2-3 (Claude check + tooltips)
3. Write Priority 4 tests
4. Update documentation and mark as complete

**Owner**: [Your Name]
**Reviewers**: [Team Members]
**Target Completion**: 2026-01-26
