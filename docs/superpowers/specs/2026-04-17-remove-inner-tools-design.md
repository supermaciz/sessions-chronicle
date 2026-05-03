# Remove Inner Tools From Subagent Inspector

**Date:** 2026-04-17  
**Status:** Draft

## Problem

The subagent detail view in the tool inspector always renders an `Inner Tools`
section, but the current parser and storage flow rarely populate it with useful
data. In practice the section is empty and adds noise to the inspector UI.

The implementation also carries dedicated loading and drill-down code whose only
purpose is to support that section. Keeping the empty UI and its supporting
plumbing increases maintenance cost without providing value.

## Goal

Remove the `Inner Tools` section from the subagent inspector and delete the code
that exists solely to load, render, and drill into those nested tool calls.

## Non-Goals

- Do not remove subagent inspection itself.
- Do not remove the `Open Full Session` button.
- Do not change parser behavior or session indexing.
- Do not remove app-level child-session navigation used by `Open Full Session`.

## Current Behavior

The subagent inspector in `src/ui/tool_inspector_pane.rs` currently includes:

- prompt and result sections for the selected subagent
- an `Inner Tools` header and `gtk::ListBox`
- async loading of tool calls associated with the selected subagent
- a drill-down navigation page for inspecting those nested tool calls

This path is wired through:

- `load_tool_calls_for_subagent(...)` in `src/database/mod.rs`
- `subagent_tools` state in `ToolInspectorPane`
- `DrillDownTool` and `PopDrillDown` messages
- `ToolInspectorPaneCmd::Subagent` carrying both subagent and inner-tool results
- `build_subagent_tool_row(...)` and drill-down page widgets

## Desired Behavior

When a subagent is selected in the inspector, the UI should show only:

- the subagent title
- the prompt section, when present
- the result section, when present
- the existing `Open Full Session` button, when `child_session_id` is present

No `Inner Tools` header or list should be rendered, and there should be no
navigation path for drilling into nested tools from the subagent view.

## Design

### UI changes

In `src/ui/tool_inspector_pane.rs`:

- remove `tools_list` from `SubagentDetailViews`
- remove the `Inner Tools` header and list from `build_subagent_detail_page(...)`
- stop rebuilding subagent tool rows during subagent rendering

The subagent detail page becomes a simple prompt/result view plus the existing
child-session action button.

### State and message changes

Delete the state and messages used only for `Inner Tools`:

- remove `subagent_tools` from `ToolInspectorPane`
- remove `drilled_tool` and `pending_drill_tool_id`
- remove `ToolInspectorPaneMsg::DrillDownTool`
- remove `ToolInspectorPaneMsg::PopDrillDown`
- remove `ToolInspectorPaneCmd::DrillTool`

Keep:

- `InspectorSelection::Subagent`
- `ToolInspectorPaneMsg::SelectSubagent`
- `ToolInspectorPaneMsg::OpenChildSession`
- `ToolInspectorPaneOutput::OpenChildSession`

### Loading changes

Simplify subagent loading so it fetches only the selected subagent record.

Specifically:

- remove `load_tool_calls_for_subagent(...)` from the subagent selection path
- simplify `ToolInspectorPaneCmd::Subagent` to carry only the subagent result
- simplify `apply_subagent_cmd(...)` accordingly
- remove logic that tolerated inner-tool loading failures while keeping the main
  subagent load successful

### Drill-down removal

Remove the dedicated drill-down UI path in `ToolInspectorPane` because it is no
longer reachable after the `Inner Tools` list is deleted.

This includes:

- drill-down widget fields in the component model
- `build_drilldown_views(...)`
- navigation-view push/pop synchronization for the drill-down page
- helper functions that build or apply nested tool detail rows

The inspector still uses its normal stack pages for tool calls, subagents, and
reasoning attachments.

### Database helper cleanup

If `load_tool_calls_for_subagent(...)` has no remaining callers after the UI
removal, delete it from `src/database/mod.rs`.

No schema or migration changes are required.

## Testing Plan

Follow TDD for the behavior change.

1. Add or update a focused test that reflects the simplified subagent inspector
   structure and no longer expects the `Inner Tools` path.
2. Run the targeted test first and confirm it fails for the expected reason.
3. Remove the production code and make the test pass.
4. Run the relevant targeted test set for `tool_inspector_pane` and any affected
   database helper tests.
5. Run broader verification once implementation is complete.

## Verification

At minimum, the implementation phase should verify:

- subagent inspection still opens and renders prompt/result content correctly
- `Open Full Session` still appears only when `child_session_id` is present
- selecting normal tool calls still works
- the project compiles cleanly after removing drill-down-specific code

Expected commands during implementation verification:

```bash
cargo test --all --no-fail-fast tool_inspector_pane
cargo test --all --no-fail-fast
```

Additional `cargo fmt` and `cargo clippy` runs should be included before the
work is considered complete.

## Risks

- The drill-down navigation code is interwoven with the main inspector widget
  assembly, so removal must be careful to avoid breaking normal tool-call
  inspection.
- Some tests may implicitly depend on the old command/state shape and will need
  small updates even though behavior is being simplified.

## Rollout Notes

This is a UI simplification and dead-code removal only. No persisted data
changes are involved, and no compatibility handling is needed.
