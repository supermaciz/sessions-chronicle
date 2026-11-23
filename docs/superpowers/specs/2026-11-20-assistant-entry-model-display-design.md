# Model Attribution Display for Assistant Entries — Design

**Date**: 2026-11-20  
**Status**: Draft  
**Exploration**: [Display Model Attribution for Tool Calls and Subagents](../../explorations/2026-10-25-assistant-entry-model-display-exploration.md) (decision of 2026-10-27: the hybrid)  
**Prerequisite (merged)**: [Model storage for assistant transcript entries](2026-10-20-assistant-entry-model-storage-design.md), PR #223, schema v23  
**Mockups**: `docs/mockups/assistant-entry-model-display/` (01 for the summary section, 02 for run markers and the handoff caption)  

---

## Intent

A subagent session that contains only tool calls gives no way to know which
model ran it. The message header shows the model, but such a session has no
assistant message, so the model never reaches the screen. Since PR #223, every
tool call (`tool_calls.model`) and subagent (`subagents.source_model`) stores
its originating model. This change puts that data on screen.

What the user asked for:

- A session-level answer in the summary is enough for the triggering case.
- Model changes during a session must be visible where they happen.
- A subagent row must show both models and keep them clearly apart: the model
  that delegated and the model that actually ran the subagent.

## Goals

1. **Summary popover**: add a MODELS section that lists every model the session
   used.
2. **Transcript**: add thin run markers where the attributed model changes,
   including inside tool bursts and across stretches with no recorded model.
3. **Subagent rows**: add a handoff caption, `requested by … · ran …`, that
   says why a value is unknown.

## Non-goals

- Storing the child's own model on the parent side (Codex
  `collab_agent_spawn_end.model`, sub-agent activity `model`). It could be a
  separate, child-side field later.
- Proposal 3 elements: the relay lane, slot colours, batons, and the segmented
  MODELS bar. The bar may come later inside the MODELS section.
- A model label on tool-call rows.
- Any change to the per-message model label (`row_rendering.rs::model_label_text`).
- A model filter or a model facet in search or the session list.
- Any schema change. The data comes from v23 as it is.

## Anti-goals (failure even if it "works")

- **Inventing a model.** A missing value is never filled from a neighbour, the
  session, or the model requested in the call arguments (Claude Code
  `input.model`, Kimi `subagent_type`). See storage spec rule 4.
- **Rewriting slugs.** Show the stored string. No aliases, no provider
  guessing, no colour by provider (storage spec rule 5).
- **A new transcript list item.** Markers live inside existing rows. Search-match
  navigation, inspector routing, and scroll restoration rely on stable item
  indices.
- **Main-thread database work** for model aggregation or child lookups.
- **Flicker on unreparsable sessions**, meaning known → unknown → known markers
  on every tool burst of a session indexed before v23 that cannot be reparsed.

## Current state (verified 2026-11-20)

Re-checked on 2026-11-23 after #229 and #230: those commits change parsers,
fixtures, and format docs only. Nothing listed below has moved.

- `crates/core/src/database/mod.rs::load_transcript_items` selects `m.model`
  under the alias `model`. It does not select `tc.model`, `sa.source_model`, or
  `sa.child_session_id`. Columns are resolved by name, so the new columns need
  distinct aliases (for example `tool_call_model`, `subagent_source_model`,
  `subagent_child_session_id`). Otherwise `column_index("model")` keeps
  returning the message column without any error.
- `TranscriptItemRow` (`database/mod.rs:43`) has no fields for these columns.
- The transcript loads in the background: `SessionDetail::spawn_transcript_page_load`
  sends `SessionDetailCmd::TranscriptLoaded`, and `apply_transcript_rows`
  builds the display items (`build_display_items`, then
  `display.rs::group_transcript_rows` for bursts).
- Display items: `MessageItemInit`, `ToolCallItemInit`, `ToolBurstItemInit`,
  `SubagentItemInit`, and `ContextItemInit` in `transcript/item_init.rs`.
