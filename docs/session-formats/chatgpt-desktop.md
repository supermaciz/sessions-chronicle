# ChatGPT Desktop — Storage and Import Watch

Exploratory reference for the official ChatGPT desktop application on Linux
and its relationship to Codex session storage. ChatGPT desktop is not currently
a supported session source in Sessions Chronicle.

**Evidence checked: 2026-10-04. Documented: 2026-10-11.** This page records the
previous manual watch; it is not a new upstream verification. No native ChatGPT
storage capture or desktop import compatibility test was performed.

## ChatGPT Desktop Linux

### Sources checked

- Official [Linux application documentation](https://learn.chatgpt.com/docs/linux/linux-app).
- Official [external-agent import documentation](https://learn.chatgpt.com/docs/import).
- Official [projects and chats documentation](https://learn.chatgpt.com/docs/projects).
- Local source resolution and session model, linked under **Inspect next**.

### Confirmed deltas

**Confirmed product behavior; no native storage-schema delta confirmed:**

- The official Linux application is available in preview and provides access
  to Codex and local projects.
- Desktop imports support Claude Code, Claude Cowork, and Cursor, with optional
  automatic updates of imported work. Configuration migration and conversation
  migration are distinct concerns.
- An existing ChatGPT chat can be added to a Codex chat. Work conversation
  history remains separate from Codex history.

These capabilities do not establish that native ChatGPT or Work conversations
are stored as local Codex JSONL rollouts. The checked documentation does not
publish a native ChatGPT desktop persistence contract.

### Likely deltas

**Likely local impact:** multiple conversation origins can appear in the same
desktop application. Treating the application name as a storage-format identifier
could obscure provenance or overstate Sessions Chronicle's coverage.

### Unknowns

- Native ChatGPT and Work storage paths, filenames, schema, and local/cloud split
  on Linux.
- Whether adding ChatGPT context to Codex copies messages, attaches a reference,
  creates a summary, or uses another representation.
- The mapping between documented features and the installed desktop build.
- Preservation of attachments, tool call linkage, subagents, and timestamps.

No cache directory, database table, export schema, or native transcript path is
asserted here without direct evidence.

### Confidence

**Medium overall:** official documentation supports the product behavior;
native persistence and end-to-end readability remain unknown.

### Risk

**Medium:** incomplete coverage if desktop integration is presented as support
for all ChatGPT conversations. No parser breakage was reproduced in this watch.

### Inspect next

- [Source resolution](../../crates/core/src/session_sources.rs).
- [Session model and assistant paths](../../crates/core/src/models/session.rs).
- [Codex format reference and external-agent import persistence](codex.md#chatgpt-desktop-linux-and-imported-conversations).
- [Codex parser](../../crates/core/src/parsers/codex.rs).
- [Existing Codex fixtures](../../tests/fixtures/codex_sessions/).

### Recommended action

Capture and compare anonymized examples of:

1. A native Codex conversation created in the desktop application.
2. A conversation imported from an external AI assistant.
3. A native ChatGPT conversation added to a Codex conversation.

Record the desktop build and the before/after storage differences. Compare
message completeness, timestamps, tool call linkage, provenance, and repeated
imports before adding fixtures or compatibility tests.

The public Codex external-agent importer uses legacy Codex persistence in the
source inspected during the watch; see the [Codex reference](codex.md#confirmed-external-agent-import-persistence)
for its evidence and limits. That finding does not establish the native ChatGPT
storage format or the ChatGPT-to-Codex context-transfer mechanism.

**No native ChatGPT parser change justified yet.** Keep this source exploratory
until concrete persistence evidence establishes a readable format.
