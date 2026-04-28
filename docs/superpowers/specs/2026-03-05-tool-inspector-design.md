# Tool Inspector Design (Issue #46)

## Problem

The current Tool Inspector shows raw text blocks for `input_json`, `output_text`,
and `error_text`, without tool-aware rendering. Inline transcript chips only show
name/status/duration, so users do not get enough context before opening the pane.

The exploration document selected Proposal B (specialized per-tool rendering) as
the best direction.

## Decision

Adopt **Proposal B (Specialized Views)** with a **component-based architecture**:

1. Keep existing app/container structure (`AdwOverlaySplitView`, right-side
   utility pane, F9 toggle).
2. Each renderer is a Relm4 `SimpleComponent` in its own module, orchestrated
   by a `gtk::Stack` in the inspector pane.
3. Migrate `ToolInspectorPane` from `SimpleComponent` to `Component` to support
   async data loading via `CommandOutput`.
4. Do not add GtkSourceView in this phase.

## Delivery Strategy (Phased)

To reduce regression risk, delivery is split into incremental phases.

- **Phase A (safety foundation)**: async loading migration, explicit inspector
  load state, stale-result protection.
- **Phase B (rendering architecture)**: renderer registry + `gtk::Stack`
  switching + strict fallback behavior.
- **Phase C (polish and depth)**: diff-quality improvements, richer inline
  previews, and CSS refinements.

Release gating requires Phase A and Phase B to pass. Phase C can ship
incrementally if fallback quality remains acceptable.

## Scope

In scope:

- Tool-aware inspector rendering for common tool families.
- Contextual inline chip previews derived from tool input/output.
- Responsive pane width tuning for code-heavy content.
- Stable generic fallback for unknown/custom/MCP tools.
- Per-tool-type icons in chips and inspector header.

Out of scope:

- DB schema changes.
- Full parser redesign (targeted parser fixes are in scope as prerequisites).
- Full markdown feature parity expansion beyond current renderer.
- New heavy rendering dependency (GtkSourceView).

## Parser Prerequisites

Parser work is explicitly prioritized so lower-impact metadata improvements do
not block core inspector delivery.

| Parser | Gap | Impact | Priority |
|---|---|---|---|
| Claude Code | `is_error` field on `tool_result` messages is not checked | Error tools shown as `Completed` instead of `Error`; error styling never triggers | **Blocking (Phase B)** |
| Codex CLI | Newer `response_item.function_call*` format not parsed | Recent Codex sessions have invisible tool calls | **Blocking (Phase B)** |
| OpenCode | `state.time.start/end` and `state.metadata.exit` not extracted | `duration_ms` is often NULL; no exit code badge for Bash tools | Important (Phase C / v1.1) |
| OpenCode | `state.title` not extracted | Less contextual chip/header labeling | Nice-to-have (Phase C / v1.1) |

These are targeted fixes to existing parser code, not a full parser redesign.

## Constraints

- Respect current Relm4 architecture; use `SimpleComponent` for renderers and
  `Component` (with `CommandOutput`) for the inspector pane.
- Keep parser/storage compatibility across Claude Code, OpenCode, Codex, and
  Mistral Vibe.
- Treat tool payloads as untrusted data; rendering must be robust to malformed
  JSON and partial fields.

## High-Level Architecture

### 1) App shell and pane behavior

- Keep `AdwOverlaySplitView` as the utility-pane container in `src/app/mod.rs`.
- Keep inspector on the right (`UtilityPaneMode::ToolInspector` in
  `src/app/types.rs`).
- Keep existing F9 toggle behavior and overlay-on-narrow-window behavior.
- Set pane width constraints for code-heavy content readability:
  - `min-sidebar-width`: 360 (compact JSON and short previews)
  - `max-sidebar-width`: 720 (~90 monospace columns)
  - `sidebar-width-fraction`: 0.4

### 2) Inspector rendering layer

- Migrate `ToolInspectorPane` to `Component` (from `SimpleComponent`) to support
  async DB loading via `CommandOutput`.
- Replace imperative widget building with a `gtk::Stack` holding one
  `Controller<T>` per renderer type.
- Renderer components live in `src/ui/tool_renderers/`:

```
src/ui/tool_renderers/
  mod.rs           // RendererKind, RendererInit, resolve_renderer()
  terminal.rs      // TerminalRenderer (SimpleComponent)
  diff.rs          // DiffRenderer (SimpleComponent)
  file.rs          // FileRenderer (SimpleComponent)
  results.rs       // ResultsRenderer (SimpleComponent)
  generic.rs       // GenericRenderer (SimpleComponent)
```

- All renderers share a common `RendererInit` struct containing: `tool_name`,
  `input_json`, `output_text`, `error_text`, `status`, `duration_ms`.
- `resolve_renderer(tool_name) -> RendererKind` selects the renderer; the
  inspector calls `stack.set_visible_child_name(kind.as_str())` to switch.

### 3) Inspector layout

The inspector pane layout for a tool call:

- **Header**: tool-type icon + tool name + status badge + duration.
- **Specialized view**: the active renderer's widget. This **replaces** the
  current raw input/output sections with structured, tool-aware content.
- **Raw JSON** (collapsed): `AdwExpanderRow` containing pretty-printed
  `input_json`, collapsed by default. Provides access to raw data for
  debugging and power users.
- **Metadata**: `parser_call_id` (if present), start/end timestamps.
- **Error section** (conditional): shown when `status == Error`, `error_text`
  is non-empty, or a non-zero exit signal is detected.

### 4) SubagentView — refactoring existing view

The current inspector already has a full subagent view (prompt, result summary,
inner tools list, "Open Full Session" button). `SubagentView` is a **refactoring
of this existing view** into a `SimpleComponent`, not a new creation. It is
integrated into the `gtk::Stack` alongside other renderers.

### 5) Drill-down navigation

The existing `AdwNavigationView` push/pop mechanism for inspecting a subagent's
inner tools is preserved:

- The overview page contains the `gtk::Stack` of renderers.
- When the user clicks an inner tool in `SubagentView`, a drill-down page is
  pushed onto the `AdwNavigationView`.
- The drill-down page reuses the same renderer selection logic
  (`resolve_renderer` + renderer components) to display the inner tool.
- The existing `connect_popped` callback and `drill_page_pushed` tracking
  remain for back-button synchronization.
- Drill-down state is isolated from overview state so reloads in one context do
  not accidentally overwrite content in the other.

### 6) Data loading

- `ToolInspectorPane::update()` no longer queries the DB synchronously on the
  GTK thread.
- On receiving `SelectToolCall` or `SelectSubagent`, the component spawns a
  `Command` (async task) that loads data from SQLite off the main thread.
- The command result arrives via `update_cmd()` as a `CommandOutput` variant,
  which updates the model and triggers `post_view()` to refresh widgets.
- This requires migrating `ToolInspectorPane` from `SimpleComponent` to
  `Component`.

Load-state contract:

- `Idle`: no active selection.
- `Loading`: selection is known, async load is in flight.
- `Ready`: data resolved and view can render normally.
- `LoadError`: data load failed; show contextual error and retry affordance.

Stale-result protection:

- Each async request carries a monotonically increasing `request_id`.
- `update_cmd()` applies output only if `request_id` equals current active
  request.
- Outdated responses are ignored, preventing race conditions during rapid
  selection changes.

### 7) Transcript chip enrichment flow

- Extend transcript row loading to include enough fields for preview extraction
  (at minimum `input_json`; optionally `output_text` for result count hints).
- Compute `preview: Option<String>` in UI mapping code
  (`transcript_item_init_from_row`), not in parsers.
- Keep backwards compatibility by falling back to existing `summary` when no
  preview can be extracted.

Chip preview extraction remains parser-light: parser changes are not required to
ship preview text because extraction happens in UI mapping.

## Renderer Registry Specification

| Tool pattern | Renderer | Icon | Primary source fields | Output style |
|---|---|---|---|---|
| `Bash`, `shell`, `exec_command` | TerminalRenderer | `utilities-terminal-symbolic` | `input_json.command`, `output_text`, `error_text` | Monospace terminal block + exit metadata |
| `Edit`, `apply_patch` | DiffRenderer | `document-edit-symbolic` | `input_json.old_string`, `input_json.new_string`, `input_json.file_path` | Unified diff with `+/-` line styling |
| `Read` | FileRenderer | `document-open-symbolic` | `input_json.file_path`, `input_json.offset/limit`, `output_text` | File-like text with contextual header |
| `Write` | FileRenderer | `document-open-symbolic` | `input_json.file_path`, written content fields | File-like text marked as written content |
| `Grep`, `Search`, `Glob` | ResultsRenderer | `system-search-symbolic` | `input_json.pattern`, `output_text` | Structured match/result list |
| `Agent`, `Task` | SubagentView | `system-run-symbolic` | Existing subagent fields | Refactored existing subagent inspector view |
| `*` | GenericRenderer | `application-x-addon-symbolic` | raw input/output/error | Pretty JSON + markdown/plain fallback |