- Scroll infrastructure already exists: `display_targets_by_item_index:
  BTreeMap<i64, ScrollTarget>` maps a transcript `item_index` to a top-level
  row, plus an optional burst child. `apply_scroll_target` / `focus_scroll_target`
  scroll there and expand a collapsed burst when needed. Search navigation
  uses this today.
- The summary popover (`session_detail.rs`, `summary_popover`) holds
  `SessionSummary` (`session_detail/session_summary.rs`). Its sections are
  FIRST PROMPT, ACTIVITY, and TOKENS, each with a `section-heading` caption.
  `SessionSummary::update(&Session)` runs on `SetSession`, before the
  transcript loads.
- Subagent row: `typed_row.rs::build_subagent_page_content`, a single
  horizontal box (icon, title, optional Thinking pill, spacer, inspect button).
- Burst header: `typed_row.rs::rebuild_tool_burst_header`, category pills in an
  `adw::WrapBox`.
- The `subagents` table has `child_session_id`, with
  `idx_subagents_child(session_id, child_session_id)`.

## Definitions

**Attributed entry.** A transcript item that can carry a model:

| Item kind | Attributed value |
|---|---|
| Message with `role = assistant` | `messages.model` |
| Tool call | `tool_calls.model` |
| Subagent | `subagents.source_model` |
| User message, tool result, context entry | none: *transparent*, never part of a run |

Reasoning attachments are not entries of their own. They follow the item they
are attached to.

An empty or whitespace-only string counts as NULL, as in `model_label_text`.

**Run.** A maximal sequence of consecutive attributed entries, in
`transcript_items.item_index` order, with the same value. Transparent items do
not break a run. NULL is a value of its own: a NULL stretch is a run.

**Pre-attribution session (heuristic).** For run detection, treat tool calls and
subagents as transparent when at least one assistant message has a recorded
model, at least one tool call or subagent exists, and all tool calls and
subagents lack a model. This avoids spurious run changes in sessions indexed
before v23 that could not be reparsed. These entries still count in the MODELS
footer.

A freshly indexed Codex session can match the same pattern. For example, a
delegation discovered only from a spawn-end event has no source model. If it
is the sole delegation between two messages from model A and there are no tool
calls, the heuristic shows one continuous run of A. We accept this missed
unknown stretch to keep older histories readable.

The same pattern covers other NULL rows that are not generations: OpenCode V2
standalone completions, and Vibe Unified sessions whose only tool calls are
user-side `skill` calls (storage spec rules 7 and 8). There, treating them as
transparent is more accurate than a `model not recorded` marker. In every case
the unknown stays visible elsewhere: the subagent caption still reads
`requested by: not recorded` (Part 4), and the entry still counts in the
MODELS footer.

## Part 1 — Core: `session_models`

A new read function in `sessions-chronicle-core` (name is the builder's call,
for example `load_session_models(db_path, session_id) -> Result<SessionModels>`):

```text
SessionModels {
    models: Vec<SessionModelUsage>,   // first-use order
    unattributed_count: usize,        // attributed-kind entries with NULL model
}
SessionModelUsage {
    model: String,                    // stored slug, unchanged
    entry_count: usize,
    first_item_index: i64,            // transcript_items.item_index of first entry
    first_used_at: Option<i64>,       // see below
}
```

- Entries are the attributed entries of the session's own `transcript_items`,
  each counted once. Entries owned by an indexed child session belong to that
  child, not to the parent.
- `first_used_at` comes from the entry at `first_item_index`. Use
  `messages.timestamp` for a message or `tool_calls.started_at` for a tool call.
  If that entry lacks a timestamp (including every subagent entry), return
  `None` and omit the time from the summary, even if a later entry has one.
- A session with no row in `sessions`, or with no attributed entry at all,
  returns empty `models`. The caller tells "not indexed" apart from "not
  recorded" (Part 4).
