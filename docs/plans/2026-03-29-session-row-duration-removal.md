# Session Row Duration Removal Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the ambiguous wall-clock duration in session-row subtitles with message count while keeping dominant activity and relative time.

**Architecture:** The change stays local to `SessionRow::session_subtitle` and its unit tests. No database or parser work is needed because `message_count` already exists on `Session` and the row already formats dominant activity and relative time.

**Tech Stack:** Rust 2024, Relm4/libadwaita UI, cargo test

---

### Task 1: Update session-row subtitle formatting

**Files:**
- Modify: `src/ui/session_row.rs`
- Test: `src/ui/session_row.rs`

**Step 1: Write the failing test**

Adjust the existing subtitle test so it expects `7 messages` in the subtitle and
does not expect the duration segment.

**Step 2: Run test to verify it fails**

Run: `cargo test session_subtitle_shows_message_count_and_dominant_activity -- --exact`
Expected: FAIL because the subtitle still contains duration instead of message count.

**Step 3: Write minimal implementation**

Update `SessionRow::session_subtitle` to remove the duration calculation and use
`session.message_count` in the subtitle between location and dominant activity.

**Step 4: Run test to verify it passes**

Run: `cargo test session_subtitle_shows_message_count_and_dominant_activity -- --exact`
Expected: PASS.

### Task 2: Verify adjacent session-row behaviors still hold

**Files:**
- Test: `src/ui/session_row.rs`

**Step 1: Run focused session-row tests**

Run: `cargo test session_subtitle_ -- --nocapture`
Expected: PASS for subtitle-related tests, including dominant-activity precedence
and message fallback coverage.

**Step 2: Run formatting and lint verification if the file changed shape**

Run: `cargo fmt --all -- --check`
Expected: PASS.

**Step 3: Commit**

Only if requested by the user:

```bash
git add src/ui/session_row.rs docs/plans/2026-03-29-session-row-duration-removal-design.md docs/plans/2026-03-29-session-row-duration-removal.md
git commit -m "fix: replace session row duration with message count"
```
