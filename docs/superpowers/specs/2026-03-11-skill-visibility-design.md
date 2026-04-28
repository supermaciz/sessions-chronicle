# Skill Visibility — Folded Rows Design (Issue #47)

Implements **Proposal A** from the
[skill visibility exploration](2026-03-10-skill-visibility-exploration.md):
each skill lifecycle event becomes a distinct, typed row in the transcript.
Boilerplate is folded by default; session list shows skill chips.

## Problem

Skills are a first-class workflow concept across AI assistants, but Sessions
Chronicle treats them as raw data, producing four visible problems:

1. **Garbled user messages** — Claude Code encodes slash commands as XML tags
   (`<command-message>brainstorming</command-message>…`), displayed verbatim.
2. **Generic tool call rows** — the Skill tool call shows `Skill` with no
   indication of *which* skill was loaded.
3. **Boilerplate flooding** — full skill definitions (2–5 KB of markdown) and
   system reminders (4+ KB) appear as regular messages, drowning out the
   actual conversation.
4. **No session-level signal** — no way to see at a glance that a session used
   brainstorming, TDD, or debugging workflows.

## Scope

This design covers:

- **Skill extraction** during parsing — detect skill artifacts per assistant
  and store structured metadata.
- **New transcript row types** — skill command rows, folded content rows,
  enhanced Skill tool call rows.
- **Session list skill chips** — display skill names below the subtitle.
- **Session title cleaning** — render slash commands as readable text instead
  of raw XML / SKILL.md frontmatter.

Out of scope:

- Grouped/collapsed skill blocks (Proposal B — future evolution).
- Inspector skill renderer (Proposal C).
- Skill-based search filtering (`skill:brainstorming`).
- Skill usage analytics.

---

## 1. Schema changes (migration v5)

### 1.1 `session_skills` table

```sql
CREATE TABLE session_skills (
    session_id TEXT NOT NULL,
    normalized_skill_name TEXT NOT NULL,
    PRIMARY KEY (session_id, normalized_skill_name),
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX idx_session_skills_name ON session_skills(normalized_skill_name);
```

Stores the distinct set of skills detected in each session. Populated during
indexing. Used for session-level skill display and future search/filter
features.

`normalized_skill_name` is the parser-normalized skill identifier persisted in
the database. UI-facing labels such as a short display name or an optional
qualifier badge are derived at runtime from this stored value and are not
persisted separately.

If the same skill appears multiple times in one session, transcript rows keep
each occurrence, while `session_skills` stores only the deduplicated
session-level set.

### 1.2 `transcript_items` — new skill metadata columns

```sql
ALTER TABLE transcript_items
    ADD COLUMN skill_kind TEXT;                    -- NULL | "command" | "content" | "reminder"
ALTER TABLE transcript_items
    ADD COLUMN normalized_skill_name TEXT;         -- NULL or parser-normalized skill name
```

Existing rows keep `skill_kind = NULL` and continue to render exactly as they
do today.

New skill-tagged rows use:

| `skill_kind` | Meaning | Row rendering |
|--------------|---------|---------------|
| `"command"` | Explicit skill invocation command | Skill command row |
| `"content"` | Injected skill definition or body | Folded content row |
| `"reminder"` | Injected system reminder/checklist | Folded reminder row |
| `NULL` | Regular message / tool call / subagent | Unchanged |

`normalized_skill_name` links a transcript item to the detected skill identity
when classification succeeds. If parsing is ambiguous or no stable skill name
can be produced, the row keeps `NULL` and falls back to the existing
rendering.

### 1.3 `tool_calls` — new skill metadata column

```sql
ALTER TABLE tool_calls
    ADD COLUMN normalized_skill_name TEXT;         -- NULL or parser-normalized skill name
```

Set when a tool call explicitly represents skill loading, such as Claude Code
`Skill` calls or OpenCode `skill` parts.

This allows the transcript to display `Skill -> brainstorming` instead of a
generic `Skill`, while keeping the existing tool call rendering path unchanged
for all non-skill calls.

### 1.4 Persistence boundary

No display-only skill fields are stored in the database. In particular:

- `normalized_skill_name` is persisted
- `display_name` is derived at runtime
- an optional qualifier badge is derived at runtime when the normalized name
  includes a prefix

