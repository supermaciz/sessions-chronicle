# OpenCode V2 synthetic fixture

Synthetic, source-derived SQLite fixture for the native OpenCode V2 reader
(`crates/core/src/parsers/opencode/v2.rs`). No personal transcript content is
present; every prompt, skill body and summary is invented for the test suite.

## Provenance

Pinned upstream commit: **`0c6be8694894dc96b87c5a7b773d52e57ddaa1b2`**
(`anomalyco/opencode`, mirrored from `sst/opencode`). Inspected on 2026-10-21.

Source paths verified against that commit:

| Upstream path | What it pins |
| --- | --- |
| `packages/core/src/session/sql.ts` | `session_v2` and `session_message` tables, column names and nullability |
| `packages/core/src/database/schema.sql.ts` | `Timestamps` = `time_created` / `time_updated` integer millis |
| `packages/schema/src/session-message.ts` | The `SessionMessage.Info` union and the exact `data` payload shape per message `type` |
| `packages/schema/src/session-inbox.ts` | `UserPayload`/`SyntheticPayload`/`CompactionPayload` (inbox promotion shape) |
| `packages/schema/src/prompt.ts` | User `skills[]` entries have `id`, `name`, optional `text`, optional `mention` |
| `packages/schema/src/session-error.ts` | Structured error `{ type, message, status?, response?, retryAfterMs? }` |
| `packages/schema/src/token-usage.ts` | `TokenUsage.Info` = `{ input, output, reasoning, cache: { read, write } }` |
| `packages/schema/src/model.ts` | `Model.Ref` = `{ id, providerID, variant? }` (assistant `model.id` is read) |
| `packages/schema/src/tool.ts` | Tool result blocks are `{ type: "text", text }` or `{ type: "file", uri, mime, name? }` |
| `packages/core/src/session/subagent-completion.ts` | Synthetic completion `metadata = { source: "subagent", childID, agent, state }` and the `<subagent …>` `text` payload |

The message `data` column stores `SessionMessageData` — the encoded
`SessionMessage.Info` with the `type` and `id` fields lifted into the
`type` / `id` table columns. `time.created` inside `data` is an epoch-millis
number (`DateTimeUtcFromMillis`), matching the `time_created` column type.

## Selected persisted columns

The reader selects only the columns it consumes, so the fixture persists a
faithful subset of the upstream schema:

- `session_v2`: `id`, `project_id`, `directory`, `title`, `parent_id`,
  `version`, `slug`, `model`, `tokens_input`, `tokens_output`,
  `tokens_reasoning`, `tokens_cache_read`, `tokens_cache_write`,
  `time_created`, `time_updated`.
  - The reader selects only `id, directory, title, parent_id, time_created,
    time_updated`.
- `session_message`: `id`, `session_id`, `type`, `seq`, `time_created`,
  `time_updated`, `data` plus `UNIQUE (session_id, seq)`.
  - The reader selects only `id, seq, type, time_created, data`.

## Deliberate omissions

Not persisted here (present upstream but unused by this reader): `workspace_id`,
`fork_session_id`, `fork_boundary`, `path`, `share_url`, `summary_additions`,
`summary_deletions`, `summary_files`, `summary_diffs`, `metadata`, `cost`,
`revert`, `permission`, `agent`, `time_idle`, `time_viewed`, `idle_outcome`,
`time_compacting`, `time_archived`, `time_suspended`, `resume_attempts`, and
`session_pending` / `session_inbox` / instruction tables.

The `session_v2.model` and `tokens_*` columns are persisted only to prove the
reader ignores them: session rollup tokens (`999`) must not be added to
assistant usage, and a missing per-message assistant `model` must not inherit
the session model.

The reader selects only consumed columns, so a source database missing any of
the omitted columns still opens and parses successfully.

## Sessions

- `ses_v2` — the main fixture. `seq` order, SQL insertion order, and
  `time_created` values deliberately disagree, proving `ORDER BY seq, id` wins
  over both. Contains: system, genuine user `"real prompt"`, assistant content
  `[text, reasoning, text]`, a user-attached skill (one with text, one
  without), a standalone skill (same name/text as the attached one, a different
  source message), a completed compaction, running and failed compactions, an
  unknown message type (`idle`) and an unknown assistant content type.
  Title is blank whitespace, so `first_prompt` falls back to `"real prompt"`.
- `ses_v2_system_only` — a system message only; must be `NoUserMessages`.
- `ses_v2_malformed` — a malformed `data` row followed by valid user/content.
- `ses_v2_corrupt_user` — a user row whose `data` is not JSON; must be
  `IncompleteTranscript`, not `NoUserMessages`.
- `ses_v2_malformed_then_valid` — a corrupt user row followed by a valid user.
- `ses_v2_null_title` — a NULL title falling back to its first prompt.
- `ses_v2_tools` — tool-call lifecycle in one assistant message: `streaming`
  (raw string input `{"path":`), `running` (object input + metadata),
  `completed` (text/file/text result blocks), `error` (structured `message`
  plus partial text), an unknown status, and a reversed `ran`/`completed`
  range (invalid duration). Proves `\n`-joined result text, file bodies
  skipped, structured error message, and duration only from valid content
  times.
- `ses_v2_skill_tool` — an ordinary `skill` tool call next to a user-attached
  skill injection: the call stays a tool call, the injection stays context.
- `ses_v2_task_done` — a `task` tool with no result content plus a synthetic
  completion. The completion fills the absent result; its prose deliberately
  names a different `sessionID` than `metadata.childID` to prove the metadata
  is authoritative.
- `ses_v2_completion_only` — a completion with no matching task, yielding a
  standalone subagent row keyed by the native message id (`msg_completion`).
- `ses_v2_child_missing` — a completion whose `childID` has no `session_v2`
  row; the native child ID is preserved unindexed.
- `ses_v2_resumed` — two task invocations resuming the same child, each with
  its own completion; explicit task results survive and no `task` tool call is
  emitted.
- `ses_v2_completion_before_task` — a completion that precedes its task in
  `seq` order; correlation looks backwards only.
- `ses_v2_ambiguous` — two task invocations for the same child sharing one
  message `seq`; the completion cannot be matched uniquely and stays
  standalone.
- `ses_v2_task_id_link` — a task carrying only `input.task_id` (a resume
  request) and no `state.metadata.sessionId`; `task_id` alone establishes no
  child link, so a completion naming that id stays standalone.

## Reproducible database creation

The fixture is shipped as two reviewed SQL files so it can be audited as text.
A disposable runnable database is built from them with the SQLite CLI (or any
SQLite client):

```bash
# From the repository root.
mkdir -p /tmp/opencode-v2-run/opencode_storage
sqlite3 /tmp/opencode-v2-run/opencode_storage/opencode.db \
  < tests/fixtures/opencode_v2/schema.sql
sqlite3 /tmp/opencode-v2-run/opencode_storage/opencode.db \
  < tests/fixtures/opencode_v2/sessions.sql
```

That layout follows the repository's `--sessions-dir` fixture override
convention (an `opencode_storage/` subdirectory holding `opencode.db`), so the
built application can be run against it:

```bash
flatpak-builder --run flatpak_app \
  build-aux/dev.maciz.sessionschronicle.Devel.json sessions-chronicle \
  --sessions-dir /tmp/opencode-v2-run
```

The acceptance tests in `tests/opencode_v2_integration.rs` build this same
database in-process (see the `create_source` helper) and drive the public
indexing, search, and transcript-reading paths headlessly; no personal
transcript content is involved.