- The function is used for the current session (summary) and for each
  subagent's child session ("ran"). Rows sharing a `child_session_id` (resumed
  subagents, swarms) are looked up once per load.

Builder's call: the function may run one SQL aggregate or a light ordered query
aggregated in Rust. Ordering must follow `transcript_items.item_index`.

## Part 2 — Summary popover: MODELS section

Placed between ACTIVITY and TOKENS. Same `section-heading` caption, label
`MODELS`, same spacing as its neighbours.

- **One row per model**, in first-use order, each a horizontal `gtk::Box`:
  - the slug, `gtk::Label` with the `monospace` class, ellipsized at the end,
    full slug as tooltip;
  - `N entries` (`dim-label`; singular for 1);
  - `first used HH:MM` (`dim-label`) when `first_used_at` is known, in the same
    local-time format as the message header;
  - a `gtk::Button` with classes `flat` and `circular`, icon `go-down-symbolic`,
    tooltip "Jump to first use", accessible label "Jump to first use of <slug>".
- **Jump**: pops the popover down, then scrolls the transcript to
  `first_item_index`, resolved through `display_targets_by_item_index` and
  applied through the existing `apply_scroll_target` path. That path also
  expands a collapsed burst when the target is a child.
- **Single model**: one row, no jump button.
- **Footer**: when `unattributed_count > 0`, a `caption dim-label` line reading
  `N entries with no recorded model`.
- **Hidden** when `models` is empty, even if `unattributed_count > 0`. Nothing
  is attributed, so there is nothing to list ("unknown stays unknown", no
  filler).
- **Timing**: `SessionSummary::update(&Session)` keeps running on `SetSession`.
  The section is filled when the model data arrives, after the transcript load
  (Part 5), with a separate method (for example `update_models`). Until then it
  stays hidden. A stale result (old `request_id` / `session_id`) is dropped,
  like `TranscriptLoaded`.

## Part 3 — Transcript: run markers

### Computation

Runs are computed once, while display items are built from the loaded rows. A
pure function over the ordered rows gives each attributed entry an
`Option<ModelRunMarker>`:

```text
ModelRunMarker {
    model: Option<String>,     // None = "model not recorded"
    previous: Option<String>,  // previous run's model, when it was known
}
```

Rules:

1. If the session has no attributed entry with a non-NULL model, there are no
   markers.
2. The first entry of every run gets a marker, the first run included. The
   first run's marker names its model with no `previous`. If the first run is
   NULL, it gets no marker: there is nothing before it to contrast with.
3. Later markers set `previous` to the previous run's model when that model
   was known. After a NULL run, `previous` is `None`. Example: Opus → NULL →
   Opus gives `opus`, then `model not recorded`, then `opus` (no "from").
4. Switching back counts: Opus → Sonnet → Opus gives three markers.
5. In a pre-attribution session, tool calls and subagents are transparent
   (see Definitions).
6. Known limitation (my call): NULL also covers user-side Vibe Unified `skill`
   calls and rows discovered without a generation context (storage spec rule 7
   and rule 8, plus its hand-off section). Storage does not tell these cases
   apart, so they can draw a `model not recorded` marker. These rows are rare,
   and not marking them would make a real unrecorded stretch look as if it
   inherited the previous model. A later storage field could separate them.

The marker is stored on `MessageItemInit`, `ToolCallItemInit`, and
`SubagentItemInit` (for example a `model_run: Option<ModelRunMarker>` field).
`ToolBurstItemInit` gets the data needed for the burst rules below. The row
only shows or hides a pre-built marker. `bind` does no computation.

### Widget

- `gtk::Box` with the class `model-run-marker`, prepended inside the row's root
  and hidden when there is no marker. Children: a short leading
  `gtk::Separator`, a `gtk::Label` with `caption monospace` (the model, or the
  plain text `model not recorded` without monospace), an optional
  `gtk::Label` with `caption dim-label` reading `from <previous>`, and a
  trailing `gtk::Separator` with `hexpand`.