This keeps the schema minimal and avoids storing duplicated presentation data.

### 1.5 Migration behavior

Migration v5 is additive only:

- new columns are nullable
- existing transcript rows remain valid
- existing tool call rows remain valid
- existing sessions require reindexing before skill metadata appears

Until reindexing populates the new fields, all rows continue to use the
current non-skill rendering.

No changes to `messages`, `sessions`, `subagents`, or `file_fingerprints`.

---

## 2. Skill extraction — parser changes

Each parser detects skill artifacts during indexing and populates the new skill
metadata columns when confidence is high enough.

Extraction is conservative by design: false positives are treated as a higher
risk than missed detections. If detection is ambiguous, the item keeps its
current message or tool call rendering and no skill metadata is stored.

### 2.1 Detection precedence

Detection precedence is strict:

1. Structural signals  
   Parent/child links, explicit tool metadata, explicit file paths, or other
   parser-native references.
2. Explicit syntax parsing  
   Well-delimited formats such as XML tags, YAML frontmatter, or wrapped skill
   payloads.
3. Heuristic proximity rules  
   Relative position, content size, adjacency to other skill artifacts.

A lower-confidence detector must not override a higher-confidence result.

A row is only tagged as a skill artifact when the parser can also produce a
stable `normalized_skill_name`. If no stable skill name can be produced, the
row falls back to existing rendering.

### 2.2 Claude Code

Claude Code skill invocations may surface through three related artifacts:

| Event | Preferred detection | `skill_kind` | `normalized_skill_name` source |
|-------|----------------------|--------------|--------------------------------|
| User command message | Explicit command XML tags | `"command"` | Parsed from `<command-name>` or another explicit command field |
| Injected reminder message | Explicit `<system-reminder>` wrapper following a detected skill command | `"reminder"` | Inherited from the associated command |
| `Skill` tool call | Tool call with `name == "Skill"` | N/A (tool call row) | `input.skill` from tool input |

**Preferred strategy:**  
Use explicit command metadata and the `Skill` tool call as the primary
anchors.

**Heuristic fallback:**  
If injected skill content appears between a detected command and the
corresponding `Skill` tool call, nearby long user messages may be tagged as
`"content"` only when they are strongly bounded by those explicit anchors.

Heuristics such as message distance or minimum content length are fallback-only
and must not be the sole basis for reclassifying otherwise normal-looking user
content.

**Args extraction:**  
`<command-args>` from the command message is used as the subtitle shown in the
skill command row when present.

### 2.3 OpenCode

OpenCode provides the strongest structural linkage of the four assistants.

| Event | Preferred detection | `skill_kind` | `normalized_skill_name` source |
|-------|----------------------|--------------|--------------------------------|
| User message containing injected skill body | Structural parent link to a `tool == "skill"` assistant part | `"content"` | Inherited from the linked tool part metadata |
| Assistant skill tool part | `part.data.tool == "skill"` | N/A (tool call row) | `state.metadata.name`, fallback `state.input.name` |

OpenCode has no separate skill command row in the currently observed format.
The transcript therefore surfaces skill visibility through the folded content
row and the enriched tool call row.

Because OpenCode exposes both a structural relation and explicit skill
metadata, these signals should be treated as high confidence.

### 2.4 Codex

Codex may expose skill usage through a command placeholder and a wrapped skill
payload.

| Event | Preferred detection | `skill_kind` | `normalized_skill_name` source |
|-------|----------------------|--------------|--------------------------------|
| User command message | Placeholder value beginning with `$` | `"command"` | Placeholder value with the leading `$` removed |
| Injected skill content | Content parses as a `<skill>` wrapper payload | `"content"` | Extracted from `<name>...</name>` inside the wrapper |

The `<skill>` wrapper should be treated as an explicit syntax signal only when
it can be parsed as a structured wrapper, not by substring matching alone.

If wrapper parsing fails, the row falls back to regular message rendering.

### 2.5 Mistral Vibe

Mistral Vibe appears to expose skill loading through multiple patterns, which
should be handled conservatively.

**Exact slash path** (`/<skill-name>`):

| Event | Preferred detection | `skill_kind` | `normalized_skill_name` source |
|-------|----------------------|--------------|--------------------------------|
| Injected SKILL.md body | Content parses as YAML frontmatter-backed SKILL content | `"content"` | `name:` from frontmatter |

