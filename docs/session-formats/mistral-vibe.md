# Mistral Vibe — Session Format Reference

Format reference for Mistral Vibe session files.
See [SESSION_FORMAT_ANALYSIS.md](../SESSION_FORMAT_ANALYSIS.md) for cross-assistant comparison tables.

---

## Storage & File Naming

| Field   | Value |
|---------|-------|
| **Path** | `~/.vibe/logs/session/session_YYYYMMDD_HHMMSS_<shortid>/` |
| **Pattern** | `session_YYYYMMDD_HHMMSS_<shortid>/` (directory) |
| **Example** | `session_20260123_174305_64883c86/` |
| **Format** | Directory-based: `meta.json` + `messages.jsonl` |

The default path can be overridden via:
- `VIBE_HOME` environment variable
- `session_logging.save_dir` in `config.toml`

**Path encoding:**

```
~/.vibe/logs/session/session_20260123_174305_64883c86/
                    └──────────────┬──────────────┘
                       timestamp + session id prefix
```

---

## Session Directory Structure

```
session_YYYYMMDD_HHMMSS_<shortid>/
├── meta.json        # Session-level metadata, timestamps, config snapshot
├── messages.jsonl   # OpenAI-style chat transcript (one message per line)
└── agents/          # Present only when the session spawned sub-agents
    └── <agent>_YYYYMMDD_HHMMSS_<shortid>/
        ├── meta.json
        └── messages.jsonl
```

