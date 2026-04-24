# Website Hero Centered Commands Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the website hero shell commands below the full hero grid and center the command block.

**Architecture:** Keep this as a surgical Astro component change. `Hero.astro` already owns the hero markup and scoped styles, so no new component or shared CSS is needed.

**Tech Stack:** Astro 6.x, scoped `.astro` CSS, website verification with `npm run build`.

---

## File Structure

- Modify: `website/src/components/sections/Hero.astro`  
  Owns the hero content, screenshot composition, command markup, and scoped styles. The command block should move from the text column to a full-width row after `.hero__grid`.

No new files are required.

## Task 1: Move And Center Hero Commands

**Files:**
- Modify: `website/src/components/sections/Hero.astro`

- [ ] **Step 1: Inspect current hero command placement**

Read `website/src/components/sections/Hero.astro` and confirm these current facts:

```astro
<div class="hero__text">
  ...
  <div class="hero__ctas">
    <Button variant="pill" href={githubUrl}>View on GitHub →</Button>
  </div>
  <div class="hero__commands" aria-label="Flatpak install commands">
    <code>{flatpakInstallCommand}</code>
    <code>{flatpakRunCommand}</code>
  </div>
  <p class="hero__meta">MIT · Self-hosted Flatpak · Linux</p>
</div>
```

Expected: `.hero__commands` is inside `.hero__text`, between `.hero__ctas` and `.hero__meta`.

- [ ] **Step 2: Move command markup below the grid**

In `website/src/components/sections/Hero.astro`, remove the command block from `.hero__text` and add it after the closing `</div>` for `.hero__grid`.

The top-level section markup should become:

```astro
<section class="hero">
  <div class="container">
    <div class="hero__grid">
      <div class="hero__text">
        <p class="hero__eyebrow">OPEN SOURCE · LOCAL-FIRST · GTK 4</p>
        <h1 class="hero__title">Every conversation you've had with a machine.</h1>
        <p class="hero__sub">
          Sessions Chronicle indexes your coding agents transcripts on your disk into one searchable archive. Nothing
          leaves your machine.
        </p>
        <ul class="hero__assistants" aria-label="Supported AI assistants">
          {supportedAssistants.map((assistant) => (
            <li class="hero__assistant-pill">
              <span class="hero__assistant-icon" style={`--assistant-icon: url('${assistant.icon}')`} aria-hidden="true"></span>
              <span>{assistant.name}</span>
            </li>
          ))}
        </ul>
        <div class="hero__ctas">
          <Button variant="pill" href={githubUrl}>View on GitHub →</Button>
        </div>
        <p class="hero__meta">MIT · Self-hosted Flatpak · Linux</p>
      </div>

      <div class="hero__visual">
        <div class="hero__slot hero__slot--main">
          <Picture
            src={sessionDetail}
            formats={["avif", "webp"]}
            fallbackFormat="png"
            widths={[400, 640, 960, 1280]}
            sizes="(max-width: 640px) 88vw, (max-width: 1023px) 70vw, 620px"
            loading="eager"
            fetchpriority="high"
            alt="A single Claude Code session showing tool calls inline"
          />
        </div>

        <div class="hero__slot hero__slot--overlay">
          <Picture
            src={sessionList}
            formats={["avif", "webp"]}
            fallbackFormat="png"
            widths={[240, 360, 480, 720]}
            sizes="(max-width: 640px) 42vw, (max-width: 1023px) 28vw, 260px"
            loading="eager"
            alt="Sessions Chronicle main window listing recent AI assistant conversations"
          />
        </div>
      </div>
    </div>

    <div class="hero__commands" aria-label="Flatpak install commands">
      <code>{flatpakInstallCommand}</code>
      <code>{flatpakRunCommand}</code>
    </div>
  </div>
</section>
```

- [ ] **Step 3: Update command block centering styles**

In the existing `.hero__commands` CSS rule, add centering and a top margin for separation from the hero grid.

Replace the current rule:

```css
.hero__commands {
  display: grid;
  gap: var(--space-2);
  margin-bottom: var(--space-4);
  max-width: min(100%, 42rem);
}
```

With:

```css
.hero__commands {
  display: grid;
  gap: var(--space-2);
  width: min(100%, 42rem);
  margin: var(--space-10) auto 0;
}
```

Expected: the command block is centered as a block, keeps its 42rem max width, and no longer reserves bottom margin before `.hero__meta` because metadata is no longer below it.

- [ ] **Step 4: Verify formatting**

Run:

```bash
npm run build
```

From: `website/`

Expected: Astro build completes successfully and reports generated pages without errors.

- [ ] **Step 5: Manual visual verification**

Run:

```bash
npm run preview -- --host 127.0.0.1 --port 4321
```

From: `website/`

Expected visual result at `http://127.0.0.1:4321/`:

- The command block appears below both hero columns on desktop.
- The command block is horizontally centered in the page container.
- The command text remains left-aligned inside each dark code block.
- On a narrow viewport, the command block remains below the hero content and long command text can scroll horizontally.

- [ ] **Step 6: Commit if requested**

Only commit if the user explicitly asks for a commit.

If requested, run:

```bash
git add website/src/components/sections/Hero.astro docs/superpowers/specs/2026-04-24-website-hero-centered-commands-design.md docs/superpowers/plans/2026-04-24-website-hero-centered-commands.md
git commit -m "fix: center hero install commands"
```

Expected: commit succeeds with the repository's normal hooks.

## Self-Review

Spec coverage: Task 1 moves `.hero__commands` below `.hero__grid`, centers it with `margin: ... auto`, preserves bounded width, left-aligned code text, `white-space: pre`, and `overflow-x: auto`.

Placeholder scan: No TBD/TODO/implement-later placeholders remain.

Type consistency: CSS selectors and Astro variable names match the existing `Hero.astro` component.