Frontmatter-based detection should only apply when the content shape matches
an actual skill file structure strongly enough to avoid classifying arbitrary
markdown with frontmatter as a skill definition.

**Free-form invocation form:**

| Event | Preferred detection | `skill_kind` | `normalized_skill_name` source |
|-------|----------------------|--------------|--------------------------------|
| User command message | Explicit slash-command form with an extractable skill token | `"command"` | Command token after `/` |
| File-read tool call for a skill file | Tool input path resolving to `skills/<name>/SKILL.md` | N/A (tool call row) | `<name>` extracted from the path |

Any looser pattern matching should be avoided unless the parser has a
concrete, testable source of truth for known skill names.

### 2.6 `session_skills` population

After parsing all transcript items and tool calls for a session, collect
distinct `normalized_skill_name` values from:

- transcript items where `skill_kind IS NOT NULL`
- tool calls where `normalized_skill_name IS NOT NULL`

Insert the deduplicated set into `session_skills`.

Repeated invocations of the same skill remain visible in the transcript, but
only one session-level entry is stored.

### 2.7 Fallback guarantee

Skill extraction is best-effort and non-destructive:

- if explicit detection succeeds, store skill metadata
- if detection is ambiguous, do not store skill metadata
- if extraction fails, preserve the current rendering path
- no parser should reclassify content as a skill artifact unless it can
  produce a stable `normalized_skill_name`

---

## 3. Data structures

### 3.1 New `TranscriptItemInit` variants

```rust
pub enum TranscriptItemInit {
    Message(MessageItemInit),
    ToolCall(ToolCallItemInit),
    Subagent(SubagentItemInit),
    // New:
    SkillCommand(SkillCommandInit),
    FoldedContent(FoldedContentInit),
}
```

```rust
pub struct SkillCommandInit {
    pub normalized_skill_name: String,
    pub args: Option<String>,         // e.g. "heatmap width limit exploration"
    pub assistant: AiAssistant,       // assistant badge
}

pub struct FoldedContentInit {
    pub normalized_skill_name: String,
    pub content_kind: FoldedContentKind,
    pub byte_size: usize,            // for display: "2.4 KB"
    pub content_preview: String,     // first ~200 chars for DB preview
    pub content_len: usize,          // full content length
    pub message_index: usize,        // for loading full content on expand
}

pub enum FoldedContentKind {
    SkillDefinition,    // skill_kind = "content"
    SystemReminder,     // skill_kind = "reminder"
}
```

### 3.2 Enhanced `ToolCallItemInit`

The existing `ToolCallItemInit` gains an optional `normalized_skill_name`:

```rust
pub struct ToolCallItemInit {
    pub tool_call_id: String,
    pub tool_name: String,
    pub tool_status: String,
    pub tool_summary: Option<String>,
    pub tool_input_json: Option<String>,
    pub tool_output_text: Option<String>,
    pub duration_ms: Option<i64>,
    pub preview: Option<String>,
    // New:
    pub normalized_skill_name: Option<String>,
}
```

### 3.3 Session model extension

```rust
pub struct Session {
    // ... existing fields ...
    pub skills: Vec<String>,         // normalized skill names from session_skills
}
```

### 3.4 Row-to-init conversion update

`transcript_item_init_from_row()` in `src/database/mod.rs` checks the new
`skill_kind` column:

```
match (row.kind, row.skill_kind) {
    ("message", Some("command"))  => SkillCommand(...)
    ("message", Some("content"))  => FoldedContent(... SkillDefinition)
    ("message", Some("reminder")) => FoldedContent(... SystemReminder)
    ("message", None)             => Message(...)           // unchanged
    ("tool_call", _)              => ToolCall(...)           // normalized_skill_name passed through
    ("subagent", _)               => Subagent(...)           // unchanged
}
```

UI-facing display labels are derived from `normalized_skill_name` at runtime.
The database and row init types do not persist separate display-only fields.

---

## 4. Database query changes

### 4.1 `load_transcript_items()` — extended SELECT

Add `skill_kind`, `normalized_skill_name` from `transcript_items` and
`normalized_skill_name` from `tool_calls` to the existing LEFT JOIN query:

```sql
SELECT ti.item_index, ti.kind,
       ti.skill_kind, ti.normalized_skill_name AS ti_skill_name, -- NEW
       ti.message_index, ti.tool_call_id, ti.subagent_id,
       m.role, substr(m.content, 1, ?2) AS content_preview,
       length(m.content) AS content_len, m.timestamp, m.model,
       tc.tool_name, tc.status, tc.summary,
       tc.normalized_skill_name AS tc_skill_name,         -- NEW
       substr(tc.input_json, 1, 512) AS input_json,
       substr(tc.output_text, 1, 512) AS output_text,
       tc.duration_ms,
       sa.title AS subagent_title, sa.prompt AS subagent_prompt
FROM transcript_items ti
LEFT JOIN messages m ON ...
LEFT JOIN tool_calls tc ON ...
LEFT JOIN subagents sa ON ...
WHERE ti.session_id = ?1
ORDER BY ti.item_index
LIMIT ?3 OFFSET ?4
```

### 4.2 `load_sessions()` — skill chips

After loading sessions, batch-load normalized skill names:

```sql
SELECT session_id, normalized_skill_name
FROM session_skills
WHERE session_id IN (?, ?, ...)
ORDER BY session_id, normalized_skill_name
```

Group results by `session_id` and attach to each `Session.skills` vector.

Transcript loading remains the primary deliverable for this design. Session
chips are useful secondary context, but they should not complicate the core
transcript path if `SessionRow` changes prove more invasive than expected.

---

## 5. UI rendering

### 5.1 Rendering priorities

The primary UI goal of this design is transcript readability. Skill-aware
transcript rendering is the core deliverable.

Session-level skill chips are useful secondary context, but they should not
complicate or delay the core transcript experience if the required
`SessionRow` changes prove more invasive than expected.

### 5.2 Skill command row

A skill command row represents an explicit user-visible skill invocation.

It should remain visually close to existing transcript rows, while clearly
distinguishable from normal user messages.

Recommended display elements:

- skill icon
- short skill display name
- optional qualifier badge when the normalized skill name includes a prefix
- optional argument subtitle when arguments are present

The row shows the UI-facing display name, not the full stored
`normalized_skill_name`.

If the normalized name includes a qualifier such as `namespace:name`, the row
derives:

- `display_name = name`
- `qualifier_badge = namespace`

If no qualifier exists, only the display name is shown.

CSS class: `.skill-command-row`

### 5.3 Folded content row

Folded content rows represent injected skill boilerplate that would otherwise
flood the transcript.

There are two folded row subtypes:

- skill definition or injected skill body
- system reminder or checklist content

Both are collapsed by default.

The user-facing interaction pattern should match the existing long-message
expand/collapse behavior:

- collapsed by default
- expandable on click
- keyboard-toggleable when focused
- full content loaded on first expansion
- cached after first load

However, these rows remain their own transcript row type. They should not be
modeled as ordinary message rows with special CSS alone.

Expanded content is displayed in a read-only `gtk::TextView` with a bounded
height and vertical scrolling. A copy action is available in expanded state.

CSS classes: `.folded-content-row`, `.folded-reminder-row`

### 5.4 Enhanced skill tool call row

Skill-aware tool call rows preserve the existing tool call layout and
behavior.

The only behavior change is the title label:

- generic form: `Skill`
- skill-aware form: `Skill -> brainstorming`

This is intentionally minimal. It improves readability without introducing a
new interaction model or inspector path.

The displayed skill label is derived from the stored normalized name and uses
the short display name.

### 5.5 Session list skill chips

If session chips remain in scope, they appear as lightweight secondary metadata
below the existing session subtitle.

They communicate only the deduplicated session-level set of detected skills.
They do not attempt to show invocation count or ordering.

Important constraint: the current `SessionRow` is based on a compact
`adw::ActionRow` layout. Adding a dedicated chip row may require extending or
partially restructuring that layout, rather than simply attaching a suffix
widget.

If implemented, the container shows at most four chips and then a `+N`
overflow indicator.

CSS class: `.skill-chip`

### 5.6 Theme and accessibility behavior

Skill-aware rows remain compatible with both light and dark themes.

