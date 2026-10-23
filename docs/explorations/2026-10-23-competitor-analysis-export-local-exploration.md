# Exploration — Competitor Analysis: Local Export

Date: 2026-10-23  
Status: product research (explorations)  
Question: how do competing projects approach the "Local Export" feature  
(exporting one or more sessions as Markdown, HTML, JSON, PDF, with a secret  
redaction step), and what should Sessions Chronicle conclude from it?

Internal context:

- `docs/SIMILAR_PROJECTS.md` — competitor shortlist.
- `docs/IDEAS.md` ("Import, export, and storytelling") — export is already listed:
  "Export selected sessions as Markdown, HTML, JSON, Obsidian, or Logseq, with a
  privacy-first redaction preview".
- `docs/product-assessments/2026-06-21-product-assessment.md` — recommends P2:
  "Lightweight export/share", insisting on: local, privacy-first, secret redaction,
  tool timeline and metadata, **no** cloud publishing product.
- Product guardrails (`docs/IDEAS.md`): local-first, privacy-first, Linux-native,
  no team sync, no server mode, no generic PKM layer.

Method: web research (product docs, READMEs, changelogs, GitHub issues, Reddit,  
blogs) for the 10 competitors on the shortlist, supplemented by actors discovered  
along the way (ectype, Claudoscope, claude-replay, agento, alexi, and the  
first-party features of Claude Code, OpenCode, Hermes, and Cursor). Raw research  
results are kept in the session scratchpad (`competitor-export-research/raw/`).

---

## 1. Executive summary

