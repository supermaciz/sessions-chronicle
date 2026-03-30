# Session Row Duration Removal Design

**Date:** 2026-03-29  
**Type:** Design plan  
**Status:** Approved

## Problem

The session list currently shows a wall-clock duration derived from
`last_updated - start_time`. That value can include long inactive gaps when a
session is resumed later, which makes it easy to misread as active work time.

## Scope

Replace the duration segment in the session-row subtitle with a message-count
segment while keeping the dominant-activity and relative-time signals.

**In scope:**
- `src/ui/session_row.rs` subtitle formatting
- session-row unit tests that assert subtitle content

**Out of scope:**
- changing how `message_count` is computed or stored
- changing dominant-activity precedence or wording
- changing relative-time formatting
- changing the session detail view

## Recommended Approach

Keep the current compact subtitle structure but swap the ambiguous duration for a
reliable message count.

New subtitle format:

```text
<location> · <message_count> messages · <dominant_activity> · <relative_time>
```

Examples:

- `my-project · 12 messages · 8 edits · 5m ago`
- `my-project · 3 messages · 2 commands · 1d ago`

## Presentation Rules

1. Show the location segment exactly as today.
2. Always show `message_count` using the existing singular/plural wording style.
3. Keep dominant activity selection unchanged: edits, then commands, then reads,
   then messages.
4. Keep relative time based on `last_updated` unchanged.
5. Remove all duration calculation and formatting from the session-row subtitle.

## Rationale

`message_count` communicates session size without implying continuous work.
Keeping dominant activity and relative time preserves the current scan value:
users still see what kind of session it was and how recent it is.

## Testing And Verification

Unit tests should verify:

- subtitles no longer include the duration segment
- subtitles include message count before dominant activity
- dominant activity precedence still works
- message fallback behavior still works when no activity counts are present

Manual verification should confirm fixture-backed rows read naturally in the list
and remain compact.