Icons are used both in inline transcript chips and in the inspector header.

## Renderer Contract and Degradation Rules

Every renderer consumes the same `RendererInit` input and must tolerate missing
or malformed fields.

- Renderers must never panic on malformed payloads.
- Missing required renderer-specific fields triggers a deterministic downgrade.
- Final fallback is always `GenericRenderer`.

Deterministic degradation examples:

- `DiffRenderer` without valid `old_string/new_string` -> `FileRenderer` when
  file/text context exists, else `GenericRenderer`.
- `ResultsRenderer` without parseable query/matches -> `GenericRenderer`.
- `TerminalRenderer` without command text -> output-focused terminal view,
  else `GenericRenderer` if output is also unavailable.

All render paths apply bounded initial content rendering to avoid UI freezes
with very large outputs.

## Diff Rendering with `similar`

The `DiffRenderer` computes diffs from `input_json.old_string` and
`input_json.new_string` using the `similar` crate:

- Use `TextDiff::from_lines(old_string, new_string)` for line-by-line diff.
- Use `Algorithm::Patience` (better for code than default Myers; same as
  `git diff --patience`).
- Set `timeout(Duration::from_millis(500))` to protect against very large diffs.
- Use `grouped_ops(3)` to get hunks with 3 lines of context for custom
  rendering (not `unified_diff()` which produces flat text).
- Enable the `inline` cargo feature for `iter_inline_changes()` to support
  intra-line change highlighting in future iterations.

## Rendering Rules and Fallbacks

Rendering order:

1. Resolve renderer from `tool_name`.
2. Attempt specialized parse/render.
3. If specialized data is missing or invalid, downgrade to nearest safe view.
4. Final fallback is always `GenericRenderer`.

Generic fallback behavior:

- Input: parse JSON if possible, pretty-print with 2-space indent; otherwise
  render raw text.
- Output: if JSON-looking text parses, pretty-print; otherwise reuse
  `render_markdown_to_textview(output_text, None)`.
- Unknown tool names are displayed as-is.

## Inline Chip Preview Extraction

Define UI helper:

`extract_preview(tool_name: &str, input_json: &str, output_text: Option<&str>) -> Option<String>`

Preview rules:

- Bash/shell: `$ <first command segment>`
- Read: `<file_path>:<start>-<end>` when offsets are available
- Edit: `<file_path> +<added> -<removed>` from old/new content stats
- Grep: `pattern="..." -> N matches` when count can be inferred
- Agent/Task: `<description>` truncated
- Fallback: first meaningful short string field

Constraints:

- Maximum preview length: 60 characters. Truncate with ellipsis (`…`) when
  exceeded.
- Preview extraction is best-effort and must never fail rendering.
- File paths in previews show only the basename + parent directory when the
  full path exceeds the length limit.

## Error and Status Model

- Error styling is triggered by either:
  - normalized `status == Error`, or
  - non-empty `error_text`, or
  - detected non-zero process exit signal.
- Error section is shown even if normal output exists.
- Shell-like tools may show exit code badge in chips and metadata row.

Note: error detection depends on the parser prerequisites listed above.
Until the Claude Code `is_error` fix is applied, Claude Code tool errors
will not trigger error styling.

## CSS Specifications

New CSS classes required beyond the existing `.inspector-code-block` and
`.inspector-section-heading`:

| Class | Purpose | Key properties |
|---|---|---|
| `.diff-added` | Added lines in DiffRenderer | Green text or green-tinted background |
| `.diff-removed` | Removed lines in DiffRenderer | Red text or red-tinted background |
| `.diff-context` | Unchanged context lines | Default text, slightly dimmed |
| `.diff-hunk-header` | Hunk separator (`@@ ... @@`) | Monospace, dimmed, italic |
| `.terminal-output` | Terminal output in TerminalRenderer | Monospace, dark background |
| `.file-header` | File path header in FileRenderer/DiffRenderer | Monospace, bold, card background |
| `.preview-label` | Preview text on inline chips | Dimmed, ellipsized, smaller font |

