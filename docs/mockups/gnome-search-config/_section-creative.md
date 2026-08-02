# Proposal — Creative: "Show me what GNOME will see"

> **Correction note (2026-08-02).** This is the proposal as written on 2026-08-01. Its *argument* stands unchanged, but several codebase facts it cites were invalidated when [PR #200](https://github.com/supermaciz/sessions-chronicle/pull/200) shipped the deep-link prerequisite (#197). Corrected points are marked inline below; the authoritative current facts live in [`../../explorations/2026-08-01-gnome-search-configurability-exploration.md`](../../explorations/2026-08-01-gnome-search-configurability-exploration.md). The reasoning is preserved rather than rewritten so the record of what was argued, and on what basis, stays intact.

**Angle**: configuration by direct manipulation, not by abstract preference rows.  
**One-liner**: a single **exposure dial** whose control *is* a live replica of the Activities result row, plus **per-project opt-out that lives on the project**, not in a settings list.

---

## The reframe

Every other way of configuring this feature asks the user to read a sentence and predict a consequence:

> ☐ Show matched text in search results

To answer that honestly you have to already know what a GNOME Shell result row looks like, which of your transcript fields lands in `name` versus `description`, and how much of a matched snippet survives ellipsization at 500 px. Nobody knows that. So the switch gets flipped at random, or left at its default forever — which means the default is the whole design and the switch was decoration.

The only question that actually matters here is **what text from my private AI conversations appears on a screen I might be sharing**. That question has a picture-shaped answer. So make the control a picture.

**Exposure dial**: one row, three positions, and directly beneath it a **live mock Activities result** built from the user's own most recent indexed session, redrawn on every change. You do not read about the consequence, you look at it.

The second axis — *which sessions are eligible at all* — is not a preference either. It is a property of a project ("this client's work never leaves this app"). Properties of objects belong on the objects. So it goes in the project row's context menu in the sidebar, not in a checklist in Preferences.

---

## Control 1 — the exposure dial (Preferences ▸ General ▸ *GNOME Search*)

An `adw::ComboRow`, three values, each a coherent product rather than a checkbox combination:

| Value | `name` (result title) | `description` (result subtitle) | What leaks |
|---|---|---|---|
| **Project and date** | `sessions-chronicle` | `Claude Code · 3 days ago · 142 messages` | nothing the user typed |
| **First prompt** *(default)* | `Add a date range picker to the sidebar` | `Claude Code · sessions-chronicle · 3 days ago` | the opening prompt only |
| **Matched text** | `Add a date range picker to the sidebar` | `…rotate the SUPABASE_SERVICE_KEY before the demo…` | arbitrary transcript body |

Row title: **Result detail**  
Row subtitle: **What Sessions Chronicle shows in the Activities overview**

The three levels are ordered by exposure, and each is independently useful — *Project and date* is genuinely usable when you navigate by project, *Matched text* is genuinely better when you are hunting a half-remembered phrase. This is a dial, not a mute button, which is why it is a `ComboRow` and not a `SwitchRow`.

**Default is the middle position, not the safest one.** *Project and date* would make the feature fail its own acceptance criterion — "results appear with a recognizable title" — because three sessions in the same project on the same day are indistinguishable. The first prompt is text the user deliberately authored as the opening move of a session; it is the closest thing this app has to a document title, and treating it as one is defensible. Arbitrary matched transcript text is not, so it is opt-in.

### The preview is the point

Directly under the `ComboRow`, inside the same `PreferencesGroup`, sits a non-interactive `GtkBox` styled to match a Shell result row: 32 px icon, bold `name` line, dimmed single-line `description` line, ellipsized at the end. It is populated from the most recent indexed session — real data, the user's own — and rebuilt on `selected-notify`. No animation, no toast, no explanation text. The row changes; that is the explanation.

If the index is empty it falls back to a static placeholder session rather than an empty box, and the group gains a dimmed caption **"No sessions indexed yet — showing an example."**

Cost: one `GtkBox` of three labels and one query for the newest session, run once when the dialog opens. It is not a live search; it never touches FTS.

---

## Control 2 — per-project opt-out, on the project

`Exclude from GNOME search` as a checkable item in the project row's context menu in `src/ui/sidebar.rs`, and a small `system-search-symbolic` icon with a slash rendered as a dim suffix on excluded project rows so the state is visible without opening a menu.

This is **not** a GSettings key. "This project is confidential" is a durable fact about that project, it must survive re-index, and it must be queryable from SQL so the provider can apply it as a `WHERE` clause instead of filtering in Rust after the fact. It is a column:

```sql
-- schema v18 (CURRENT_DB_VERSION is 17 at src/database/schema.rs:5)
ALTER TABLE projects ADD COLUMN exclude_from_shell_search INTEGER NOT NULL DEFAULT 0;
```

The provider query becomes the existing `search_sessions_with_query` (`src/database/mod.rs:~455`) with one added join and clause:

```sql
LEFT JOIN projects p ON p.id = s.project_id
...
AND COALESCE(p.exclude_from_shell_search, 0) = 0
```

`LEFT JOIN` + `COALESCE` because `sessions.project_id` is `Option<i64>` (`src/models/session.rs:46`) — unresolved-project sessions stay searchable. `SELECT DISTINCT` is unnecessary; the existing dedup-by-`session.id` `HashSet` loop already handles the one-row-per-message-hit fan-out.

No new preference page enumerates the excluded projects. If you want to know which projects are excluded, you look at the sidebar, where the projects are.

---

## What is deliberately *not* configurable

- **On/off.** GNOME Settings ▸ Search owns it, the Flatpak `.ini` ships disabled, so the feature is opt-in by construction. A second in-app switch could disagree with the OS one and would have to explain which wins.
- **Result count.** The Shell caps the visible rows anyway; a user-facing "how many results" spinner configures something the user cannot observe.
- **Assistant / date filters for the provider.** These exist in-app because you are *browsing*. In Activities you are *retrieving a known item* — you already know what you are looking for, and pre-filtering by assistant can only hide it.
- **Ranking.** bm25 order, `s.last_updated DESC` tie-break, same as `search_sessions_with_query` today. There is no second ordering a user could ask for that beats relevance in a five-row list.

---

## Where the provider reads this

The provider is a separate D-Bus-activated entry point (`--gnome-search-provider`) in the same binary, not the GUI process — Shell must get answers when the app is closed, and it must not raise a window. It therefore reads:

- `gio::Settings::new(APP_ID).string("gnome-search-detail")` at the top of each `GetResultMetas` call. One GSettings read per request is free relative to the FTS query; no `changed` subscription is needed, because the provider process is short-lived and re-reads on every call anyway.
- `projects.exclude_from_shell_search` inline in the SQL, so an exclusion toggled in a running GUI takes effect on the very next keystroke in Activities with no IPC between the two processes. The database is the shared state.

Packaging deltas this proposal implies (shared with the other proposals, listed for completeness):

- ~~`data/dev.maciz.sessionschronicle.desktop.in.in` gains `DBusActivatable=true`.~~ **Shipped in #197** (2026-08-02), together with the application's per-profile D-Bus service file.
- A new `data/dev.maciz.sessionschronicle.search-provider.ini` installed to `datadir/gnome-shell/search-providers/`, basename prefixed with the app ID so Flatpak exports it.
- ~~`build-aux/dev.maciz.sessionschronicle.json` finish-args gain `--own-name=dev.maciz.sessionschronicle.SearchProvider`.~~ **Not needed** (2026-08-02): Flatpak grants an app `--own=$APP_ID` and `--own=$APP_ID.*`, so a sub-name requires no permission. The name must be generated per profile as `@APP_ID@.SearchProvider` — hardcoded, it is a *sibling* of the `.Devel` app ID rather than a sub-name, so it would not be granted for development builds.

---

## GSettings

```xml
<key name="gnome-search-detail" type="s">
  <default>"first-prompt"</default>
  <summary>Detail shown in GNOME Activities results</summary>
  <description>
    How much session content the GNOME Shell search provider reveals.
    Accepted values: project-date, first-prompt, matched-text.
  </description>
</key>
```

One key. Plus one DB column, `projects.exclude_from_shell_search`.

---

## Risks

- **The preview can lie.** It renders our approximation of a Shell result row, not the real thing; Shell's own truncation, icon size and font may differ per theme and per screen width. Mitigation: keep the mock deliberately plain and label the group **"Preview"** rather than implying pixel fidelity. If it drifts far enough to mislead, it is worse than no preview at all — this is the proposal's main failure mode.
- **`first-prompt` as the default still exposes user-authored text** on the overview. That is a real, accepted trade: the feature is unusable without a distinguishing title, and the dial makes stepping down one position a two-click operation once the user has *seen* what is exposed.
- **A DB column for a preference-shaped thing** requires a migration and a small write path from the sidebar. It is the right home for the data, but it is strictly more work than a GSettings string list of excluded project paths, and a `strv` key would ship faster. The argument for the column is correctness under re-index and the ability to filter in SQL.
- **Discoverability of Control 2.** Nothing in Preferences hints that per-project exclusion exists. Mitigation: the exposure dial's group carries one dimmed caption line — *"Individual projects can be excluded from the sidebar."*

---

## Complexity

**Medium.** The dial itself is Small — one `ComboRow`, one mock row, one GSettings key. The per-project exclusion adds a schema migration, a sidebar context-menu action, a row-suffix indicator, and one SQL clause: another Small. Both sit on top of the provider work from issue #189, which is where the actual Large lives (provider binary, its D-Bus service file, and a bounded per-keystroke query path) and which all three proposals share. **Smaller since 2026-08-02:** `DBusActivatable` and deep-link activation shipped with #197, and the Flatpak `--own-name` turned out not to be required at all.