Visual differentiation relies on subtle accenting, distinct row styling, and
consistent spacing and affordances, not on strong background fills alone.

Accessibility expectations:

- folded rows can receive focus
- Enter and Space toggle expansion
- copy actions remain keyboard reachable
- visual distinction does not rely only on color

### 5.7 Rendering fallback rule

Rows with no skill metadata render exactly as they do today.

Skill-aware rendering is additive only. It must never make unsupported or
ambiguously parsed transcripts less readable than the current UI.

---

## 6. Session title cleaning

### 6.1 Goal

Session title cleaning exists to prevent raw skill boilerplate or command
markup from becoming the visible session title.

The goal is not to perfectly reconstruct every original invocation form. The
goal is to produce a short, readable title when the first session artifact is
confidently identified as skill-related.

### 6.2 Confidence rule

Title cleaning only applies when the first prompt is identified as a skill
artifact with high confidence.

If the first message is ambiguous, malformed, or cannot be linked to a stable
`normalized_skill_name`, the existing `first_prompt` behavior remains
unchanged.

### 6.3 Stored vs displayed values

The cleaned session title is a display-oriented summary. It uses the same
derived UI-facing skill label as the transcript:

- unqualified normalized name -> display directly
- qualified normalized name -> display the short name, not the full qualified
  form

Arguments may be appended when they are clearly extractable and short enough
to improve readability.

Examples:

| Assistant | Raw `first_prompt` | Cleaned `first_prompt` |
|-----------|-------------------|------------------------|
| Claude Code | `<command-message>brainstorming</command-message>\n<command-name>/brainstorming</command-name>\n<command-args>heatmap width</command-args>` | `brainstorming: heatmap width` |
| Codex | `$logseq un fichier markdown` | `logseq: un fichier markdown` |
| Mistral Vibe (exact) | Full SKILL.md body (2+ KB) | `learn-rust` |
| Mistral Vibe (free-form) | `/<skill-name> args` | `<skill-name>: args` |
| OpenCode | Skill markdown body | `brainstorming` |

The cleaned title does not expose raw XML tags, full SKILL.md bodies, or
internal wrapper syntax.

### 6.4 Assistant-specific expectation

Different assistants may require different extraction paths, but they all
follow the same output rule:

- extract a stable normalized skill name when possible
- derive a short display label from it
- include short args only when they are explicit and useful
- otherwise fall back to existing title behavior

### 6.5 Search and source preservation

Cleaning happens during parsing, before writing `first_prompt` to the
`sessions` table.

```rust
pub fn clean_skill_prompt(raw: &str, assistant: AiAssistant) -> Option<String> {
    // Returns Some(cleaned) if a skill artifact is detected with high confidence
}
```

The cleaned `first_prompt` is used for display, while the original underlying
message content remains available through normal indexed message storage. This
preserves search behavior and avoids lossy rewriting of transcript history.

### 6.6 Failure mode

If title cleaning fails:

- indexing continues
- the session remains visible
- the title falls back to current behavior
- no transcript data is discarded

---

## 7. Expand/collapse interaction

Folded content rows reuse the existing expand/collapse pattern from
`transcript_row.rs`:

1. Initial state: collapsed (32px row, stub label).
2. Click anywhere on the row (or press Enter/Space when focused) toggles
   expanded state.
3. On first expand: send `LoadFullContent(session_id, message_index)` message
   to load the full message content from DB.
4. Content is cached in the widget model after first load (no re-fetch on
   subsequent toggles).
5. Collapse hides the content `TextView` and restores the 32px row height.

The `▶`/`▼` indicator updates on toggle. The "Copy" button is only visible
in expanded state.

Folded rows share the loading pattern used by long message rows, but remain a
distinct transcript row type with their own semantics and styling.

**Keyboard accessibility:**
- Tab/Shift-Tab navigates between rows (existing behavior).
- Enter or Space toggles expand/collapse on focused folded rows.
- The "Copy" button is focusable within the expanded row.

---

## 8. Error handling and edge cases

Skill detection is conservative. A false positive that hides or reframes
normal conversation content is considered worse than a missed detection.

### 8.1 Fallback behavior

- If a skill artifact is detected with sufficient confidence, store skill
  metadata and use the corresponding skill-aware rendering.
