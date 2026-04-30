# Session List Search - Explicit `id:` Session ID Filter

**Date:** 2026-04-30  
**Status:** Accepted

## Problem

`SessionDetail` exposes a session ID in the header, but the global search entry currently has only text-search semantics. A user who has a concrete session ID from a note, script output, or previous reference cannot use that ID directly to narrow the session list.

Session IDs differ across AI assistants, so implicit format detection would be brittle. The search UI needs an explicit, low-risk way to say "treat this input as a session ID", while preserving the current search behavior for all other queries.

## Scope

In scope:
- Support `id:<session_id>` in `SessionList` as an explicit exact-match filter on `sessions.id`.
- Keep active list filters in effect: AI assistant, project filter, and `Pinned`.
- Match exactly after trimming the value after `id:`.
- Show a dedicated empty state when an ID lookup returns no result.
- Leave ordinary non-prefixed search behavior unchanged.

Out of scope:
- Heuristic session ID detection without a prefix.
- Partial, fuzzy, or prefix matching on session IDs.
- Any navigation-specific behavior such as automatically opening a detail view.
- Any special support for `id:` in `SessionDetail`.
- Toast notifications for successful matches.

## Decisions

The brainstorming dialogue resolved these rules explicitly:

1. **`id:` is a SessionList-only filter mode.** In the list view it narrows the list to the matching session; outside the list view there is no session-ID behavior.
2. **Current filters still apply.** `id:<session_id>` does not bypass assistant, project, or pinned filters.
3. **Matching is exact after trim.** `id: abc ` matches only `abc`.
4. **Unknown or empty ID values produce a dedicated empty state.** The UI should say `No session found with this ID` rather than reusing the generic search-empty message.
5. **Only lowercase `id:` is recognized in v1.** This keeps the rule explicit and minimal.
6. **The ID empty state takes precedence over other search-empty variants.** For example, `id:abc` with `Pinned` selected should still show the session-ID-specific empty state when no pinned session matches.

## Architecture

The change stays localized to the existing session-list search flow.

- **UI layer (`src/ui/session_list.rs`)** detects whether the trimmed query begins with `id:` and chooses the correct loading path.
- **Database layer (`src/database/mod.rs`)** adds a dedicated exact-match query on `sessions.id` that reuses the existing filter semantics.
- **App-level search plumbing** remains unchanged. The shared search entry still emits the same messages to list and detail; only `SessionList` interprets the `id:` prefix specially.

No new modules, no schema migration, and no search-state redesign are required.

## Data flow

```text
User types into shared search entry
   |
   v
AppMsg::SearchQueryChanged(query)
   |
   +--> SessionListMsg::SetSearchQuery(query)
   |
   +--> SessionDetailMsg::UpdateSearchQuery(detail_query)

SessionList reloads:
   - empty query -> load_sessions_for_filter(...)
   - query starts with `id:` -> exact session-id lookup with active filters
   - otherwise -> search_sessions_for_filter(...) using FTS
```

`SessionDetail` continues to receive the shared query exactly as it does today. This design does not add a cross-component mode flag. The `id:` behavior is purely a local interpretation in `SessionList`; in `SessionDetail`, an input such as `id:abc` intentionally remains just an ordinary transcript query string in v1.

## Database layer

Add a dedicated query function that follows the same filtering rules as the current list loaders, for example:

```rust
pub fn load_session_by_id_for_filter(
    db_path: &Path,
    tools: &[AiAssistant],
    project_filter: &ProjectFilter,
    session_id: &str,
) -> Result<Vec<Session>>;
```

It should return `Vec<Session>` for compatibility with the current `SessionList::fetch_sessions` pipeline, even though the practical cardinality is `0` or `1`.

Implementation requirements:
- Query `sessions` directly, not `messages_fts`.
- Apply `WHERE id = ?` together with the existing `is_subagent = 0` condition.
- Apply the same assistant filter behavior already used by `load_sessions_for_filter` and `search_sessions_for_filter`.
- Apply the same project / pinned filtering already used by the list.
- Order by `last_updated DESC` for consistency, even though exact ID lookup should only return one visible session.

No migration is needed because `sessions.id` already exists and is the canonical session identifier.

## UI layer

Add one small local parser helper in `session_list.rs`, used by both loading and empty-state decisions:

```rust
fn parse_session_id_query(query: &str) -> Option<&str>;
```

Expected behavior:
- Trim the whole query before checking the prefix.
- Recognize only lowercase `id:`.
- Trim the suffix before returning it.
- Return `Some("")` for `id:` or `id:   ` so the UI can still identify the query as an explicit session-ID lookup and show the dedicated empty state.
- Return `None` for all non-prefixed queries.

`SessionList::fetch_sessions` then becomes a 3-way branch:

1. Empty trimmed query: load the normal filtered list.
2. Query with lowercase `id:` prefix: extract the suffix, trim it, and run the exact ID lookup.
3. Any other non-empty query: keep the current FTS search path.

No global search-mode enum is needed.

`compute_empty_state(...)` should use the same parser helper to distinguish this explicit lookup mode from ordinary text search. When the active query is an `id:` lookup and the resulting list is empty, the empty state should be:

- Title: `No session found with this ID`
- Description: `Try a different session ID or adjust filters`

This rule also applies when the user enters `id:` with no usable suffix after trimming.

This check should run before the existing `has_search && pinned_selected` and generic `has_search` empty-state branches. The explicit ID lookup copy is more specific than the pinned-search copy.

## Error handling

- Missing database file or query errors continue to follow the current list behavior: log the error and return an empty list.
- `id:` with an empty or whitespace-only suffix is not a validation error. It is treated as a lookup with no match and shows the dedicated empty state.
- `SessionDetail` gets no special-case suppression or rerouting. `id:abc` remains an ordinary transcript query string there. Any future behavior that disables, annotates, or reroutes `id:` in detail view would require broader global-search semantics and is explicitly deferred.

## Testing

Database tests in `tests/search_sessions.rs`:
- exact lookup returns the expected session when filters allow it
- exact lookup respects AI assistant filters
- exact lookup respects project filters
- exact lookup respects pinned filter
- exact lookup returns no rows for unknown IDs

`SessionList` tests in `src/ui/session_list.rs`:
- `id:` queries use the dedicated empty state copy
- `id:` queries use the dedicated empty state even when `Pinned` is selected
- ordinary text queries keep the current generic search empty state
- `id: abc ` is normalized to `abc`
- successful `id:` lookup reloads the list with the expected filtered session
- `id:` with no suffix yields the dedicated empty state
- `id:` with no suffix does not need a database-specific test if the local parser and list behavior already cover it

Regression expectation:
- non-prefixed search continues to use the current FTS behavior with no ranking or sanitization changes.

## Implementation notes

- Keep the change minimal: prefer one new DB helper and a small amount of branching in `SessionList`.
- Do not introduce partial ID matching, fallback heuristics, or new shared search state in this iteration.
- Reuse existing list-selection preservation logic during reload so exact-ID filtering behaves like any other list refresh.
