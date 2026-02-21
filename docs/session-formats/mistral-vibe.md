# Mistral Vibe — Session Format Reference

Format reference for Mistral Vibe session files.
See [SESSION_FORMAT_ANALYSIS.md](../SESSION_FORMAT_ANALYSIS.md) for cross-tool comparison tables.

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
└── messages.jsonl   # OpenAI-style chat transcript (one message per line)
```

---

## `meta.json` Format

Session-level metadata:

| Field | Description |
|-------|-------------|
| `session_id` | UUID |
| `start_time` | ISO-8601 string |
| `end_time` | ISO-8601 string |
| `environment.working_directory` | Working directory |
| `git_commit` | Optional git commit hash |
| `git_branch` | Optional git branch name |
| `stats` | Token usage, tool call counters, other session metrics |
| `tools_available` | Set of tools available to the agent for the session |
| `config` | Optional config snapshot: `active_model`, `providers`, `models` arrays |
| `agent_profile` | Optional selected profile/override metadata |

**Model metadata** is session-level via `meta.json.config` snapshot when present
(`config.active_model`, plus `config.providers`/`config.models`).
Requires session logging metadata output — minimal/older logs may omit full config snapshot.

---

## `messages.jsonl` Format

OpenAI-style chat transcript. One JSON object per line.

Messages do not include a normalized model identifier field.

### User / Assistant Messages

```json
{ "role": "user", "content": "Help me refactor this function" }
{ "role": "assistant", "content": "Sure, here's how we can refactor it..." }
```

### System Message

```json
{ "role": "system", "content": "You are a helpful assistant..." }
```

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

---

## Threading

Linear message list in `messages.jsonl`.
Tool calls are embedded in assistant messages and resolved by subsequent `tool` role messages.
No dedicated subagent session model observed in current format.

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

**Streaming:** Use `BufReader` line-by-line iteration on `messages.jsonl` —
do not load entire JSONL into memory.

---

## Primary Sources

- [Mistral Vibe Repository](https://github.com/mistralai/mistral-vibe)
- [Mistral Vibe session logger (`meta.json` + `messages.jsonl` + config dump)](https://github.com/mistralai/mistral-vibe/blob/main/vibe/core/session/session_logger.py)
- [Mistral Vibe message/session models (`SessionMetadata`, `LLMMessage`)](https://github.com/mistralai/mistral-vibe/blob/main/vibe/core/types.py)
- [Mistral Vibe Configuration Docs](https://docs.mistral.ai/mistral-vibe/introduction/configuration)
