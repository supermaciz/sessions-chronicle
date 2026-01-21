# Session Resumption: Failure Notification Spec

**Status**: Updated (Simplified)
**Last Updated**: 2026-01-21
**Related**: `PROJECT_STATUS.md`

---

## Overview

Session resumption launches a terminal emulator and runs the resume command (for example, the
command built by `build_resume_command()` and launched via `spawn_terminal()`).

This document replaces the previous multi-step "resumption improvements" plan with a smaller,
high-impact change:

- Notify the user when "Resume in Terminal" fails.

The primary scenario is: the terminal configured in Preferences is not available on the system.

---

## Problem Statement

### Current Issue

On some systems (notably Flatpak builds), the terminal launch can fail silently when the selected
terminal emulator does not exist on the host system. In practice, a wrapper process may start
successfully while the actual terminal does not, leaving the user with no feedback.

### User Impact

- User clicks "Resume in Terminal" and nothing appears.
- There is no clear, actionable message telling them what to fix.

---

## Desired Behavior (GNOME HIG)

When the resume action fails, the app must:

- Show a transient, non-blocking notification (toast) in the main window.
- Use short, human-readable wording (avoid technical errors in the UI).
- Provide an obvious path to fix the issue (open Preferences) when applicable.

This spec is failure-only.

- Do not show a success toast.
- Do not add progress indicators, spinners, or button-loading states.

---

## Proposed Implementation

### 1) Detect "Terminal Not Available" Reliably

**Location**: `src/utils/terminal.rs`

The terminal spawning code must validate the resolved terminal before launching.

Implementation notes:

- Resolve `Terminal::Auto` to a concrete terminal.
- Check availability for the resolved terminal using the same mechanism as the availability
  detection logic (including Flatpak host checks).
- If unavailable, return a clear, specific error early (for example: "Ptyxis is not available").

This avoids false positives where a wrapper process launches but no terminal appears.

### 2) Notify via Toast on Failure

**Location**: `src/app.rs` in the `AppMsg::ResumeSession` failure path.

Use `adw::ToastOverlay` + `adw::Toast` to present the failure.

Implementation notes:

- Add a `ToastOverlay` to the main window content.
- On failure, show a toast with a short title and a short timeout (3-5 seconds).
- Optional: add a "Preferences" button that opens the existing Preferences action.

Suggested toast titles:

- "No terminal emulator found" (Auto selection with no available terminal)
- "Ptyxis is not available" (explicit preference missing)

---

## Success Criteria

- If the configured terminal emulator is missing, the user always sees a toast.
- The toast message is understandable and actionable.
- No silent failures.

---

## Out of Scope (Deferred)

- Progress indication (spinners, "Launching..." state).
- Success notifications.
- Claude CLI installation checks.
- Tooltip/accessibility enhancements.
- Additional tests beyond what already exists.