Colors must work with both light and dark GTK themes. Use `@` color references
(e.g., `alpha(@success_color, 0.15)`) rather than hardcoded hex values.

## Performance and Data Safety

- Compute diffs lazily (only when an inspected item is rendered).
- Keep preview extraction lightweight and truncated.
- Protect UI against very large outputs via bounded initial render and optional
  expansion pattern.
- Do not mutate or reinterpret source data beyond view-level parsing.
- Load inspector data asynchronously via `CommandOutput` to avoid blocking the
  GTK main thread.

## Relm4 and GNOME Alignment

- Each renderer is a `SimpleComponent`; the inspector pane is a `Component`
  (for async command support).
- Keep utility-pane behavior aligned with GNOME HIG:
  - right-side subordinate pane
  - overlay behavior in constrained width
  - F9 shortcut for pane visibility
- Keep keyboard navigation and accessibility labels intact.
- Use `gtk::Stack` for renderer switching (idiomatic GTK pattern for
  mutually exclusive views).

## Testing and Verification Plan

1. Phase A verification (async safety)
   - stale-result protection (`request_id`) ignores outdated command outputs.
   - `Idle/Loading/Ready/LoadError` transitions are deterministic.
   - loading failures surface retry-friendly UI state.

2. Unit tests
   - `resolve_renderer` mappings and fallback.
   - `extract_preview` across Bash/Read/Edit/Grep/Agent plus malformed JSON,
     including truncation at 60 chars.
   - diff formatting helper for representative edit payloads using `similar`.

3. Integration tests
   - transcript mapping produces preview-rich tool chips.
   - inspector selects specialized renderer per tool and falls back safely.
   - error scenarios show error styling and sections correctly.
   - async data loading completes without blocking UI.
   - drill-down navigation preserves state isolation from overview.

4. Manual verification (fixtures)
   - Run app with `--sessions-dir tests/fixtures`.
   - Verify Bash/Edit/Read/Grep/unknown-tool displays.
   - Verify narrow-window overlay behavior and F9 toggle.
   - Verify drill-down navigation for subagent inner tools.
   - Rapidly switch selections while loading and confirm no stale flashback.

## Acceptance Criteria

- Chips provide meaningful contextual previews (max 60 chars) for common tool
  calls, with per-tool-type icons.
- Inspector output is specialized for core tool families and remains readable
  for unknown tools.
- No crashes or blank states on malformed/partial tool payloads.
- Inspector data loads asynchronously with explicit
  `Idle/Loading/Ready/LoadError` behavior.
- Stale async responses never overwrite a newer selection.
- Diff rendering uses the `similar` crate with Patience algorithm.
- No DB migration required.
- No GtkSourceView dependency required.

Release gating:

- Phase A and Phase B criteria must pass before declaring the feature complete.
- Phase C enhancements are optional for the first merge if all fallback and
  safety criteria are satisfied.

## Future Evolution (Non-blocking)

- Add intra-line change highlighting in DiffRenderer using
  `similar::iter_inline_changes()` (feature `inline`).
- Add optional syntax highlighting backend later if needed.
- Add optional secret-redaction pass before rendering sensitive outputs.

## References

- Exploration basis: `docs/plans/2026-03-05-tool-inspector-exploration.md`
- Tool format analysis: `docs/TOOL_CALLS_ANALYSIS.md`
- Relm4 Book (components/factory/commands):
  - https://raw.githubusercontent.com/Relm4/book/refs/heads/main/src/components.md
  - https://raw.githubusercontent.com/Relm4/book/refs/heads/main/src/efficient_ui/factory.md
  - https://raw.githubusercontent.com/Relm4/book/refs/heads/main/src/threads_and_async/commands.md
- GNOME HIG utility panes:
  - https://developer.gnome.org/hig/patterns/containers/utility-panes.html
- Libadwaita `AdwOverlaySplitView`:
  - https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1-latest/class.OverlaySplitView.html
- `similar` crate docs/context:
  - https://context7.com/mitsuhiko/similar/llms.txt