- If detection is ambiguous, do not store skill metadata.
- If parsing fails at any step, the row falls back to the existing message or
  tool call rendering.
- Existing sessions that have not yet been reindexed continue to render
  without skill-specific behavior.

### 8.2 Partial detection

Partial detection is valid and should not be treated as an error.

Examples:

- a skill command is detected, but no matching injected content follows
- a skill tool call is detected, but no matching folded content row can be
  linked
- a folded content candidate is found, but no stable
  `normalized_skill_name` can be produced

In these cases, only the confidently detected artifacts are surfaced. No
synthetic or inferred rows are created to "complete" a skill lifecycle when
the source data does not support it.

### 8.3 False positive prevention

Ambiguous content remains a regular transcript row.

This includes:

- long markdown messages that are not skill definitions
- messages with YAML frontmatter unrelated to skills
- XML-like content that only partially resembles a skill wrapper
- slash-prefixed user commands that are not known to represent a skill
  invocation in the parsed source format

The parser prefers under-classification over over-classification.

### 8.4 Repeated and mixed skill usage

- Multiple invocations of the same skill in one session create multiple
  transcript artifacts where detected.
- `session_skills` stores only the deduplicated session-level set.
- Different assistants may express the same visible skill through different
  raw formats, but the parser normalizes them to the same
  `normalized_skill_name` when confidence is high enough.
- If two raw forms cannot be normalized confidently to the same identity, they
  remain distinct rather than being merged speculatively.

### 8.5 Qualified skill names

If a detected skill name includes a qualifier such as `namespace:name`, the
full normalized value is stored.

UI presentation derives:

- a short display name
- an optional qualifier badge

If no qualifier is present, the full normalized value is used directly as the
display name.

### 8.6 Migration and reindex behavior

Migration v5 is additive and does not invalidate existing transcript data.

After migration:

- rows with `NULL` skill metadata continue to render unchanged
- skill-aware rendering appears only after reindexing populates the new
  columns
- partial reindex states are acceptable and must not break transcript loading

### 8.7 Unknown or evolving source formats

Assistant transcript formats are treated as untrusted and subject to change.

If a source format evolves and a parser can no longer extract skill metadata
reliably:

- indexing continues where possible
- affected rows fall back to regular rendering
- the application does not fail transcript loading solely because skill
  extraction failed

---

## 9. CSS additions

```css
/* Skill command row */
.skill-command-row {
    padding: 6px 12px;
    border-radius: 8px;
    background-color: alpha(@card_bg_color, 0.5);
    border-left: 3px solid #986a44;
    min-height: 32px;
}

.skill-command-row .skill-icon-box {
    background-color: alpha(#986a44, 0.12);
    border-radius: 6px;
    padding: 4px;
}

.skill-command-row .skill-name {
    font-weight: bold;
    font-size: 13px;
}

.skill-command-row .skill-badge {
    background-color: alpha(#986a44, 0.10);
    border-radius: 4px;
    padding: 1px 6px;
    font-size: 11px;
    color: #986a44;
}

.skill-command-row .skill-args {
    opacity: 0.6;
    font-size: 12px;
}

/* Folded content rows */
.folded-content-row {
    padding: 4px 12px;
    border-radius: 6px;
    background-color: alpha(#986a44, 0.08);
    border-left: 2px solid #986a44;
    min-height: 24px;
    margin-bottom: 2px;
}

.folded-reminder-row {
    padding: 4px 12px;
    border-radius: 6px;
    background-color: alpha(#9141ac, 0.06);
    border-left: 2px solid #9141ac;
    min-height: 24px;
    margin-bottom: 2px;
}

.folded-content-row .fold-label,
.folded-reminder-row .fold-label {
    font-size: 11px;
    opacity: 0.7;
}

.folded-content-row .fold-indicator {
    font-size: 11px;
    color: #986a44;
}

.folded-reminder-row .fold-indicator {
    font-size: 11px;
    color: #9141ac;
}

/* Skill chips in session list */
.skill-chip {
    background-color: alpha(#986a44, 0.10);
    border-radius: 4px;
    padding: 1px 6px;
    font-size: 10px;
    color: #986a44;
}
```

---

## 10. Test and verification plan

### 10.1 Schema tests

- Migration v5 adds `skill_kind` and `normalized_skill_name` to
  `transcript_items`.
