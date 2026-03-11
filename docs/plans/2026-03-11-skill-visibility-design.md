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
    skill_name TEXT NOT NULL,
    source TEXT NOT NULL,           -- "command" | "tool_call" | "injection"
    PRIMARY KEY (session_id, skill_name),
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX idx_session_skills_name ON session_skills(skill_name);
```

Stores the set of distinct skill names used in each session. Populated during
indexing. Used for session list chips and future search filtering.

The `source` column records how the skill was detected (for diagnostics, not
displayed in the UI). If the same skill appears as both a command and a tool
call, the earliest detected source wins (primary key prevents duplicates on
`(session_id, skill_name)`).

### 1.2 `transcript_items` — new `skill_kind` column

```sql
ALTER TABLE transcript_items
    ADD COLUMN skill_kind TEXT;          -- NULL | "command" | "content" | "reminder"
ALTER TABLE transcript_items
    ADD COLUMN skill_name TEXT;          -- NULL or extracted skill name
```

Existing rows keep `skill_kind = NULL` (regular items). New skill-tagged
rows get one of three values:

| `skill_kind` | Meaning | Row rendering |
|--------------|---------|---------------|
| `"command"` | Slash command message | Skill command row (brown accent) |
| `"content"` | Injected skill markdown | Folded content row (tan background) |
| `"reminder"` | System reminder with skill checklist | Folded reminder row (purple-grey) |
| `NULL` | Regular message / tool call / subagent | Unchanged |

`skill_name` stores the extracted name (e.g. `"brainstorming"`,
`"writing-plans"`) for display in the command row and for the enhanced tool
call row. For `"content"` and `"reminder"` kinds, it links the folded row
to its originating skill.

### 1.3 `tool_calls` — new `skill_name` column

```sql
ALTER TABLE tool_calls ADD COLUMN skill_name TEXT;  -- NULL or skill name
```

Set when `tool_name == "Skill"` (Claude Code) or `tool == "skill"`
(OpenCode). Allows the tool call row to display `Skill → brainstorming`
instead of generic `Skill`.

No changes to `messages`, `sessions`, `subagents`, or `file_fingerprints`.

---

## 2. Skill extraction — parser changes

Each parser must detect skill artifacts during indexing and populate the new
schema columns. Extraction is best-effort: if detection fails, the row falls
back to its existing rendering (no data loss).

### 2.1 Claude Code

Three events per skill invocation:

| Event | Detection | `skill_kind` | `skill_name` extraction |
|-------|-----------|--------------|------------------------|
| User message with XML tags | `<command-name>/` prefix in content | `"command"` | Strip `/` prefix from `<command-name>` value; split on `:` for namespaced skills (e.g. `superpowers-extended-cc:brainstorming` → `brainstorming`) |
| Following user message(s) with `<system-reminder>` | Content starts with `<system-reminder>` and follows a command message | `"reminder"` | Inherit `skill_name` from preceding command |
| `Skill` tool call | `tool_use` with `name == "Skill"` | N/A (tool call row) | Read `input.skill` from tool call input JSON |

**Injected skill content detection:**
Between a command message and the Skill tool call, user messages whose content
does *not* start with `<system-reminder>` but follows a command message are
tagged `skill_kind = "content"`. Heuristic: the message is within 3 positions
of the command and has content length > 500 characters (skill definitions are
typically 2–5 KB).

**Args extraction:**
`<command-args>` value from the command message, stored as the message content
for display as subtitle in the command row.

### 2.2 OpenCode

Two events per skill invocation:

| Event | Detection | `skill_kind` | `skill_name` extraction |
|-------|-----------|--------------|------------------------|
| User message (skill markdown as first text part) | Parent of a `tool == "skill"` assistant part (via `parentID`) | `"content"` | Inherit from tool part's `state.metadata.name` |
| Assistant `tool == "skill"` part | `part.data.tool == "skill"` | N/A (tool call row) | `state.metadata.name`, fallback `state.input.name` |

OpenCode has no separate command row — the user's original message is
replaced by the skill markdown. The tool call row carries the skill name.

### 2.3 Codex

Two events per skill invocation:

| Event | Detection | `skill_kind` | `skill_name` extraction |
|-------|-----------|--------------|------------------------|
| User message with `$skill-name` | `text_elements[].placeholder` starts with `$` | `"command"` | Strip `$` prefix from placeholder value |
| Following user message with `<skill>` wrapper | Content contains `<skill>` root element | `"content"` | Extract `<name>…</name>` from `<skill>` wrapper |

### 2.4 Mistral Vibe

Two loading paths:

**Exact slash path** (`/<skill-name>`):

| Event | Detection | `skill_kind` | `skill_name` extraction |
|-------|-----------|--------------|------------------------|
| User message is SKILL.md body | First user message content matches SKILL.md frontmatter pattern (`---\nname:`) | `"content"` | Parse `name:` from YAML frontmatter |

**Free-form path** (`/<skill-name> args`):

| Event | Detection | `skill_kind` | `skill_name` extraction |
|-------|-----------|--------------|------------------------|
| User message `/<skill-name> args` | Content starts with `/` and matches known skill pattern | `"command"` | Extract name after `/` prefix |
| `read_file` tool call targeting `skills/*/SKILL.md` | `read_file` input path matches `skills/<name>/SKILL.md` | N/A (tool call, `skill_name` set) | Extract `<name>` from path segment |

### 2.5 `session_skills` population

After parsing all transcript items for a session, collect distinct
`skill_name` values from:
- Transcript items where `skill_kind IS NOT NULL`
- Tool calls where `skill_name IS NOT NULL`

Insert into `session_skills` with the `source` value from the first detection
point (command > tool_call > injection priority).

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
    pub skill_name: String,           // e.g. "brainstorming"
    pub namespace: Option<String>,    // e.g. "superpowers-extended-cc"
    pub args: Option<String>,         // e.g. "heatmap width limit exploration"
    pub assistant: AiAssistant,       // source badge
}

pub struct FoldedContentInit {
    pub skill_name: String,
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

The existing `ToolCallItemInit` gains an optional `skill_name`:

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
    pub skill_name: Option<String>,
}
```

### 3.3 Session model extension

```rust
pub struct Session {
    // ... existing fields ...
    pub skills: Vec<String>,         // populated from session_skills table
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
    ("tool_call", _)              => ToolCall(...)           // skill_name passed through
    ("subagent", _)               => Subagent(...)           // unchanged
}
```

---

## 4. Database query changes

### 4.1 `load_transcript_items()` — extended SELECT

Add `skill_kind`, `skill_name` from `transcript_items` and `skill_name`
from `tool_calls` to the existing LEFT JOIN query:

```sql
SELECT ti.item_index, ti.kind,
       ti.skill_kind, ti.skill_name AS ti_skill_name,    -- NEW
       ti.message_index, ti.tool_call_id, ti.subagent_id,
       m.role, substr(m.content, 1, ?2) AS content_preview,
       length(m.content) AS content_len, m.timestamp, m.model,
       tc.tool_name, tc.status, tc.summary,
       tc.skill_name AS tc_skill_name,                    -- NEW
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

After loading sessions, batch-load skills:

```sql
SELECT session_id, skill_name
FROM session_skills
WHERE session_id IN (?, ?, ...)
ORDER BY session_id, skill_name
```

Group results by `session_id` and attach to each `Session.skills` vector.

---

## 5. UI rendering

### 5.1 Skill command row

Layout (44px height):

```
[gear-icon]  /brainstorming  [superpowers]  heatmap width limit exploration
  ↑ brown      ↑ bold 13px     ↑ chip          ↑ dim 12px subtitle
  #986a44
```

- Left border: 3px solid `#986a44` (brown).
- Background: `alpha(@card_bg_color, 0.5)` (same as message rows).
- Gear icon: `emblem-system-symbolic` at 16px in a rounded container with
  `#986a44` at 12% opacity.
- Skill name: bold label, 13px. Prefixed with `/` for display.
- Source badge (namespace): only shown if namespace is present. Brown chip
  with `#986a44` at 10% opacity, 11px label.
- Args: dim label, 12px, `opacity: 0.6`. Truncated to 80 chars with
  ellipsis. Only shown when args are non-empty.

CSS class: `.skill-command-row`

### 5.2 Folded content row

Layout (32px height, collapsed):

```
[▶]  Skill content: brainstorming (2.4 KB)
 ↑     ↑ dim 11px
 brown
```

- Left border: 2px solid `#986a44` (skill definition) or `#9141ac` (system
  reminder).
- Background:
  - Skill definition: `#f4f0ec` at 70% opacity (tan).
  - System reminder: `#f0eff4` at 70% opacity (purple-grey).
- Collapse indicator: `▶` (collapsed) / `▼` (expanded), 11px, matching
  accent color.
- Label: `"Skill content: {skill_name} ({byte_size})"` or
  `"System reminder: available skills + checklist ({byte_size})"`.
- Byte size: formatted as `X.X KB` (divide by 1024, one decimal).

**Expanded state** (variable height, up to 200px with scroll):

```
[▼]  Skill content: brainstorming (2.4 KB)                    [Copy]
     # Brainstorming Ideas Into Designs
     ## Overview
     Help turn ideas into fully formed designs…
     [ 48 more lines ]
```

- Full content loaded from DB on first expand (same pattern as existing
  message expand/collapse).
- Content displayed in a `gtk::TextView` (read-only, monospace, no markdown
  rendering) with max height 200px and vertical scrolling.
- "Copy" button in top-right corner (flat button, `edit-copy-symbolic`).
- "X more lines" indicator at bottom if content exceeds visible area.

CSS classes: `.folded-content-row`, `.folded-reminder-row`

### 5.3 Enhanced Skill tool call row

Same layout as existing tool call rows, with one change:

- **Tool name label** displays `Skill → brainstorming` instead of `Skill`
  when `skill_name` is set.
- The `→` is a literal arrow character (dim, not bold).
- No other changes to the tool call row layout, status badge, duration, or
  inspect button.

This is a minimal change in `transcript_row.rs` where the tool name label
is built: check `skill_name` and format accordingly.

### 5.4 Session list skill chips

Below the subtitle line in `SessionRow`, display skill name chips:

```
Heatmap width limit exploration
sessions-chronicle · 14:32 · 24 messages
[brainstorming] [writing-plans]
```

- Each chip: `gtk::Label` inside a `gtk::Box` with:
  - Background: `#986a44` at 10% opacity.
  - Text: skill name, 10px, `#986a44` color.
  - Border-radius: 4px.
  - Padding: 2px 6px.
  - Margin-right: 4px between chips.
- Container: horizontal `gtk::FlowBox` or `gtk::Box`, max 4 chips visible.
  If more than 4, show `+N` overflow indicator.
- If `session.skills` is empty, no chip row is added (no extra spacing).

CSS class: `.skill-chip`

### 5.5 Dark mode compatibility

All hardcoded colors use `alpha()` over theme colors where possible.
The accent colors (`#986a44` brown, `#9141ac` purple) are used at low
opacity for backgrounds, which adapts naturally to dark themes. The text
accent colors remain fixed (readable on both light and dark backgrounds).

The folded row backgrounds (`#f4f0ec`, `#f0eff4`) should switch to darker
tones in dark mode. Use `@theme_bg_color` with a brown/purple tint via
`mix()` or define two CSS rules gated by the `.dark` style class:

```css
.folded-content-row {
    background-color: alpha(#986a44, 0.08);
}
.folded-reminder-row {
    background-color: alpha(#9141ac, 0.06);
}
```

Using `alpha()` over the accent color automatically adapts to the theme
background.

---

## 6. Session title cleaning

When the first message in a session is a skill command, the `first_prompt`
field currently contains raw XML tags or SKILL.md content. Cleaning rules:

| Assistant | Raw `first_prompt` | Cleaned `first_prompt` |
|-----------|-------------------|----------------------|
| Claude Code | `<command-message>brainstorming</command-message>\n<command-name>/brainstorming</command-name>\n<command-args>heatmap width</command-args>` | `brainstorming: heatmap width` |
| Codex | `$logseq un fichier markdown` | `logseq: un fichier markdown` |
| Mistral Vibe (exact) | Full SKILL.md body (2+ KB) | `learn-rust` (from frontmatter `name:`) |
| Mistral Vibe (free-form) | `/<skill-name> args` | `<skill-name>: args` |
| OpenCode | Skill markdown body | `brainstorming` (from tool part metadata) |

Cleaning happens during parsing, before writing `first_prompt` to the
`sessions` table. The cleaning function is shared across parsers:

```rust
pub fn clean_skill_prompt(raw: &str, assistant: AiAssistant) -> Option<String> {
    // Returns Some(cleaned) if skill detected, None otherwise
}
```

If the first message is a skill command, `first_prompt` is set to the
cleaned version. The original content remains in the `messages` FTS5 table
for full-text search.

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

**Keyboard accessibility:**
- Tab/Shift-Tab navigates between rows (existing behavior).
- Enter or Space toggles expand/collapse on focused folded rows.
- The "Copy" button is focusable within the expanded row.

---

## 8. Error handling and edge cases

- **Partial detection:** If a skill command is detected but no matching
  content/reminder follows, the command row is still rendered (standalone).
  Missing content means no folded rows — no error.
- **Unknown skill format:** If XML tag parsing or frontmatter extraction
  fails, the message falls back to regular rendering (`skill_kind = NULL`).
- **Multiple skills in one session:** Each skill invocation generates its own
  set of rows. `session_skills` stores all distinct names.
- **Namespaced skills:** `superpowers-extended-cc:brainstorming` displays as
  `brainstorming` with a `superpowers-extended-cc` source badge. The full
  name is stored in `session_skills`; the display name strips the namespace.
- **Empty args:** The args subtitle is hidden (not shown as empty string).
- **Migration on existing data:** The v5 migration adds nullable columns.
  Existing data has `skill_kind = NULL` and is unaffected. A full reindex
  populates the new columns for all sessions.

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

- Migration v5 adds `skill_kind` and `skill_name` to `transcript_items`.
- Migration v5 adds `skill_name` to `tool_calls`.
- Migration v5 creates `session_skills` table with correct indexes.
- Existing data survives migration (nullable columns, no data loss).

### 10.2 Parser tests (per assistant)

- **Claude Code:** session with `/brainstorming` command produces transcript
  items with `skill_kind = "command"`, `"content"`, `"reminder"` and correct
  `skill_name`. Skill tool call has `skill_name` set.
- **OpenCode:** session with `tool == "skill"` part produces `"content"`
  tagged user message and tool call with `skill_name`.
- **Codex:** session with `$logseq` produces `"command"` and `"content"`
  items.
- **Mistral Vibe:** exact path session produces `"content"` item; free-form
  path produces `"command"` item and tool call with `skill_name`.
- **Title cleaning:** `first_prompt` is cleaned for all assistant types.
- **session_skills populated:** all test sessions have correct skill entries.

### 10.3 UI tests (manual)

- Skill command row displays with brown accent, gear icon, skill name, args.
- Folded content row is collapsed by default; click expands with full text.
- Folded reminder row uses purple-grey styling.
- Skill tool call row shows `Skill → brainstorming`.
- Session list shows skill chips; sessions without skills show no chips.
- Dark mode: all rows are readable, folded backgrounds adapt.
- Keyboard: Enter/Space toggles folded rows.

### 10.4 Fixture data

Add skill invocation samples to existing test fixtures:

- `tests/fixtures/claude-code/`: session with `/brainstorming` command.
- `tests/fixtures/codex/`: session with `$skill-name` invocation.
- `tests/fixtures/mistral-vibe/`: session with `/<skill-name>` invocation.

### 10.5 Verification command

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
- No new widget patterns introduced — extends existing `TranscriptRow`
  factory and CSS classes.
