# Simplify PR #7: Remove Lazy Loading

## Context

PR #7 adds OpenCode support and temporarily hides tool messages. It also introduced lazy loading for Session Detail, but this feature is causing crashes.

Decision: Ship OpenCode support + tool filtering, remove the broken lazy loading.

## What We Remove

### `src/ui/session_detail.rs`

- `full_content_by_index: HashMap<usize, String>` — content cache
- `SessionDetailMsg::LoadFullContent { message_index }` — message variant
- Handler in `update()` (lines 297-322)
- "Show full" button in `build_message_widget()` (lines 479-496)
- `full_content_by_index` parameter from `build_message_widget()`
- All `.clear()` calls on the removed HashMap

### `src/database/mod.rs`

- `load_message_content_for_session_index` function — delete completely
- Remove from exports

## What We Keep

### OpenCode Support (core of the PR)

- `src/parsers/opencode.rs` — new parser
- Fixtures in `tests/fixtures/opencode_storage/`
- Tests in `tests/opencode_search.rs`

### Tool Message Filtering

- Claude Code: ignore `system.local_command` events and `tool_use` blocks
- OpenCode: skip parts where `kind == "tool"`
- Tests updated to verify tool output is no longer searchable

### Preview for Performance

- `MessagePreview` model
- `load_message_previews_for_session` with `preview_len: 2000`
- Message pagination (`page_size`, `LoadMore`)

## Implementation Steps

1. Edit `src/ui/session_detail.rs`:
   - Remove `full_content_by_index` from struct
   - Remove `SessionDetailMsg::LoadFullContent`
   - Remove handler in `update()`
   - Simplify `build_message_widget()` — remove parameter and button
   - Clean up `.clear()` calls

2. Edit `src/database/mod.rs`:
   - Remove `load_message_content_for_session_index` from export
   - Delete the function

3. Verify tests:
   - Check `tests/message_preview.rs` doesn't use `LoadFullContent`
   - Run `cargo test`

## Future Work

- Fix lazy loading crash (separate PR)
- Add expand button for all truncated messages (not just ToolResult)
- Tool message summary view: "🔧 Bash: cargo test --lib" instead of hiding completely