- Migration v5 adds `normalized_skill_name` to `tool_calls`.
- Migration v5 creates `session_skills` with the expected primary key and
  index.
- Existing v4 data migrates without data loss.
- Rows with no skill metadata remain queryable and renderable after migration.

### 10.2 Parser tests - positive cases

- **Claude Code:** explicit skill command produces a `"command"` transcript
  item and a matching skill-aware tool call.
- **Claude Code:** injected reminder content linked to a detected command
  produces a `"reminder"` row only when bounded strongly enough by explicit
  anchors.
- **OpenCode:** `tool == "skill"` metadata produces a tool call row with
  `normalized_skill_name`, and linked injected content produces a `"content"`
  row.
- **Codex:** `$skill-name` command produces a `"command"` row; wrapped skill
  payload produces a `"content"` row when parsing succeeds.
- **Mistral Vibe:** SKILL.md-style content with parseable frontmatter produces
  a `"content"` row.
- **Mistral Vibe:** skill file path reads produce skill-aware tool call
  metadata when the path shape is explicit.
- **Title cleaning:** skill-driven `first_prompt` cleanup works only for
  high-confidence detections.
- **Session aggregation:** `session_skills` contains the deduplicated set of
  detected skills for a session.

### 10.3 Parser tests - negative cases

- Long non-skill markdown does not become a folded skill row.
- Generic YAML frontmatter does not become skill content unless the full skill
  shape matches.
- Partial XML or malformed wrappers do not produce skill rows.
- Slash commands unrelated to skill loading do not produce skill metadata.
- Ambiguous content near a real skill invocation does not get classified as
  skill content unless the detection rule is strong enough.
- Failed extraction preserves the existing row type and rendering path.

### 10.4 Normalization tests

- Equivalent raw forms normalize to the same `normalized_skill_name` when
  expected.
- Qualified names preserve their full normalized value.
- Display-oriented derivation is stable:
  - unqualified name -> no qualifier badge
  - qualified name -> short display name plus qualifier badge
- No UI-only field is required to reconstruct the expected display label from
  stored data.

### 10.5 Transcript/UI behavior tests

- Skill command rows render differently from regular message rows.
- Folded content rows are collapsed by default.
- Folded reminder rows use distinct styling from folded skill definition rows.
- Expanding a folded row loads full content and preserves subsequent toggle
  behavior.
- Skill-aware tool call rows display a specific skill label instead of generic
  `Skill`.
- Rows with no skill metadata render exactly as they did before v5.

### 10.6 Session-level UI tests

- Sessions with detected skills display chips.
- Sessions without detected skills display no extra row or spacing.
- Repeated invocations of the same skill still produce a single chip.
- Qualified names display a readable short label.

### 10.7 Fixture coverage

Add or extend fixtures so that each assistant has:

- at least one positive skill-detection sample
- at least one negative near-miss sample
- at least one case where fallback rendering is intentionally preserved

This is especially important for assistants whose detection depends partly on
syntax parsing or heuristics.

### 10.8 Existing fixture paths

Extend skill invocation coverage in existing test fixtures:

- `tests/fixtures/claude-code/`: session with `/brainstorming` command.
- `tests/fixtures/codex/`: session with `$skill-name` invocation.
- `tests/fixtures/mistral-vibe/`: session with `/<skill-name>` invocation.

### 10.9 Verification goal

Manual verification should confirm:

- raw boilerplate no longer dominates the transcript in supported cases
- ambiguous content is not incorrectly folded away
- transcript rendering remains stable for unsupported or partially supported
  sessions
- skill-aware rendering improves readability without hiding recoverable source
  content

### 10.10 Verification command

```bash
flatpak-builder --run flatpak_app \
  build-aux/io.github.supermaciz.sessionschronicle.Devel.json \
  sessions-chronicle --sessions-dir tests/fixtures
```

---

## Expected outcome

- Skill commands are displayed as readable, styled rows instead of raw XML.
- Skill boilerplate (definitions, system reminders) is folded by default,
  expandable on demand.
- Skill tool calls show which skill was loaded.
- Session list surfaces skill usage at a glance via chips.
- Session titles are clean when the first message is a slash command.
- Existing non-skill sessions keep their current rendering paths unchanged.