- Not focusable, not searchable, no copy affordance.
- CSS: only `.model-run-marker` (margins, separator opacity) in
  `data/resources/style.css`. No new colours. Light, dark, and high-contrast
  themes come from the theme.
- Accessibility: the marker text joins the row's accessible description, for
  example "Model changed to claude-sonnet-4-6 from claude-opus-4-7" or
  "Model: claude-opus-4-7" for the first run.

### Tool bursts

- A run that starts at the burst's **first child** is drawn once, above the
  whole burst row (outside the header), so it is visible collapsed and
  expanded. The child itself draws no marker.
- A run that starts at a **later child** draws its marker on that child row
  inside the revealer. While the burst is **collapsed**, the header also shows a
  hint after the pills: `→ <model>` (`caption monospace dim-label`), naming the
  model active at the burst's last child, or `→ model not recorded`. When a
  burst holds several changes, the hint's tooltip lists them in order (my
  call). The hint is hidden while the burst is expanded, where the child
  markers carry the information.

## Part 4 — Subagent rows: handoff caption

The subagent root becomes vertical. The existing horizontal title row is kept
as it is, and a caption line (`caption dim-label`, ellipsized at the end,
tooltip with the full text) is added under it:

`requested by <source_model> · ran <child models>`

| Case | Left side | Right side |
|---|---|---|
| `source_model` known | `requested by <slug>` | |
| `source_model` NULL | `requested by: not recorded` | |
| Child with one model | | `ran <slug>` |
| Child with several models | | `ran <first slug> +N`, tooltip listing every child model in first-use order |
| `child_session_id` NULL | | `no linked child session` (my call: the exploration only had "not indexed", which would be wrong here) |
| `child_session_id` set, no `sessions` row | | `child session not indexed` |
| Child indexed, `session_models(child).models` empty | | `child model not recorded` |

- Slugs are rendered in `<tt>` markup. They are escaped before markup is built,
  because slugs come from untrusted session files.
- "Ran" comes only from `session_models(child)`, never from call arguments.
- Each subagent row shows its own `source_model`. Rows sharing a child show
  the same "ran" value. Values are never harmonized across rows.
- The caption joins the row's accessible label.
- In a pre-attribution session, the left side still reads
  `requested by: not recorded`, which is true. Only run detection is
  affected by that rule.

## Part 5 — Loading and data flow

1. Extend `load_transcript_items` and `TranscriptItemRow` with the three new
   columns, under distinct aliases (`tool_call_model`, `subagent_source_model`,
   `subagent_child_session_id`). Add a test asserting that a row with both
   `m.model` and `tc.model` set (or a crafted fixture) reads each from its own
   column.
2. In the background closure of `spawn_transcript_page_load`, after
   `load_all_transcript_items`:
   - compute `session_models` for the current session;
   - collect the distinct `child_session_id` values from the rows, then for
     each one check whether a `sessions` row exists and compute
     `session_models(child)`.
   Carry the results in `SessionDetailCmd::TranscriptLoaded`, or in a sibling
   command with the same `request_id`. This is the builder's call, but a
   failed model lookup must not fail the transcript. It should log at `warn`
   and degrade to "no information".
3. On the main thread, `apply_transcript_rows` computes the run markers from the
   rows, feeds the child results into `SubagentItemInit` as an already
   resolved caption state, and calls the summary's model update.
4. Cache: a per-load map keyed by `child_session_id` deduplicates lookups. No
   cross-load cache (my call). The aggregate is cheap with the existing
   indexes, and a cross-load cache would go stale after reindexing.

The sessions list and the search provider are unaffected.

## Part 6 — Validation

Unit and integration tests (headless, as in AGENTS.md):

