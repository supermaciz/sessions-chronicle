# Tool Inspector Design (Issue #46)

## Problem

The current Tool Inspector shows raw text blocks for `input_json`, `output_text`,
and `error_text`, without tool-aware rendering. Inline transcript chips only show
name/status/duration, so users do not get enough context before opening the pane.

The exploration document selected Proposal B (specialized per-tool rendering) as
the best direction.

## Decision

Adopt **Proposal B (Specialized Views)** with a **hybrid architecture**:

1. Keep existing app/container structure (`AdwOverlaySplitView`, right-side
   utility pane, F9 toggle).
2. Add a renderer registry and specialized views in the inspector.
3. Keep implementation lightweight by starting with renderer modules/functions,
   while defining clear boundaries so we can migrate to child components later.
4. Do not add GtkSourceView in this phase.

## Scope

In scope:

- Tool-aware inspector rendering for common tool families.
- Contextual inline chip previews derived from tool input/output.
- Responsive pane width tuning for code-heavy content.
- Stable generic fallback for unknown/custom/MCP tools.

Out of scope:

- DB schema changes.
- Full parser redesign.
- Full markdown feature parity expansion beyond current renderer.
- New heavy rendering dependency (GtkSourceView).

## Constraints

- Respect current Relm4 architecture and existing `SimpleComponent` boundaries.
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
- Tune pane width policies (`min/max/fraction`) for better readability of diff
  and terminal outputs.

### 2) Inspector rendering layer

- Keep `ToolInspectorPane` in `src/ui/tool_inspector_pane.rs` as the orchestration
  component.
- Add an internal rendering layer (new module, e.g. `src/ui/tool_rendering.rs`):
  - `RendererKind`
  - `resolve_renderer(tool_name: &str) -> RendererKind`
  - per-renderer widget builders (`render_terminal_view`, `render_diff_view`,
    `render_file_view`, `render_results_view`, `render_generic_view`)
  - helper parsers/formatters for previews and metadata
- Inspector layout becomes:
  - Header (icon + tool name + compact context)
  - Specialized view area
  - Collapsible input JSON section
  - Metadata section
  - Error section (conditional)

### 3) Transcript chip enrichment flow

- Extend transcript row loading to include enough fields for preview extraction
  (at minimum `input_json`; optionally `output_text` for result count hints).
- Compute `preview: Option<String>` in UI mapping code
  (`transcript_item_init_from_row`), not in parsers.
- Keep backwards compatibility by falling back to existing `summary` when no
  preview can be extracted.

This is a parser-light-compatible design: parser changes are optional and not
required for the initial design outcome.

## Renderer Registry Specification

| Tool pattern | Renderer | Primary source fields | Output style |
|---|---|---|---|
| `Bash`, `shell`, `exec_command` | TerminalView | `input_json.command`, `output_text`, `error_text` | Monospace terminal block + exit metadata |
| `Edit`, `apply_patch` | DiffView | `input_json.old_string`, `input_json.new_string`, `input_json.file_path` | Unified diff with `+/-` line styling |
| `Read` | FileView | `input_json.file_path`, `input_json.offset/limit`, `output_text` | File-like text with contextual header |
| `Write` | FileView | `input_json.file_path`, written content fields | File-like text marked as written content |
| `Grep`, `Search`, `Glob` | ResultsView | `input_json.pattern`, `output_text` | Structured match/result list |
| `Agent`, `Task` | SubagentView | Existing subagent fields | Existing subagent inspector view |
| `*` | GenericJsonView | raw input/output/error | Pretty JSON + markdown/plain fallback |

## Rendering Rules and Fallbacks

Rendering order:

1. Resolve renderer from `tool_name`.
2. Attempt specialized parse/render.
3. If specialized data is missing or invalid, downgrade to nearest safe view.
4. Final fallback is always `GenericJsonView`.

Generic fallback behavior:

- Input: parse JSON if possible, pretty-print with 2-space indent; otherwise
  render raw text.
- Output: if JSON-looking text parses, pretty-print; otherwise reuse
  `render_markdown_to_textview(output_text, None)`.
- Unknown tool names are displayed as-is; MCP names can be formatted for
  readability (`server > tool`) without changing stored values.

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

Preview extraction is best-effort and must never fail rendering.

## Error and Status Model

- Error styling is triggered by either:
  - normalized `status == Error`, or
  - non-empty `error_text`, or
  - detected non-zero process exit signal.
- Error section is shown even if normal output exists.
- Shell-like tools may show exit code badge in chips and metadata row.

## Performance and Data Safety

- Compute diffs lazily (only when an inspected item is rendered).
- Keep preview extraction lightweight and truncated.
- Protect UI against very large outputs via bounded initial render and optional
  expansion pattern.
- Do not mutate or reinterpret source data beyond view-level parsing.

## Relm4 and GNOME Alignment

- Use existing Relm4 component boundaries; avoid unnecessary controller churn.
- Keep utility-pane behavior aligned with GNOME HIG:
  - right-side subordinate pane
  - overlay behavior in constrained width
  - F9 shortcut for pane visibility
- Keep keyboard navigation and accessibility labels intact.

## Testing and Verification Plan

1. Unit tests
   - `resolve_renderer` mappings and fallback.
   - `extract_preview` across Bash/Read/Edit/Grep/Agent plus malformed JSON.
   - diff formatting helper for representative edit payloads.

2. Integration tests
   - transcript mapping produces preview-rich tool chips.
   - inspector selects specialized renderer per tool and falls back safely.
   - error scenarios show error styling and sections correctly.

3. Manual verification (fixtures)
   - Run app with `--sessions-dir tests/fixtures`.
   - Verify Bash/Edit/Read/Grep/unknown-tool displays.
   - Verify narrow-window overlay behavior and F9 toggle.

## Acceptance Criteria

- Chips provide meaningful contextual previews for common tool calls.
- Inspector output is specialized for core tool families and remains readable for
  unknown tools.
- No crashes or blank states on malformed/partial tool payloads.
- No DB migration required.
- No GtkSourceView dependency required.

## Future Evolution (Non-blocking)

- Promote renderer modules into dedicated child components if complexity grows.
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