- **Session export is becoming table stakes** in this category: 6 of the 10  
  competitors studied already offer it (AgentsView, cass, Agent Sessions, Claude Code  
  History Viewer, SpecStory, claude-replay), and user demand is visible and recurring  
  (issues `agent-sessions#29`, `alexi#2070`, `agento#591/592`,  
  `anthropics/claude-code#40981`, Reddit threads "How do you share your Claude code  
  conversations?").
- **Two formats dominate overwhelmingly**: **Markdown** (interoperable, versionable,  
  readable by Obsidian/Logseq/another agent) and **standalone HTML** (human sharing,  
  readable in a browser, offline). JSON/JSONL serves tooling. **PDF is almost absent  
  from coding sessions** — it belongs to the web-conversation segment  
  (ChatGPT/Claude.ai), not ours.
- **Redaction is the most interesting gap.** Few GUI competitors do it, and none do  
  better than a binary flag without a preview. The only serious actor on "redaction  
  preview" is **ectype** (CLI/web): machine-derived rules, custom terms, match list,  
  exact export preview, token budget. First-party actors (Cursor, Hermes, OpenCode)  
  do redaction **without a preview** (`--redact`/`--sanitize` flag or silent  
  "best-effort" redaction). **Nobody combines** multi-assistant index +  
  multi-session selection + redaction preview + multi-format export in a native GUI.  
  That is the place to take.
- **Hosted sharing is the norm elsewhere** (Nimbalyst "Share Link", OpenCode  
  `opncd.ai/s/…`, Claude Code "session sharing", SpecStory Cloud) — with E2E  
  encryption on Nimbalyst's side. Sessions Chronicle must **refuse this terrain**  
  (local-first guardrails) and turn it into an argument: "the export stays on your  
  machine, the redaction is verified beforehand".
- **The "context bundle"** (exporting a condensed summary reusable in a new session)  
  is a real need but today served by handcrafted skills/plugins (context-transfer,  
  /handoff-context, claude-handover) and by cass/ectype on the CLI side. It is a  
  natural V2 for us, not a V1.

---

## 2. Comparative matrix

Legend: ● = yes / complete, ◐ = partial or repurposed, ○ = no or not found.

### 2.1 Direct competitors (shortlist)

| Project | Session export | Output formats | Secret redaction | Preview before export | Multi-sessions | Replay / storytelling | Hosted sharing |
|---|---|---|---|---|---|---|---|
| **AgentsView** (web/desktop Go) | ● (UI `e` + menu, CLI `agentsview export sessions`) | Standalone offline HTML, Markdown (endpoint `/api/v1/sessions/{id}/md`), CSV (analytics), JSON/NDJSON (metadata only) | ○ at export — but ◐ detection: `agentsview secrets scan/list` (results masked by default) | ○ (Normal/Focused toggle **inside** the exported HTML, recipient side) | ○ (current session; CLI = metadata) | ○ | ◐ session share links in shared PostgreSQL deployment |
| **cass** (CLI/TUI Rust) | ● (TUI: `Ctrl+E` modal, `Ctrl+Shift+E`, Export tab `e`/`h`/`m` ; CLI `cass export`, `cass export-html`) | Markdown, standalone HTML, JSON (`--include-tools`) ; **AES-256-GCM encrypted** HTML (`--encrypt`) as option | ○ (encryption replaces redaction) | ○ | ○ (one file at a time) | ◐ rich HTML: themes, collapsible tool calls, integrated search, print-friendly | ○ |
| **Agent Sessions** (macOS Swift) | ● (transcript toolbar) | "Human-readable" Markdown (metadata, sections by role, collapsible tool details, images → `assets/` folder) | ○ | ○ | ○ | ○ | ○ |
| **Claude Code History Viewer** (Tauri) | ● (in-app + headless flag `--export <id> --format html\|json`) | HTML, Markdown (comrak), JSON ; "Full Backup" Archive Manager (all sessions copied) | ○ | ○ | ◐ (global backup only, no selection) | ○ | ○ |
| **CCManager** (CLI Go) | ○ (not found; no "export" issue) | — | — | — | — | — | — |
| **Nimbalyst** (Electron) | ◐ (no transcript file export; PDF export of markdown **documents**) | PDF (docs), "Share Link to a Session" (hosted) | ○ (warning: "Do not share sessions containing sensitive information") | ○ | ○ | ○ | ● E2E encrypted, key in URL fragment, expiration, "Unshare" |
| **SpecStory** (VS Code extension + CLI Go) | ● (auto capture + manual export) | Local Markdown (`.specstory/history/`), export/combine into a single Markdown | ○ | ○ | ● (combine multiple conversations) | ○ | ◐ SpecStory Cloud (sync + share, optional) |
| **claude-tap** (Python proxy) | ◐ export of API **traces** (local viewer/export ; "capture-only prompt export" reused by Phistory) | Trace JSON | ○ | ○ | — | ○ | ○ (local only, by design) |
| **claude-historian-mcp** (MCP TS) | ○ (no file export; **feeds context** to a new Claude session) | — (returns context to the agent) | — | — | ● (entire history queryable) | ○ | ○ |
| **claude-replay** (CLI) | ● | **Interactive** standalone HTML (video-like replay, chapters, themes, `--turns`, `--serve --watch`) | ○ | ○ | ○ | ● (it is the product) | ◐ (HTML file to self-host) |

### 2.2 Actors discovered during research (relevant)

| Project | Session export | Formats | Redaction | Preview | Notes |
|---|---|---|---|---|---|
| **ectype** (CLI + local web UI, Python) | ● `ectype export <id>` | text, markdown, html, json, jsonl, csv **+ agent-native resumable format** ; `convert` cross-agent | ● `--redact`: machine rules (home, media, username, hostname, emails, API key shapes, IP, timezone) + `--redact-term` custom ; **match report** (`home×6, media×62, email×1`) | ● exact export preview + **token budget** (max context vs "selected") | Redaction/preview reference; CLI, one session at a time |
| **Claudoscope** (macOS) | ○ | — | ◐ real-time **detection** of secrets (API keys, tokens) in sessions | — | Born from a realization: "I found my database password in a Claude Code session file" |
| **obsidian-opencode** (Obsidian plugin) | ● | Structured Markdown in Obsidian vault (metadata, model usage, costs) | ○ | ○ | Proof of Obsidian demand |
| **agento** (web viewer) | ● (PR #592, list context menu + detail toolbar) | Markdown / JSONL / TXT, with inline disclosure panel (format choice + scope checkboxes: reasoning, tool calls, tool results, system messages, metadata header) | ○ | ◐ (scope checkboxes, no render preview) | Emerging competitor adding export |
| **alexi** (viewer) | ◐ requested (issue #2070) | Markdown | — | — | Demand signal |
| **openchamber** (viewer) | ◐ requested (issue #5231) | Markdown (default) / standalone HTML / JSON | — | — | Demand signal: "human-readable export" |

### 2.3 First-party references (the assistants themselves)

| Assistant | Export | Redaction | Sharing |
|---|---|---|---|
| **Claude Code** | `/export` → **plain text** file or clipboard (no Markdown, no redaction) | ○ | ● "session sharing" by link (Feb. 2026, hosted claude.ai, web/desktop/mobile) ; team sharing request: issue #40981 |
| **OpenCode** | `opencode export <id>` ; `opencode import` (file **or URL** `opncd.ai/s/…`) | ● `--sanitize` "redact sensitive transcript/file data" | ● hosted URL sharing |
| **Hermes Agent** | `/save json\|md\|html` (CLI, TUI, messaging) ; `hermes sessions export --format html\|jsonl\|md\|qmd`, filters (`--only user-prompts`, `--older-than`, `--model`…) | ● `--redact` "scrubs secrets from message content and tool output" ; **snapshotted** regex redaction (a compromised model cannot disable it mid-session) | ○ |
| **Cursor** | "Shared transcripts" (conversation links) | ◐ "best-effort" redaction of known secret patterns (API keys, tokens, passwords), on the sharing side | ● hosted |

### 2.4 User demand signals (selection)

- Reddit r/ClaudeCode "How do you share your Claude code conversations?" : the
  dominant workflow is "`jq` to filter + a small formatter to get Markdown" — the
  demand is **real and poorly served**.
- Reddit r/ClaudeCode "I made a tool to export claude code transcripts nicely for
  sharing" → **clawport** (Markdown/HTML) ; "I created a script that exports
  Claude Code chat sessions in markdown or XML" (16 votes).
- Export requests in competing viewers' issues: `agent-sessions#29` (implemented),
  `alexi#2070` (Markdown), `agento#591` → PR #592 (Markdown/JSONL/TXT, context menu
  + toolbar), `openchamber#5231` ("human-readable export" Markdown/HTML/JSON with
  inline subagent history), `anthropics/claude-code#40981` (team sharing).
- Dedicated tools proving the need: **claude-code-log** (HTML/MD/JSON), **cctx**
  ("committable" Markdown), **Claude-Export** (project/branch/duration/model/modified
  files header + table of contents + timestamped sections), **claude-code-share**
  (hosted links), **claude-to-markdown** (browser extension).
- Obsidian/Logseq demand: r/ObsidianMD and r/logseq threads ("Claude Code + Obsidian
  — how I use it"), XWX Exporter (sync Claude → Obsidian vault in Markdown).
- "Context bundle" / handoff demand: handcrafted skills and plugins
  (context-transfer, /handoff-context, claude-handover, "handoff bundle").
- Privacy side: issue `anthropics/claude-code#29434` (context window redaction),
  **ingestion-time redaction** tools (claude-code-redact, cc-redact, PrivacyScrubber),
  Prism article "How to Redact Sensitive Information From AI Conversations Before
  Sharing" — the concern is mainstream.

---

## 3. Per-competitor profiles

### 3.1 AgentsView (kenn-io/agentsview) — the breadth benchmark

**Approach.** Two distinct export surfaces: (1) on the UI side, `e` or the session  
header export menu downloads a **standalone HTML** (styled render, offline, 12 syntax  
highlighting languages, collapsible tool calls) with a **Normal/Focused** toggle  
embedded in the document — the recipient switches to "user prompts + final answers"  
mode themselves without re-exporting; the same menu offers "Copy markdown export link"  
(endpoint `/api/v1/sessions/{id}/md`, "particularly suited for passing context to  
another agent") and "Copy source file path". Subagent inclusion depth via `?depth=`.  
(2) On the CLI side, `agentsview export sessions --format json|ndjson` exports  
**metadata + token/cost totals only** (no transcript text) for scripting, and  
"Export CSV" on the dashboard side for analytics. In parallel, `agentsview secrets  
scan/list` detects secrets in the archive (results masked by default, `--reveal`  
local only).

**Strengths.** Truly standalone and offline HTML ; the Normal/Focused toggle deferred  
to the reader is an elegant, low-cost UX idea ; Markdown endpoint designed for  
agent-to-agent handoff ; secret detection as a separate building block.

**Weaknesses.** No redaction at export ; no multi-session selection ; no preview ;  
link sharing assumes a shared PostgreSQL deployment (contradicts strict local-first).

### 3.2 cass (Dicklesworthstone/coding_agent_session_search) — the power-user benchmark

**Approach.** Native, unapologetic export: **Export** tab in the detail panel  
(`e`/`h` = HTML modal, `m` = immediate Markdown), shortcuts `Ctrl+E` /  
`Ctrl+Shift+E`, and CLI: `cass export <file> --format markdown|html|json  
--include-tools` and `cass export-html` with **optional AES-256-GCM encryption**  
(PBKDF2 600,000 iterations, password via stdin), namable output (`--output-dir`,  
`--filename`), `--open`. The HTML is mostly standalone (inline CSS, Prism.js via CDN  
with SRI + graceful offline degradation), dark/light themes, collapsible tool calls,  
integrated search, print styles. The Markdown is explicitly "suited for handing  
session context to another agent".

**Strengths.** Covers CLI + TUI ; export encryption is an original privacy-first  
response ; golden tests lock the export JSON contract.

**Weaknesses.** No redaction (encryption protects confidentiality, not the integrity  
of the shared narrative) ; one session at a time ; no preview.

### 3.3 Agent Sessions (jazzyalex/agent-sessions) — the closest native competitor

**Approach.** **Markdown** export from the transcript toolbar: "human-readable"  
document with session metadata, sections by role, collapsible tool details ; embedded  
images are written to a sibling `assets/` folder. The founding issue  
`agent-sessions#29` ("Export and share session transcript") was closed by this  
feature (v3.9.3+). The app remains local-first, without telemetry.

**Strengths.** Polished native UX (SwiftUI) ; one-click export from reading view ;  
Markdown format directly integrable into a project doc.

**Weaknesses.** Markdown only ; no redaction or preview ; one session at a time ;  
macOS-only (leaves the GNOME/Linux field to us).

### 3.4 Claude Code History Viewer (jhlee0409) — the cross-platform "default viewer"

**Approach.** In-app export (HTML + Markdown via comrak ; the HTML render lives in  
`src-tauri/src/export.rs`, a Rust port of `src/services/export/`), headless flag  
`--export <session-id|path> --format html|json [--output]` for SSH/CI, and an  
**Archive Manager** with a "Full Backup" card that copies all sessions of all  
Claude Code projects in one action (anti-cleanup). Auto-generated titles from Claude  
Code appear in exports.

**Strengths.** Covers GUI + headless ; full backup addresses the fear of losing  
history ; subagent export fix (issue #433) shows a living feature.

**Weaknesses.** No granular multi-session selection (all or nothing) ; no redaction ;  
no preview.

### 3.5 CCManager (kbwo/ccmanager) — the CLI control plane

**Approach.** No export feature identified (web search + GitHub issues empty on the  
subject). The product is operational: worktrees, notifications, session restoration,  
Claude/Codex plugins.

**Takeaway.** A CLI manager does not need transcript export to exist ; export demand  
lives on the "viewer" side. Do not over-invest in a CLI positioning for Sessions  
Chronicle.

### 3.6 Nimbalyst (nimbalyst) — the workbench betting on hosted sharing

**Approach.** No transcript file export. Instead: **"Share Link to a Session"** —  
public hosted link, **E2E encrypted client-side**, decryption key in the URL fragment  
(never sent to the server), chosen expiration, "Unshare" revokes immediately ;  
explicit warning: "Anyone with the link can view the shared content. Do not share  
sessions containing sensitive information." The workbench's Markdown documents export  
as **PDF** (title + generated outline). The product also has a hosted Teams/console  
offering.

**Strengths.** Frictionless sharing for collaboration ; polished E2E.

**Weaknesses.** This is exactly what our guardrails forbid (cloud, teams, hosting) ;  
no redaction ; sharing is the product, not export. **Takeaway for us**: the market  
exists for sharing, but Sessions Chronicle can capture it through **file** output  
(Markdown/HTML) + redaction, without ever hosting anything.

### 3.7 SpecStory (specstoryai/getspecstory) — auto-capture in Markdown

**Approach.** The extension/CLI **continuously writes** every conversation as  
Markdown into `.specstory/history/` (without rewriting the editor's buffer) ; one  
can export separately or **combine multiple conversations into a single Markdown**,  
reference the file in the next session to "carry the context forward", or commit it  
with the code. SpecStory Cloud (optional, paid) adds cross-project search and sharing.

**Strengths.** Zero friction (auto capture) ; local Markdown is the "smallest common  
denominator" (vault, git, another agent) ; combines multi-conversations.

**Weaknesses.** No redaction ; no HTML ; sharing goes through the cloud ; IDE-oriented  
(VS Code/Cursor), not native desktop.

### 3.8 claude-tap (liaohch3/claude-tap) — the API forensics, not the transcript

**Approach.** Local proxy that intercepts agents' API traffic and displays traces  
(requests, tool schemas, streaming, tokens, diffs) in a local viewer ; **exports  
traces locally** without a hosted dashboard. Phistory reuses its "capture-only prompt  
export" to archive system prompts.

**Takeaway.** Export at claude-tap serves **debugging**, not human sharing. Sessions  
Chronicle should not follow it on this terrain (out of scope), but the lesson is that  
"local export without leakage" is a marketing argument in itself.

### 3.9 claude-historian-mcp (Vvkmnn) — the context feeder

**Approach.** MCP server that lets Claude **search and retrieve** its own conversation  
history (fast search, priorities, file tracking). No file export : the output is  
context for the agent.

**Takeaway.** This is proof that "putting history back into a new session" is a  
mature need — served here by MCP. For us, the GUI equivalent is the **context  
bundle** (see §6).

### 3.10 claude-replay (es617) — storytelling in HTML

**Approach.** CLI that converts logs (Claude Code, Cursor, Codex, Gemini, OpenCode,  
Kimi, Hermes) into **interactive HTML replays** : video-like playback, chapters  
(`--mark`), turn ranges (`--turns 5-15`), themes, `--serve --watch` for live.  
Positioned for "blog posts, demos, documentation".

**Strengths.** The only true session "storytelling" product ; strong Reddit traction  
(755★). Validates the demand to "tell" a session.

**Weaknesses.** Single-purpose (replay) ; no redaction ; no link to a durable archive.  
**Takeaway**: a native GNOME "presentation/replay" mode is a credible V2 for us  
(already in `docs/IDEAS.md`), but it is not the V1.

### 3.11 ectype (chaybits) — the redaction/preview reference (discovered in research)

**Approach.** CLI + local web UI: `ectype export <id> --mode brief|custom|full  
--format text|markdown|html|json|jsonl|csv --cap N --redact -o file`. Redaction  
(`--redact`) applies **machine-derived rules** (home, media paths, username, hostname,  
emails, API key shapes, IP, timezone) + custom terms (`--redact-term  
TEXT=REPLACEMENT`) ; each run **reports what matched** (`home×6, media×62, email×1`)  
and the web UI shows each value next to its replacement. The preview shows  
**exactly** what the export will contain, with a **token budget** ("max context" vs  
"selected", gauge compared to the target model's window). The export starts with a  
header (session, project, model, turn count) and inline-marks any truncated content.  
`ectype convert --install` can rewrite a session into an agent's store to **summarize**  
it. Assumed warning: "Redaction is best effort, not a guarantee. Read an export before  
you share it."

**Strengths.** The only one making redaction a first-class citizen with preview +  
report + budget. Explicit honesty about limits.

**Weaknesses.** CLI/web, not a native desktop GUI ; one session at a time ; learning  
curve (modes, caps).

### 3.12 First-party: what the assistants themselves do

- **Claude Code**: `/export` = plain text or clipboard (no Markdown, no redaction) ;
  hosted link sharing since February 2026. The redaction demand exists for them too  
  (issue #29434: the context window is "append-only", secrets stay in it).
- **OpenCode**: `export --sanitize` (redaction) + `import` from file **or hosted  
  sharing URL** (`opncd.ai/s/…`).
- **Hermes**: `/save json|md|html` with `--redact` ; `sessions export` with rich  
  filters and `--only user-prompts` ; snapshotted regex redaction at startup.
- **Cursor**: shared transcripts with "best-effort" redaction of known patterns,  
  **without preview or informing the user of what was masked**.

**Takeaway.** Even first-party actors do redaction **without preview**. The bar is  
low ; clearing it costs little and differentiates.

---

## 4. Export formats: which are relevant, and why

Ranking by observed frequency × fit to our use cases (bug report, PR note, personal  
archive, sharing with a colleague, reuse in a new session):

| Rank | Format | Demand evidence | Why relevant for Sessions Chronicle |
|---|---|---|---|
| 1 | **Markdown** | SpecStory, cass, Agent Sessions, Hermes, cctx, Claude-Export, clawport, claude-code-log, Reddit threads ("jq + formatter → markdown") | Interoperable: Obsidian/Logseq vault, git, commit with code, **paste into a new agent session** (context bundle). Also the format of `docs/IDEAS.md`. |
| 2 | **Standalone HTML** | AgentsView, cass, CCHV, Hermes, clawport, claude-code-log, claude-replay | Human sharing "readable in a browser", offline, styled, syntax highlighting, collapsible tool calls. The de facto standard for "send the session to someone". |
| 3 | **JSON / JSONL** | cass, CCHV, Hermes, ectype, SpecStory (cloud) | Tooling, reimport, faithful archiving. Not sexy, but expected for the exportable "source of truth". |
| 4 | **"Condensed" Markdown (context bundle)** | cass ("handing session context to another agent"), ectype (`recall`, token budget), SpecStory ("reference the file in your next session"), handoff skills | Distinct use case: reuse the session as **context** for a new session, not as an archive. |
| 5 | **CSV** | AgentsView (analytics only) | Useful for **metrics** export, not transcripts. Separate from session export. |
| 6 | **PDF** | Nimbalyst (docs), saveai.net / fromaitopdf / HonestPDF / XWX (web-conversation segment) | Almost no demand for coding sessions ; heavy to generate cleanly (GTK print engine). **Backlog**, not V1. |
| — | **Encrypted HTML** (cass `--encrypt`) | cass | "Privacy" alternative to redaction ; possible V2 complement for sharing sensitive sessions. |
| — | **QMD** | Hermes | Hermes-proprietary Markdown variant ; no cross-assistant demand. Ignore. |
| — | **Native resumable format** (ectype `convert --install`) | ectype | "Export to resume elsewhere" = cross-agent conversion, not readable export. Out of scope V1 (and risky: ectype documents it as non-faithful). |

**Obsidian / Logseq** are not formats: they are **Markdown destinations** (obsidian-opencode  
plugin, XWX Exporter, Reddit threads). Producing clean Markdown + optional YAML  
frontmatter is enough to cover this case.

---

## 5. Identified UX patterns

### 5.1 Selection (what to export)

- **Per session, from the reading view**: button in the transcript toolbar (Agent  
  Sessions), menu/shortcut `e` in the detail header (AgentsView), Export tab in the  
  detail panel (cass), per-session export button (CCHV, agento). This is the dominant  
  pattern.
- **Batch**: almost nonexistent as granular selection. CCHV does "Full Backup" (all or  
  nothing) ; SpecStory combines chosen conversations ; Hermes filters via CLI  
  (`--older-than`, `--model`, `--workspace`, `--only user-prompts`).
- **Headless/CI**: CCHV (`--export`), cass (`export`, `export-html`), AgentsView  
  (`export sessions`) — useful for a GNOME desktop product? No, but good for a future  
  optional CLI flag.
- **Identified gap**: **multi-session selection in a list** (checkboxes + "export  
  selection") and **"export the filtered view"** (project, date range, assistant).  
  Nobody does it properly. It is ours.

### 5.2 Redaction and privacy

- **Binary flag without preview** (Hermes `--redact`, OpenCode `--sanitize`): simple,  
  but the user does not know what was masked.
- **Silent redaction on the sharing side** (Cursor "best-effort"): convenient, opaque.
- **Detection separate from export** (AgentsView `secrets scan/list`, Claudoscope  
  "secrets detection"): detection lives in the session browser, not in the export  
  flow. The user has to make the connection themselves.
- **Redaction + preview + report** (ectype): machine rules + custom terms, match list  
  with replacements, exact preview, "best effort, read before you share" warning.  
  **Best known pattern ; no native GUI has it.**
- **Encryption instead of redaction** (cass `--encrypt`): protects content at rest/in  
  transit, but the recipient decrypts everything — not a redaction.
- **Warning without a tool** (Nimbalyst: "do not share sensitive content"): the worst  
  answer, and yet frequent.
- **Ingestion-time redaction** (claude-code-redact, cc-redact, PrivacyScrubber):  
  upstream prevention (hooks/proxy), complementary but out of export scope.

### 5.3 Preview / export control

- **Normal/Focused toggle embedded in the exported HTML** (AgentsView): the recipient  
  chooses reading density without re-export. Low cost, very clever.
- **Token budget + exact preview** (ectype): "selected" vs "max context", gauge  
  compared to the target window. Specific to the "paste into an agent" case.
- **Systematic header + inline truncation marking** (ectype, Claude-Export): an export  
  must say what it contains and what it omitted.
- **Scope checkboxes in an inline disclosure panel** (agento, PR #592): format  
  (Markdown/JSONL/TXT) + scope (reasoning, tool calls, tool results, system messages,  
  metadata header) presented before export, without a dedicated modal.
- **Images in a sibling folder** (Agent Sessions) vs embedded images (cass): the  
  `assets/` folder keeps Markdown lightweight and versionable.

### 5.4 Destination

- Browser download (AgentsView), configurable folder + naming (cass  
  `--output-dir/--filename`), clipboard (Claude Code `/export`), **agent store for  
  resume** (ectype), Obsidian vault (plugin), hosting (Nimbalyst, OpenCode, Claude  
  Code, SpecStory Cloud). For a GNOME app: **native GTK file/folder selection dialog**  
  (`GtkFileDialog` pattern), never an "upload".

---

## 6. Concrete recommendations for Sessions Chronicle

Aligned with the product assessment's P2 ("export selected session as HTML/Markdown,  
redact obvious secrets, include tool timeline and metadata, local only") and with  
`docs/IDEAS.md`. The repo has **no** export code today (verified: only `src/lib.rs`  
mentions the Rust keyword `export`).

### 6.1 V1 — Local export of one session, with redaction preview (the differentiator)

1. **Entry point**: "Export" button in the transcript view header + keyboard shortcut  
   + action in the session list row context menu.
2. **Export dialog** (`AdwDialog`) in three blocks:
   - **Format**: Markdown / standalone HTML / JSON (three-option selector ; Markdown  
     by default).
   - **Content**: full transcript / "Focused" (prompts + final answers, tool calls  
     collapsed) — AgentsView's Normal/Focused, but chosen **before** export, and  
     embedded as a toggle in the HTML.
   - **Redaction**: "Secret redaction" checkbox (enabled by default) + **preview  
     panel**: list of found matches (type, count, location, masked value), rules =  
     API key/token/credential shapes + local paths (home, username) + custom terms ;  
     each rule can be disabled ; "best-effort, read before you share" warning  
     (ectype tone).
3. **Destination**: folder selector + filename template (`{project}-{date}-{title}.md`),  
   overwrite confirmed.
4. **Generation**: from the SQLite index + streamed re-read of the source JSONL  
   (BufReader, cf. AGENTS.md guardrails), never a full in-memory load.

### 6.2 V1.1 — Multi-sessions

5. **Multi-selection** in the session list (checkboxes, GNOME `AdwListBox` + selection  
   pattern) → "Export selection": one file per session in a chosen folder, **or** a  
   combined Markdown (chapter per session, with metadata: assistant, project, date,  
   model, costs).
6. **"Export filtered view"**: applies the export to the currently filtered list  
   (project / assistant / date range) — cass and CCHV do neither.

### 6.3 V1.2 — Context bundle

7. **"Copy as context" / "Export as bundle"**: condensed Markdown (prompts + decisions  
   + tool calls collapsed to N tokens), designed to be pasted into a new session ;  
   display the **estimated token budget** (ectype model, without the fancy gauge). It  
   is the GUI counterpart of claude-historian-mcp, and it consumes cass/ectype's  
   Markdown output on their own terrain.

### 6.4 Backlog (do not do in V1)

8. **PDF**: no demand for coding sessions ; revisit if a "printable report" use case  
   emerges.
9. **Password-encrypted HTML** (cass `--encrypt` pattern): privacy complement for  
   sharing sensitive sessions ; V2.
10. **Presentation / replay mode** (storytelling): cf. claude-replay, Mindwalk, Agent  
    Flow ; separate V2+, already in `docs/IDEAS.md`.
11. **Hosted sharing / links / teams**: **explicitly excluded** by the guardrails.  
    Never.
12. **CSV metrics export**: separate feature (analytics), not "session export".

### 6.5 Export dialog mockup (V1)

```
┌───────────────────────────────────────────────────────┐
│  Export session                                   [✕]  │
├───────────────────────────────────────────────────────┤
│  Format          (●) Markdown  ( ) HTML  ( ) JSON     │
│  Content         (●) Full       ( ) Focused           │
│                                                      │
│  ☑ Secret redaction                                  │
│    ┌─ Redaction preview (12 matches) ─────────────┐  │
│    │ [API key]   sk-ant-••••  ×4   ▸ message 12   │  │
│    │ [path]      /home/alex  ×6   ▸ Bash tool    │  │
│    │ [email]     a•••@•••.co  ×1  ▸ message 3     │  │
│    │ [+] Add a term…                  [See all]    │  │
│    └──────────────────────────────────────────────┘  │
│    "Best-effort: read the export before sharing."   │  │
│                                                      │
│  Folder   ~/Documents/exports            [Browse]    │
│  File     sessions-chronicle-{date}-{title}.md       │
│                                                      │
│              [Cancel]          [Export]              │
└───────────────────────────────────────────────────────┘
```

Sidebar (V1.1): multi-selection makes "Export (N sessions)" appear in the list toolbar.

---

## 7. Differentiation opportunities

1. **Redaction preview in native GNOME.** No GUI competitor (AgentsView, Agent  
   Sessions, CCHV, Nimbalyst, Claudoscope, agento) shows what will be masked before  
   export. ectype does it in CLI/web. We would be the only ones to do it in a native  
   desktop GUI, multi-assistant. That is exactly the "privacy-first" intersection of  
   our positioning.
2. **Multi-assistant + multi-sessions.** A readable, cleaned export, whatever the  
   source (Claude Code, OpenCode, Codex, Vibe, Kimi), with multi-selection or filtered  
   view — a combination that does not exist elsewhere.
3. **Local-first asserted as a feature, not a constraint.** The "share link" market is  
   taken (Nimbalyst, OpenCode, Claude Code, SpecStory Cloud, Cursor). We offer  
   **controlled file output** (Markdown for vault/git, standalone HTML for the  
   colleague) with redaction as proof of trust. Message: "what you share is yours, and  
   you know what it contains".
4. **Focused mode from the export.** AgentsView's Normal/Focused toggle is good ;  
   offering it **at export time** (and in the HTML) covers the "tell the story without  
   diagnostic noise" case — a first step toward storytelling without the cost of a  
   replay mode.
5. **Context bundle as a GUI product.** claude-historian-mcp (MCP), handoff skills,  
   cass/ectype (CLI) serve the "resume elsewhere with context" need in a handcrafted  
   way. A "copy as context" with a token budget is a differentiating, native use case  
   for a session browser.
6. **Trust through transparency.** Systematic header (assistant, project, dates,  
   model, costs, turn count), inline marking of truncated/redacted content,  
   "best-effort" warning — the same honesty as ectype, in a mainstream GUI.

---

## 8. Alternatives considered and decision

| Option | Description | Advantages | Disadvantages | Verdict |
|---|---|---|---|---|
| **A — Minimal V1** (Markdown + HTML, no redaction) | Copy the baseline (Agent Sessions, CCHV) | Fast, familiar | No differentiation ; contradicts privacy-first ; redaction is already in `docs/IDEAS.md` and the P2 | Rejected |
| **B — V1 with redaction preview** (recommended) | §6.1: MD/HTML/JSON + match panel + machine/custom rules | Strong differentiator, local-only, faithful to P2 | Secret detection engine + preview UI to build | **Selected** |
| **C — Encrypted export** (cass `--encrypt` pattern) | Password-protected HTML, no redaction | Useful for very sensitive sessions | Does not replace redaction (recipient sees everything) ; crypto cost + password UX | **V2**, complement to B |
| **D — Hosted E2E sharing** (Nimbalyst/OpenCode pattern) | Share links | Growth/virality | **Excluded** by guardrails (no cloud, no team sync) | Rejected |

**Decision.** Build **option B** in V1: local export of one session as Markdown /  
standalone HTML / JSON, with a **redaction preview** panel (machine + custom rules,  
listed matches, assumed "best-effort"), native local destination. V1.1: multi-session  
selection and filtered-view export. V1.2: context bundle with token budget. PDF,  
encrypted HTML, and replay mode stay in backlog. No form of hosted sharing, ever.

**Rationale.** The competition converges on "export as Markdown/HTML" (table stakes),  
but redaction there is either absent, a blind flag, or delegated to encryption. Our  
privacy-first + native GNOME + multi-assistant index positioning makes "redaction  
preview" both cheap to differentiate and directly aligned with the product assessment's  
P2 and `docs/IDEAS.md`. Multi-sessions and the context bundle are the two extensions  
that naturally extend the existing SQLite index, without a new server or any data  
leaving the machine.

---

## 9. Main sources

- AgentsView — docs "Session Export", "Usage Guide", "Session API", "CLI Reference"  
  (agentsview.io/docs) ; issue #482 (Focused mode export).
- cass — README + SKILL.md  
  (github.com/Dicklesworthstone/coding_agent_session_search) : `cass export  
  --format markdown|html|json`, `cass export-html --encrypt`.
- Agent Sessions — release v3.9.3 (newreleases.io / GitHub) ; issue #29 "Export and  
  share session transcript".
- Claude Code History Viewer — README + CLAUDE.md (jhlee0409) : `--export  
  --format html|json`, Archive Manager "Full Backup", `src-tauri/src/export.rs` ;  
  issue #433.
- CCManager — README + issues (kbwo/ccmanager) : no export identified.
- Nimbalyst — docs "Share Link to a Session" (docs.nimbalyst.com) ; features page.
- SpecStory — docs.specstory.com (quickstart, features) ; specstory.com.
- claude-tap — README (liaohch3/claude-tap).
- claude-historian-mcp — README (Vvkmnn/claude-historian-mcp).
- claude-replay — README, es617 blog, Reddit/DEV posts (es617/claude-replay).
- ectype — README + releases (chaybits/ectype) : `--redact`, preview, token budget.
- Claudoscope — claudoscope.com, blog "I found my database password in a Claude Code  
  session file" (beardedeagle/Claudoscope, cordwainersmith/Claudoscope).
- obsidian-opencode — DeepWiki "Session Export to Markdown" (kriss-spy).
- agento — issue #591 / PR #592 (shaharia-lab/agento) ; alexi issue #2070 ;  
  openchamber issue #5231.
- First-party: kentgigger.com "How to export, save, and share a Claude Code  
  conversation" ; blog vincentqiao.com (`/export`) ; alooks.ai and lore.link (session  
  sharing) ; opencode.ai docs CLI + mager.co (OpenCode `export --sanitize`,  
  `import`) ; hermes-agent-lab.com `/save` + nyk.dev (Hermes `sessions export  
  --redact`, `agent/redact.py`, PR #60186) ; cursor.com/help "Shared transcripts".
- User demand: Reddit r/ClaudeCode (conversation sharing, clawport, markdown/XML  
  script), r/ClaudeAI (PDF/Markdown export), r/ObsidianMD and r/logseq (vault),  
  issues anthropics/claude-code #40981 (team sharing) and #29434 (context window  
  redaction).
- Redaction ecosystem: Prism "Redact sensitive info from AI conversations" ;  
  paroque28/claude-code-redact ; ShindouMihou/cc-redact ; privacyscrubber.com.
- Dedicated export tools: daaain/claude-code-log,  
  alexgs/claude-code-transcript-export (cctx), gsanders300/Claude-Export,  
  alexspeller/clawport, wsxiaoys/claude-code-share, legoktm/claude-to-markdown, XWX  
  Exporter, saveai.net, fromaitopdf.com.
- Handoff context: mcpmarket context-transfer, claudeplug.com /handoff-context,  
  heyitworks Claude-Handover.

*Freshness note: research conducted on 2026-10-23 ; star counts and versions are  
snapshots (see `docs/SIMILAR_PROJECTS.md` for the last shortlist review, 2026-08-04).*