Each spawned sub-agent (via the `task` tool) is logged as its **own**
self-contained session directory under the parent's `agents/` folder, named
with the agent profile as prefix (for example `comique_20260609_121044_57ffdbcd/`).
The parent/child relationship is encoded by this directory layout, **not** by a
metadata field. See [Threading](#threading).

---

## `meta.json` Format

Session-level metadata:

| Field | Description |
|-------|-------------|
| `session_id` | UUID |
| `parent_session_id` | Present but **always `null`** in observed sessions (including sub-agent children); parent linkage is directory-based instead — see [Threading](#threading) |
| `start_time` | ISO-8601 string |
| `end_time` | ISO-8601 string |
| `environment.working_directory` | Working directory |
| `git_commit` | Optional git commit hash |
| `git_branch` | Optional git branch name |
| `username` | Username of the session owner |
| `title` | First ~50 chars of the first user message |
| `title_source` | How the title was set: `auto` or `manual` |
| `total_messages` | Count of all non-system messages |
| `stats` | Token usage, tool call counters, other session metrics |
| `tools_available` | Set of tools available to the agent for the session |
| `config` | Optional config snapshot: `active_model`, `providers`, `models` arrays |
| `agent_profile` | Optional selected profile/override metadata. On sub-agent child sessions, `agent_profile.name` holds the agent name (e.g. `"comique"`) used to pair the child with the parent's `task` call — see [Threading](#threading) |
| `system_prompt` | System message object (`{"role": "system", "content": "..."}`) — moved here from `messages.jsonl` |
| `loops` | Optional; agent loop metadata (added conditionally) |
| `experiments` | Optional; experiment flags (added conditionally) |

Upstream type: `SessionMetadata` in `vibe/core/types.py`.

**Model metadata** is session-level via `meta.json.config` snapshot when present
(`config.active_model`, plus `config.providers`/`config.models`).
Requires session logging metadata output — minimal/older logs may omit full config snapshot.

### Token Usage / Stats (Optional)

`meta.json.stats` is optional and can be `null` in minimal/older logs or when configured without stats.
When present, it provides **session-level token totals** plus **last-turn metrics**.

Upstream type: `AgentStats` in `vibe/core/types.py`.

Token totals:

- `session_prompt_tokens`, `session_completion_tokens`
- `session_total_llm_tokens` (computed)
- `context_tokens`, `last_turn_prompt_tokens`, `last_turn_completion_tokens`, `last_turn_total_tokens` (computed)

Tool-call counters (per session):

- `tool_calls_agreed`, `tool_calls_rejected`, `tool_calls_failed`, `tool_calls_succeeded`

Performance metrics:

- `steps`, `tokens_per_second`, `last_turn_duration`

Pricing:

- `input_price_per_million`, `output_price_per_million`
- `session_cost` (computed)

Example (abridged):

```json
{
  "stats": {
    "session_prompt_tokens": 115968,
    "session_completion_tokens": 262,
    "session_total_llm_tokens": 116230,
    "last_turn_prompt_tokens": 10222,
    "last_turn_completion_tokens": 41
  }
}
```

Current observed limitation:

- No separate cache-token counters were observed in `meta.json.stats`.
- No separate reasoning-token counter was observed in `meta.json.stats`; current logs expose aggregate prompt/completion totals only.
- Reasoning content itself is available per-message via `reasoning_content` on assistant messages (see above), but is not reflected as a separate token counter in `stats`.
- Cost is exposed as aggregate `session_cost` plus per-million unit pricing (`input_price_per_million`, `output_price_per_million`); there is no per-turn cost breakdown.

---

## `messages.jsonl` Format

OpenAI-style chat transcript. One JSON object per line.

Messages do not include a normalized model identifier field.

### User / Assistant Messages

```json
{ "role": "user", "content": "Help me refactor this function" }
{ "role": "assistant", "content": "Sure, here's how we can refactor it..." }
```

**System messages are not written to `messages.jsonl`.** They are filtered out by the session logger and stored in `meta.json` under the `system_prompt` key.

**Optional message fields** (nullable, excluded from serialization when absent):

| Field | Present on | Notes |
|-------|-----------|-------|
| `message_id` | `user`, `assistant` | Auto-generated UUID per message; absent on `tool` role |
| `images` | `user`, `assistant` | Attached image content, when present |
| `injected` | any | Marks a message injected by the runtime (e.g. compaction summaries) rather than typed by the user |
| `reasoning_content` | `assistant` | Thinking/reasoning block content (reasoning-capable models) |
| `reasoning_signature` | `assistant` | Signature for reasoning blocks |
| `reasoning_state` | `assistant` | State of the reasoning block (accumulates alongside `reasoning_content`) |
| `reasoning_message_id` | `assistant` | Auto-generated UUID for the reasoning block when reasoning is present |

Upstream type: `LLMMessage` in `vibe/core/types.py`.

### Assistant Message with Tool Calls

```json
{
  "role": "assistant",
  "tool_calls": [
    {
      "id": "abc123",
      "index": 0,
      "type": "function",
      "function": {
        "name": "bash",
        "arguments": "{\"command\":\"ls -la\"}"
      }
    }
  ]
}
```

### Tool Result Message

```json
{
  "role": "tool",
  "name": "bash",
  "tool_call_id": "abc123",
  "content": "stdout: ...\n\nstderr: ...\nreturncode: 0"
}
```

Tool call correlation: `tool_calls[*].id` → `tool_call_id`.
Arguments are stored as JSON-encoded strings (`tool_calls[*].function.arguments`).

> **Note (Vibe ≥ 2.14.0):** the changelog reports a new tool-call format for the
> `read` tool and the file `edit` tool, and `write_file` is now create-only. This
> affects the JSON shape inside `tool_calls[*].function.arguments`. The Sessions
> Chronicle parser stores arguments as an opaque string, so indexing is
> unaffected; the concrete argument schema has not yet been captured from a real
> session.

---

## Threading

Linear message list in `messages.jsonl`.
Tool calls are embedded in assistant messages and resolved by subsequent `tool` role messages.

### Sub-agents

A session delegates work to a sub-agent through the **`task` tool call**, whose
JSON arguments carry the agent name and the delegated prompt:

```json
{
  "role": "assistant",
  "tool_calls": [
    {
      "id": "YuV7lzFC6",
      "type": "function",
      "function": {
        "name": "task",
        "arguments": "{\"task\": \"Review the README\", \"agent\": \"comique\"}"
      }
    }
  ]
}
```

The sub-agent's final response comes back as the matching `tool` result
(`tool_call_id == "YuV7lzFC6"`), while its **full transcript** is logged as a
separate child session directory under `<parent>/agents/<agent>_*/`.

Linkage is entirely **directory- and name-based**; `meta.json.parent_session_id`
is `null` on both sides and carries no linkage:

- The child knows its parent because it lives under `<parent>/agents/`.
- A parent `task` call is paired to a child by matching the call's `agent`
  argument against the child's `agent_profile.name`, in chronological order when
  the same agent is invoked more than once.

No inline sub-agent transcript model exists within a single `messages.jsonl`;
the child transcript is always a distinct session file.

---

## Skills / Slash Commands

Mistral Vibe skills are discovered from skill directories and surfaced to the
AI assistant through the system prompt. In sampled logs, skill loading does
**not** use a dedicated native skill event, but two distinct persisted patterns
were observed.

### Upstream Behavior

Primary source findings:

- `vibe/core/system_prompt.py` injects an `<available_skills>` section with
  skill `name`, `description`, and `path`, plus the instruction to read the
  full `SKILL.md` when a task matches a skill.
- `vibe/cli/textual_ui/app.py` exposes `/<skill-name>` completions for
  `user_invocable` skills.
- That same handler only matches the **exact** skill name after the slash:
  - `/<skill-name>` is intercepted client-side and replaced with the full
    `SKILL.md` body as the outgoing user message
  - if the user types additional arguments (for example `/learn-rust path B`),
    the input falls through as a normal user message instead of being
    intercepted as a dedicated slash command action
- `vibe/core/session/session_logger.py` persists only generic `LLMMessage`
  entries and ordinary `tool_calls`.

### Observed Local Session Pattern

Observed on session `5ef4776f-7545-4e13-a25d-0ce2eb58a0ac`
(`session_20260311_113459_5ef4776f/`) for the exact slash-command path:

```json
{"role":"user","content":"---\nname: learn-rust\ndescription: ..."}
{"role":"assistant","tool_calls":[
  {"function":{"name":"read_file","arguments":"{\"path\": \"skills/learn-rust/PATHS.md\"}"}}
]}
```

Observed on session `b6999d83-6ddd-48ec-9766-fb12395b1158`
(`session_20260304_125746_b6999d83/`) for the slash-with-arguments path:

```json
{"role":"user","content":"/learn-rust path B (en français)"}
{"role":"assistant","tool_calls":[
  {"function":{"name":"read_file","arguments":"{\"path\": \"skills/learn-rust/SKILL.md\"}"}}
]}
{"role":"tool","name":"read_file","content":"path: .../skills/learn-rust/SKILL.md\ncontent: ---\nname: learn-rust\n..."}
{"role":"assistant","tool_calls":[
  {"function":{"name":"read_file","arguments":"{\"path\": \"skills/learn-rust/PATHS.md\"}"}}
]}
```

### Recommended Detection Heuristic

Best current markers of a **loaded** Mistral Vibe skill:

- Exact `/<skill-name>` path:
  - persisted `role == "user"` message is the full `SKILL.md` body
  - the session title is often derived from that injected content
- Free-form / slash-with-arguments path:
  - assistant `tool_calls[*].function.name == "read_file"`
  - and `tool_calls[*].function.arguments` points to `skills/<skill-name>/SKILL.md`

Useful secondary evidence:

- subsequent reads from the same skill directory (for example `PATHS.md`)
- nearby user message starting with `/<skill-name>`
- `role == "tool"` result containing the resolved `SKILL.md` path and body

What not to rely on:

- the leading slash command alone, unless it resolves to the injected
  `SKILL.md` body in the persisted `role == "user"` message
- `meta.json.tools_available`
- any dedicated native skill `tool call` marker, because none was observed in
  sampled local logs or in the upstream session schema

---

## Input History (Not a Session Log)

`~/.vibe/vibehistory` stores a JSONL list of user inputs for prompt recall.
It does **not** contain the full assistant/tool transcript.

---

## Parser Behavior (Sessions Chronicle)

Current implementation: `src/parsers/mistral_vibe.rs`

- Indexes `role == user|assistant` text
- Skips `role == tool` records
- Skips assistant records that only contain `tool_calls` with empty text

**Title extraction:** First `messages.jsonl` entry where `role == "user"` and `content` is non-empty.

**Timestamp parsing:**

- `start_time`: from `meta.json.start_time`
- `last_updated`: from `meta.json.end_time` (fallback to `start_time`)

**Content extraction:**

```rust
fn extract_vibe_content(event: &Value) -> Option<String> {
    event.get("content")?
        .as_str()
        .map(str::to_string)
        .filter(|s| !s.trim().is_empty())
}
```

**Tool call handling:**

- Tool calls appear on assistant messages under `tool_calls[]`
- Tool outputs are separate messages with `role == "tool"` and `tool_call_id` matching the call id
  (`name` may be present depending on producer)
- Arguments are stored as JSON-encoded strings (`tool_calls[*].function.arguments`)
- Current parser behavior: indexes assistant `tool_calls[]` entries and correlates
  `role == "tool"` outputs by `tool_call_id`; uncorrelated outputs are skipped

**Sub-agent handling:**

- A child session is detected when its directory's parent is named `agents/`;
  `parent_session_id` is then derived from the grandparent `meta.json` and the
  session is marked `is_subagent`.
- The indexer descends into `<session>/agents/` so each child is indexed as its
  own (hidden) session.
- A `task` tool call is surfaced as a navigable **subagent** transcript item
  (not a plain tool call). Its `child_session_id` is resolved at parse time by
  pairing the call's `agent` argument with a child under `agents/`
  (by `agent_profile.name`, chronological for repeats); the `tool` result is
  captured as the subagent's `result_summary`.

**Streaming:** Use `BufReader` line-by-line iteration on `messages.jsonl` —
do not load entire JSONL into memory.

---

## Primary Sources

- [Mistral Vibe Repository](https://github.com/mistralai/mistral-vibe)
- [Mistral Vibe session logger (`meta.json` + `messages.jsonl` + config dump)](https://github.com/mistralai/mistral-vibe/blob/main/vibe/core/session/session_logger.py)
- [Mistral Vibe session loader (filename constants `METADATA_FILENAME` = `meta.json`, `MESSAGES_FILENAME` = `messages.jsonl`)](https://github.com/mistralai/mistral-vibe/blob/main/vibe/core/session/session_loader.py)
- [Mistral Vibe message/session models (`SessionMetadata`, `LLMMessage`, `AgentStats`)](https://github.com/mistralai/mistral-vibe/blob/main/vibe/core/types.py)
- [Mistral Vibe CHANGELOG (verified through 2.14.0, 2026-06-04)](https://github.com/mistralai/mistral-vibe/blob/main/CHANGELOG.md)
- [Mistral Vibe Configuration Docs](https://docs.mistral.ai/mistral-vibe/introduction/configuration)
- [Mistral Vibe system prompt skill section (`<available_skills>`)](https://github.com/mistralai/mistral-vibe/blob/main/vibe/core/system_prompt.py)
- [Mistral Vibe CLI skill slash-command handler](https://github.com/mistralai/mistral-vibe/blob/main/vibe/cli/textual_ui/app.py)