- **Core `session_models`**: first-use order; per-model counts; the
  unattributed count; empty-string models treated as NULL; `first_used_at`
  matching the first message timestamp or first tool call start time; a first
  use without a timestamp (subagent or tool call) followed by a timestamped
  message of the same model keeps the original index and yields `None`;
  a session that does not exist; child entries not counted in the parent;
  a session shaped like the pre-attribution false negative below counts the
  NULL subagent in `unattributed_count`.
- **Transcript query**: the three new columns read under their own aliases and
  never shadow `m.model`.
- **Run computation (pure function)**: single model (one marker); switch;
  switch back (three markers); NULL in the middle (marker with no `previous`
  afterwards); leading NULL run (no marker); all NULL (no markers);
  transparent user and context items; pre-attribution session (no markers for
  NULL tool calls); the accepted false negative, two model-A assistant
  messages around a sole subagent with no source model and no tool call (one
  A marker, no `model not recorded` marker); a run starting at a burst's first
  child vs a later child.
- **Subagent caption text**: every row of the Part 4 table, plus escaping of a
  slug containing `<`/`&`.
- **GTK (`#[gtk::test]`)**: the MODELS section hidden, single model (no jump
  button), several models with footer; a marker visible on the correct row;
  the collapsed-burst hint shown, then hidden once expanded; the subagent
  caption present.
- **Fixtures**: add or extend fixtures in `tests/fixtures/` so at least one
  session switches model mid-session with tool-call-only stretches, and one
  parent links an indexed tool-call-only child. Then check by hand with
  `--sessions-dir tests/fixtures`.
  `codex_sessions/2026/11/20/rollout-2026-11-20T10-00-00-turn-context-0169.jsonl`
  (added in #230) already switches model between two assistant messages
  (`gpt-5.5-codex` → `gpt-5.5-codex-mini`) but has no tool call. It can be
  extended with tool calls rather than starting from scratch. Adding a Codex
  fixture changes the session count asserted in
  `crates/core/src/database/indexer.rs` (currently 6).

Definition of done: `cargo fmt --all -- --check`, `cargo clippy --all -- -D
warnings`, the headless test suite, and screenshots of the MODELS section, a
run marker, a collapsed-burst hint, and a subagent caption, in light and dark
themes, for the PR.

## Decisions taken in this spec (beyond the exploration)

- The pre-attribution rule is a heuristic applied to run detection only. Its
  false negative is accepted: a session where every tool call and subagent
  lacks a model, for example a fresh Codex session with a sole spawn-end-only
  delegation, shows one continuous run. The subagent caption and the MODELS
  footer still show the unknown.
- `first_used_at` is read only from the entry at `first_item_index`, so the
  time shown matches the jump target. When that entry has no timestamp
  (always for a subagent), the time is omitted, even if a later entry of
  the same model has one.

- A leading NULL run draws no marker. After a NULL run, the next marker has
  no `from`.
- NULL rows that are not generations (Vibe `skill` calls, standalone
  completions) are accepted as possible `model not recorded` markers, as a
  documented limitation.
- A run starting at a burst's first child is drawn above the burst row. The
  collapsed hint names the model active at the burst's end, and its tooltip
  lists every change.
- `no linked child session` is added for `child_session_id IS NULL`.
- The MODELS section is hidden when nothing is attributed, even if the footer
  count would be non-zero.
- The child lookup is deduplicated per load, with no cross-load cache.
- Model data arrives with the transcript load, so the MODELS section appears
  after the transcript, not on `SetSession`.

## Left to the builder

- Function and type names, and whether `session_models` aggregates in SQL or
  in Rust.
- Whether model data rides on `TranscriptLoaded` or on a sibling command.
- How the summary's jump button reaches `SessionDetail` (sender, callback, or
  output message).
- Exact margins and spacing in `.model-run-marker`, within the existing visual
  rhythm.
- Fixture content, as long as it covers the cases in Part 6.
