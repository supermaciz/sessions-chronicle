# Session Outcome Display -- Quiet Signals with Ending Text

**Issue:** [#90 -- Session outcome display and stopping point in session list](https://github.com/supermaciz/sessions-chronicle/issues/90)  
**Date:** 2026-03-27  
**Type:** Design plan  
**Status:** Draft  
**Exploration:** `docs/explorations/2026-03-27-session-outcome-display-exploration.md` -- Proposal C with Proposal D's colored ending text signal

## Problem

The session list still does not let users understand a session in under two
seconds. The exploration already established that Proposal C is the best fit for
the current product constraints: keep the standard `AdwActionRow`, keep the row
compact, and make the subtitle tell a clearer story.

The remaining weakness in Proposal C is the ending signal. A small colored dot
is easy to miss, requires a learned legend, and communicates less clearly than
text. The list needs a more explicit ending signal without taking on the layout
cost and maintenance burden of Proposal D's custom row structure.

## Scope

This design keeps Proposal C's compact `AdwActionRow` layout and replaces the
suffix status dot with short colored ending text.

**In scope:**
- dominant-activity subtitle formatting for session rows
- suffix ending label wording and color semantics
- accessibility and narrow-width behavior for the ending signal
- deterministic presentation rules for duration, activity, and ending state

**Out of scope:**
- replacing `AdwActionRow` with a custom two-line row
- token usage, subagent count, or stopping-point prose in the list row
- multi-chip activity summaries or third-line row layouts
- implementation planning in this document

---

## Recommended Approach

Adopt Proposal C as the base design and modify only the ending signal:

- keep the current `AdwActionRow` structure in `src/ui/session_row.rs`
- keep Proposal C's subtitle model: `project · duration · dominant activity · relative time`
- replace the suffix dot with a short colored text label before the chevron
- preserve the existing row density, navigation behavior, and right-click resume interaction

This is preferred over appending ending text to the subtitle because suffix text
remains visible longer on narrow widths. It is preferred over a partial Proposal
D hybrid because the row should still feel like a quiet refinement of the
existing list, not a redesign.

## Row Layout

```text
[icon]  Fix the flaky parser test in session_sources   completed  [>]
        my-project · 45m · 8 edits · 2d ago
```

- **Prefix:** existing 16px symbolic assistant icon
- **Title:** unchanged; first prompt when present, otherwise project fallback
- **Subtitle:** project, duration, dominant activity, relative time
- **Suffix:** ending label, then existing `go-next-symbolic` chevron
- **Row height:** unchanged from Proposal C's compact two-line `AdwActionRow`

The ending label is part of the suffix area, not the subtitle. This keeps the
list visually quiet while making the ending signal explicit.

## Presentation Rules

### Subtitle structure

The subtitle remains a single plain-text line in this exact order:

1. project
2. duration
3. dominant activity
4. relative time

This order preserves Proposal C's "subtitle tells a story" idea while keeping
recency available without crowding the title line.

### Dominant activity selection

The row shows one activity segment only. The goal is fast categorization, not a
full breakdown.

Use this deterministic precedence:

1. show edits when `edit_count > 0`
2. otherwise show commands when `command_count > 0`
3. otherwise show reads when `read_count > 0`
4. otherwise fall back to `message_count`

This keeps the most outcome-oriented signal visible first. File edits best imply
substantive work, commands are the next strongest signal, reads indicate
exploration, and message count remains the fallback when no tool-call activity
is worth surfacing.

### Activity counts

Each count is a simple operation count, not a distinct-file count. This avoids
fragile `json_extract` parsing on `input_json` (whose schema varies across
assistants) and keeps all four categories consistent — commands have no file
target, so mixing distinct files for edits/reads with operation counts for
commands would be incoherent.

### Activity wording

The activity segment uses short, human-readable singular/plural forms:

- `1 edit` / `N edits`
- `1 command` / `N commands`
- `1 read` / `N reads`
- `1 message` / `N messages`

No icons, pills, or multiple categories appear in the row.

### Ending label mapping

The backend-facing `ending_status` remains storage-oriented, but the UI uses
clearer presentation words:

| Stored status | Ending label | Visibility |
|---|---|---|
| `clean` | `completed` | shown |
| `abrupt` | `interrupted` | shown |
| `error` | `failed` | shown |
| `unknown` | none | hidden |

The wording is intentionally plain and self-explanatory. The row should not
require users to infer meaning from a symbol legend.

---

## Styling And Accessibility

### Visual treatment

The ending label uses semantic libadwaita color styling rather than custom hard-
coded colors:

- `completed` uses success styling
- `interrupted` uses warning styling
- `failed` uses error styling

The label remains lowercase and short to match Proposal C's understated visual
tone. It should read as lightweight status text, not as a badge or button.

### Accessibility

The ending label must not rely on color alone. The text itself carries the full
meaning, while color only reinforces it. This is a direct improvement over the
original suffix dot.

The row should continue to work in:

- high-contrast mode
- large-text mode
- screen-reader navigation
- narrow-width layouts

If the suffix label needs explicit accessible metadata, add it so assistive
technologies announce the ending state clearly.

### Narrow-width behavior

Subtitle text may still ellipsize on narrow windows, as already noted in the
exploration. Placing ending text in the suffix area mitigates the most important
loss: the ending signal stays visible longer than it would if appended to the
subtitle.

Unknown ending state remains hidden rather than showing `unknown`, because weak
signals should not add visual noise.

---

## Data And UI Boundaries

This design assumes the shared backend prerequisite from the exploration:

- denormalized activity counts on `sessions`
- denormalized `ending_status`
- duration derived from `last_updated - start_time`

No additional data model expansion is introduced for this design. In
particular, the row does not add:

- token usage
- per-row stopping-point summaries
- subagent indicators
- multiple activity categories

Those details belong in the session detail view or future follow-up work, not in
the highest-frequency scanning surface.

## Testing And Verification Expectations

This design should be implemented with both unit and manual verification in
mind.

### Unit test coverage

Session-row tests should cover:

- dominant activity precedence: edits > commands > reads > messages
- singular/plural formatting for each activity category
- ending label mapping for `clean`, `abrupt`, `error`, and `unknown`
- existing title/subtitle escaping behavior remaining intact

### Manual verification

Manual verification should use fixture data via `--sessions-dir tests/fixtures`
to confirm:

- mixed AI assistant rows remain visually consistent
- suffix text and chevron spacing remain stable
- large-text and narrow-width layouts remain readable
- high-contrast rendering preserves ending-label legibility

---

## Risks And Trade-Offs

### Suffix width pressure

Ending text adds more horizontal pressure than a dot. The risk is most visible
in large-text mode or future translations. This is acceptable because the chosen
labels are intentionally short, and suffix visibility is more valuable than the
minimal width savings of the dot.

### Unknown-state inconsistency

Rows with `unknown` ending state will have no ending label while others do. This
creates some visual variation, but it is preferable to showing low-confidence
status text that does not help triage.

### Less nuance than chips

Proposal C still shows only one dominant activity instead of a full activity
summary. This is a deliberate trade-off in favor of density, calmness, and low
implementation complexity.

## Decision

Implement Proposal C as the row model, with one change from Proposal D:

- keep the compact two-line `AdwActionRow`
- keep relative time in the subtitle
- replace the status dot with colored suffix text: `completed`, `interrupted`, `failed`
- hide the ending label for `unknown`
- keep message count as the fallback dominant activity when no edits, commands,
  or reads are present

This preserves Proposal C's low-disruption character while materially improving
ending-state clarity and 2-second scanability.
