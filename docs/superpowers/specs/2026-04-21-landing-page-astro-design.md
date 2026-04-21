# Landing Page — Astro Implementation Design (Proposal F)

**Date:** 2026-04-21  
**Status:** Design, pending user review  
**Related:** [`docs/plans/2026-04-20-landing-page-exploration.md`](../../plans/2026-04-20-landing-page-exploration.md) (Proposal F — Product Atlas), [issue #124](https://github.com/supermaciz/sessions-chronicle/issues/124)

## Problem Statement

Sessions Chronicle needs a public landing page at `sessions-chronicle.maciz.dev`. The landing should communicate what the app does (indexes local AI assistant sessions), who it's for (GNOME users, developers), and how to install it. Proposal F — Product Atlas — was selected: a visual, screenshot-led page framed by Adwaita-like web components rather than impersonating a GTK window.

This document specifies the Astro 6.1.8 implementation of that design. It is the single source of truth after the brainstorming phase and precedes the task-by-task implementation plan.

## Scope

**In scope (v1):**
- One static page served at `https://sessions-chronicle.maciz.dev`.
- English copy only.
- Light theme only.
- No analytics, no cookies.
- GitHub Pages deployment via GitHub Actions.
- Custom domain via OVHcloud DNS.
- Screenshot strategy C+D: reuse the 3 existing screenshots plus composite mockups and component-rendered plates, capture 2–3 additional light-theme screenshots.

**Out of scope (v1, potential follow-ups):**
- Dark-theme variant + theme toggle.
- Additional pages (docs, blog, changelog).
- French localization.
- Visual regression testing infrastructure.
- Analytics integration (GoatCounter would be the first candidate).

## Decisions (captured during brainstorming)

| Decision | Choice | Rationale |
| --- | --- | --- |
| Astro version | 6.1.8 (latest stable at design time) | Node 22+, production-ready. |
| Screenshots | Strategy C+D: reuse 3 + composites + 2–3 new captures | Unblocks build now, avoids full re-capture campaign. |
| Language | English only | Aligns with repo, README, UI; zero translation debt. |
| Theming | Light only in v1, dark deferred | Matches screenshot availability; CSS stays theme-aware for later toggle. |
| CSS approach | Vanilla CSS + custom properties, no external framework | Faithful to libadwaita tokens; no Tailwind/utility mismatch; no ADWaveCSS dependency since the components F needs most (Banner, ActionRow, PreferencesGroup, Toast) are not provided. |
| Fonts | Adwaita Sans + JetBrains Mono, self-hosted WOFF2 | Identical typography to the app; SIL OFL license. |
| Deployment | GitHub Pages + GitHub Actions + custom domain | Free, zero server cost. |
| Domain | `sessions-chronicle.maciz.dev` via OVHcloud CNAME | User already owns `maciz.dev`. |
| Analytics | None | Aligned with "local-first, nothing leaves your machine" positioning. |
| Architecture | Two-layer composition: Adwaita primitives + page sections | The site literally is built from Adwaita primitives — the thesis reads in the file tree. |

## Project Structure

Root directory: `website/` (chosen to avoid clash with existing `docs/`).

```
website/
├── astro.config.mjs
├── package.json
├── package-lock.json
├── tsconfig.json
├── public/
│   ├── CNAME                          # sessions-chronicle.maciz.dev
│   ├── favicon.svg
│   ├── fonts/
│   │   ├── AdwaitaSans-Regular.woff2
│   │   ├── AdwaitaSans-Medium.woff2
│   │   ├── AdwaitaSans-Bold.woff2
│   │   └── JetBrainsMono-Regular.woff2
│   └── og-image.png                   # 1200×630 Open Graph card
├── src/
│   ├── assets/
│   │   └── screenshots/               # processed by Astro <Image>
│   ├── components/
│   │   ├── adwaita/
│   │   │   ├── Banner.astro
│   │   │   ├── ActionRow.astro
│   │   │   ├── PreferencesGroup.astro
│   │   │   ├── Toast.astro
│   │   │   ├── Card.astro
│   │   │   └── Button.astro
│   │   └── sections/
│   │       ├── Hero.astro
│   │       ├── AtlasGrid.astro
│   │       ├── SpecStrip.astro
│   │       └── Footer.astro
│   ├── layouts/
│   │   └── BaseLayout.astro           # <html>, <head>, meta, font preload
│   ├── pages/
│   │   └── index.astro
│   └── styles/
│       ├── tokens.css                 # Adwaita design tokens
│       ├── reset.css
│       └── global.css                 # @font-face, body defaults
└── README.md                          # build/deploy notes
```

**Rationale for `website/` vs separate repo:** keeps site versioned alongside code (a v1.0 release can be announced on the site in the same PR), shares `docs/screenshots/` without duplication, matches issue #124.

## Astro Config

`website/astro.config.mjs`:

```js
import { defineConfig } from 'astro/config';
import sitemap from '@astrojs/sitemap';

export default defineConfig({
  site: 'https://sessions-chronicle.maciz.dev',
  output: 'static',
  integrations: [sitemap()],
  image: {
    service: { entrypoint: 'astro/assets/services/sharp' },
  },
  build: {
    inlineStylesheets: 'auto',
  },
});
```

**Dependencies (`package.json`):**
- `astro` ^6.1.8
- `@astrojs/sitemap` (for `/sitemap-index.xml`)
- `sharp` (transitive via Astro image pipeline)

No UI framework (React/Vue/Svelte). No CSS framework. No JS libraries.

## Design Tokens (`src/styles/tokens.css`)

Values sourced from [libadwaita named colors](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/main/named-colors.html).

```css
:root {
  /* Surfaces */
  --window-bg-color: #fafafa;
  --window-fg-color: rgba(0, 0, 6, 0.8);
  --view-bg-color: #ffffff;
  --view-fg-color: rgba(0, 0, 6, 0.8);
  --headerbar-bg-color: #ebebeb;
  --card-bg-color: #ffffff;
  --card-shade-color: rgba(0, 0, 6, 0.07);

  /* Accent — GNOME blue */
  --accent-bg-color: #3584e4;
  --accent-fg-color: #ffffff;
  --accent-color: #1c71d8;        /* text on light bg */

  /* Semantic */
  --success-color: #26a269;
  --warning-color: #cd9309;
  --error-color: #c01c28;

  /* Borders */
  --border-color: rgba(0, 0, 6, 0.15);
  --separator-color: rgba(0, 0, 6, 0.07);

  /* Spacing (Adwaita scale) */
  --space-1: 4px;
  --space-2: 8px;
  --space-3: 12px;
  --space-4: 16px;
  --space-6: 24px;
  --space-8: 32px;
  --space-12: 48px;
  --space-16: 64px;

  /* Radius */
  --radius-sm: 6px;
  --radius-md: 9px;
  --radius-lg: 12px;
  --radius-xl: 16px;

  /* Shadows */
  --shadow-card: 0 1px 3px rgba(0, 0, 6, 0.05),
                 0 0 0 1px var(--border-color);
  --shadow-window: 0 8px 32px rgba(0, 0, 6, 0.12),
                   0 2px 8px rgba(0, 0, 6, 0.06);
  --shadow-toast: 0 6px 16px rgba(0, 0, 6, 0.18);

  /* Typography */
  --font-sans: "Adwaita Sans", system-ui, -apple-system, sans-serif;
  --font-mono: "JetBrains Mono", ui-monospace, monospace;

  --text-display: 56px;
  --text-h1: 32px;
  --text-h2: 22px;
  --text-body: 14px;
  --text-caption: 11px;
  --line-tight: 1.15;
  --line-body: 1.5;
}
```

**Dark mode placeholder** (commented, ready for v2):
```css
/* @media (prefers-color-scheme: dark) { :root { ... } } */
```

**Font loading (`global.css`):**

```css
@font-face {
  font-family: "Adwaita Sans";
  src: url("/fonts/AdwaitaSans-Regular.woff2") format("woff2");
  font-weight: 400;
  font-style: normal;
  font-display: swap;
}
/* + Medium (500), Bold (700) + JetBrains Mono Regular */
```

Source for Adwaita Sans: `https://gitlab.gnome.org/GNOME/adwaita-fonts` (exact URL for WOFF2 to verify at download time; if only TTF/OTF is published, convert with `woff2_compress`).

## Adwaita Primitives (`src/components/adwaita/`)

Each `.astro` file is ~30–60 lines (markup + scoped `<style>` + props). No JavaScript in v1.

### `Banner.astro`
Full-width informative strip with optional icon. Equivalent of `AdwBanner`.

- **Props:** `{ variant?: 'info' | 'accent', icon?: string }`
- **Slot:** message content
- **Visual:** background at 8% accent opacity, hairline border, `--radius-md`, padding `var(--space-3) var(--space-4)`

### `ActionRow.astro`
Row with title (and optional subtitle) on the left, suffix slot on the right. Workhorse of `PreferencesGroup`.

- **Props:** `{ title: string, subtitle?: string }`
- **Slots:** `prefix` (icon), `suffix` (action element), default (overrides title+subtitle if needed)
- **Visual:** min-height 56px, padding `var(--space-3) var(--space-4)`, bottom separator managed by parent group

### `PreferencesGroup.astro`
Container with optional title + description and a list of `ActionRow` children.

- **Props:** `{ title?: string, description?: string }`
- **Slot:** children (typically `ActionRow`s)
- **Visual:** `--card-bg-color` background, `--radius-lg`, `--shadow-card`, internal 1px hairline separators between rows

### `Toast.astro`
Small floating card, dark background, light text, optional action label.

- **Props:** `{ message: string, actionLabel?: string }`
- **Visual:** background `#2e2e2e` (libadwaita toast color), white fg, `--radius-md`, `--shadow-toast`, padding `var(--space-2) var(--space-3)`

### `Card.astro`
Generic plate with `--card-bg-color`, hairline border, `--shadow-card`. Used as plate container and as window frame around hero screenshots.

- **Props:** `{ padding?: 'none' | 'sm' | 'md' | 'lg', as?: 'div' | 'article' }`
- **Slot:** content

### `Button.astro`
Three variants: `suggested` (accent bg, white fg, `--radius-sm`), `flat` (transparent, 7% hover overlay), `pill` (suggested but full-height radius).

- **Props:** `{ variant: 'suggested' | 'flat' | 'pill', href?: string, type?: 'button' | 'submit' }`
- **Slot:** label

**Styling conventions:**
- All primitives consume `tokens.css`, no hardcoded values.
- Astro scoped `<style>` — internal class names like `.banner`, `.row`, `.group` are safe (no collision risk).
- Zero JavaScript.

## Page Sections (`src/components/sections/`)

### `Hero.astro`

**Layout:** CSS grid, 40/60 (left/right) on desktop, stacked on mobile.

**Left column (40%):**
- Eyebrow small caps: `OPEN SOURCE · LOCAL-FIRST · GTK 4`
- H1 display 56px: **"Every conversation you've had with a machine."**
- Subtitle 18px: "Sessions Chronicle indexes Claude Code, OpenCode, Codex, and Mistral Vibe transcripts on your disk into one searchable archive. Nothing leaves your machine."
- CTAs: `Button variant="pill"` "Get on Flathub" + `Button variant="flat"` "View on GitHub →"
- Meta line: `MIT · Flatpak · Linux`

**Right column (60%):**
Layered screenshot composition. No tilt/3D — simple XY offsets + shadows.

- Back layer: `session_list.png` (full window), `Card` wrapper with `--shadow-window`, offset bottom-left.
- Mid layer: `session_detail.png` (~70% crop), offset top-right, `--shadow-window`, hairline border.
- Front layer: `analytics_light.png` (~50% crop), bottom-right corner, smaller.
- **Adwaita overlays:** one `Banner` floating at the top ("New: Codex sessions now indexed.") and one `Toast` at the bottom ("Session resumed in terminal · Undo").

**LCP candidate:** the back layer image. Must use `<Image loading="eager" fetchpriority="high">`.

### `AtlasGrid.astro`

Section heading: "An atlas of what it does." + short subtitle.

CSS grid: 3 × 2 on desktop (≥1024px), 2 columns on tablet (768–1023px), 1 column on mobile (<768px). Each plate = `Card` with:
- Visual at the top (screenshot crop, composite, or Adwaita render)
- H3 title
- 1–2 sentences of body copy

**The six plates:**

| # | Title | Visual | Body |
| --- | --- | --- | --- |
| 1 | Search every transcript | Crop of `session_list_search_light.png` with search focused | "FTS5 over every prompt, response, and tool call. Filter by assistant, project, or date." |
| 2 | Inspect tool calls | Crop of `session_detail.png` on a tool burst | "Every Bash, Edit, and subagent call rendered inline with arguments and output." |
| 3 | Pin sessions worth keeping | Crop of `session_list.png` showing a pinned row (may need dedicated capture if no pinned row is visible) | "Star the breakthroughs. Skip the noise." |
| 4 | Resume in your terminal | Composite (in-app modal crop + stylized terminal rectangle, inline SVG or HTML) | "Open any session back in the assistant that wrote it. One click." |
| 5 | Track your habits | `analytics_light.png` | "See which assistant you reach for, which projects consume your time, and when you actually code." |
| 6 | Settings that respect Adwaita | Pure `PreferencesGroup` + 2–3 `ActionRow` render (no screenshot) | "Looks and feels like the rest of GNOME. Because it is." |

Plate 6 is the thesis made visible: the mini-library shows up as content, not chrome.

### `SpecStrip.astro`

Full-width band with `--card-shade-color` background, generous vertical padding.

Grid of 4 columns (desktop):
- **Stack:** Rust 2024 · GTK 4 · libadwaita · SQLite FTS5
- **Distribution:** Flatpak · Flathub-ready · MIT
- **Privacy:** 100% local · No network calls · No telemetry
- **Source:** github.com/supermaciz/sessions-chronicle

Values in JetBrains Mono, labels in Adwaita Sans. Small, dense, dev-tool feel.

### `Footer.astro`

Minimal.
- Left: "Sessions Chronicle · 2026 · MIT"
- Center: links (GitHub, Flathub, Issues, Releases)
- Right: "Made with libadwaita-on-the-web."

No social networks, no newsletter.

## Screenshot Inventory

### Existing (usable as-is)

| File | Theme | Uses |
| --- | --- | --- |
| `docs/screenshots/session_list.png` | light | Hero back layer; Plate 3 (Pin) if a pinned row is visible |
| `docs/screenshots/session_detail.png` | light | Hero mid layer; Plate 2 (Tool calls, recropped) |

### Existing (not usable in v1)

| File | Theme | Reason |
| --- | --- | --- |
| `docs/screenshots/analytics.png` | **dark** | v1 is light-only |

### To capture (2–3 new screenshots, light theme)

| File | Purpose |
| --- | --- |
| `analytics_light.png` | Hero front layer + Plate 5 |
| `session_list_search_light.png` | Plate 1 (search field focused, results filtered on a query like "refactor") |
| `session_list_pinned_light.png` (conditional) | Plate 3, only if the existing `session_list.png` has no visible pinned row |

### Zero-capture plates

- Plate 4 (Resume in terminal): composite assembled inline (small modal crop + stylized terminal rectangle).
- Plate 6 (Settings): rendered purely by the Adwaita primitives (`PreferencesGroup` + `ActionRow`).

### File naming convention (for new captures)

`{view}_{variant}_{theme}.png`, e.g. `session_list_search_light.png`. Existing files keep their current names.

## Image Pipeline

- Source PNGs placed in `website/src/assets/screenshots/` (imported from `docs/screenshots/` at build time — copy, not symlink, so Astro's import graph resolves).
- Use Astro's built-in `<Image>` component (backed by sharp in 6.x).
- Output formats: AVIF (preferred) + WebP (fallback) + PNG (final fallback) via `<picture>`.
- Responsive widths: `widths={[400, 800, 1200]}` with `sizes="(max-width: 768px) 100vw, 50vw"`.
- File hash in filename → infinite cache via GH Pages response headers.

**Per-image configuration:**

| Element | `loading` | `fetchpriority` | Source min width |
| --- | --- | --- | --- |
| Hero back layer | `eager` | `high` | ≥ 1280px |
| Hero mid/front | `eager` | (default) | ≥ 960px / 720px |
| Atlas plates | `lazy` | (default) | ≥ 720px |

**Layout stability:** every `<Image>` has explicit `width`/`height` → no CLS.

**Placeholders:** Astro's built-in blurhash placeholder generation; no extra config needed.

**OG image:** `public/og-image.png` (1200×630), statically authored, not processed through the image pipeline.

## Deployment

### DNS configuration (OVHcloud)

Domain: `maciz.dev`, DNS Zone →

| Type | Sub-domain | Target | TTL |
| --- | --- | --- | --- |
| CNAME | `sessions-chronicle` | `supermaciz.github.io.` (trailing dot required) | 3600 |

### CNAME file

`website/public/CNAME` — single line, no protocol, no slash:
```
sessions-chronicle.maciz.dev
```

### GitHub Pages settings

- **Source:** GitHub Actions (not "Deploy from a branch")
- **Custom domain:** auto-populated from the deployed `CNAME`
- **Enforce HTTPS:** enable after Let's Encrypt cert is issued (~10 min after DNS resolves)

### Workflow: `.github/workflows/deploy-website.yml`

```yaml
name: Deploy website

on:
  push:
    branches: [main]
    paths:
      - 'website/**'
      - '.github/workflows/deploy-website.yml'
  workflow_dispatch:

permissions:
  contents: read
  pages: write
  id-token: write

concurrency:
  group: pages
  cancel-in-progress: false

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: '22'
          cache: 'npm'
          cache-dependency-path: website/package-lock.json
      - name: Install
        run: npm ci
        working-directory: website
      - name: Build
        run: npm run build
        working-directory: website
      - uses: actions/upload-pages-artifact@v3
        with:
          path: website/dist

  deploy:
    needs: build
    runs-on: ubuntu-latest
    environment:
      name: github-pages
      url: ${{ steps.deployment.outputs.page_url }}
    steps:
      - id: deployment
        uses: actions/deploy-pages@v4
```

### Bootstrap order

1. Scaffold `website/` with `CNAME` in place; first commit.
2. Push. The build step passes; the deploy step fails (Pages not yet enabled) — expected.
3. GitHub → Settings → Pages → Source = "GitHub Actions".
4. Re-run the workflow via `workflow_dispatch`.
5. Add the OVHcloud CNAME record.
6. Wait for DNS propagation (5–60 min) and Let's Encrypt cert issuance.
7. Tick "Enforce HTTPS".
8. Verify `https://sessions-chronicle.maciz.dev` loads.

### Rollback

- Revert the offending commit on `main` → automatic redeploy.
- Or re-run a previous successful workflow run from the Actions tab → redeploys the prior artifact.

## Verification Plan

### Local build

```
cd website
npm run build
npm run preview
```

`npm run build` must complete with zero warnings. Any Astro warning (missing asset, broken import) blocks PR.

### Lighthouse budget

Targets (non-negotiable for v1):
- Performance ≥ 95
- Accessibility = 100
- Best Practices ≥ 95
- SEO = 100

Manual run via Chrome DevTools (mobile + desktop) before merge. Lighthouse CI as a GitHub Action is a follow-up if deploy cadence warrants it.

Focus areas:
- **LCP** < 2.5s on throttled 4G (hero back layer).
- **CLS** ≈ 0 (explicit image dimensions, `font-display: swap`, preload critical WOFF2).
- **TBT** ≈ 0 (no JS).

### Accessibility checklist (manual)

- Keyboard: `Tab` walks all interactive elements in visual order; focus ring visible (2px offset, `--accent-bg-color`).
- Contrast: all body text ≥ 4.5:1 on its background (WCAG AA). Tokens already satisfy this; spot-check `.banner` accent and `.toast` dark.
- Screen reader (VoiceOver or Orca): hero reading order is logical; images have descriptive `alt` (e.g. "Sessions Chronicle main window listing recent conversations"), decorative `Banner`/`Toast` marked `aria-hidden="true"`.
- `prefers-reduced-motion: reduce` respected (v1 has no motion, but any future transition must honor it).

### HTML validation

```
npx html-validate website/dist/**/*.html
```
Or W3C validator. Zero errors expected.

### Link check

```
npx linkinator https://sessions-chronicle.maciz.dev --recurse --silent
```

Run post-deploy when external links change.

### Responsive breakpoints (manual)

- 360px (mobile): single column, hero layers stack, Atlas 1 col.
- 768px (tablet): Atlas 2 col, hero still stacked.
- 1024px (desktop): hero 40/60, Atlas 3 col.
- 1440px+: container max-width ~1280px, centered.

Browser targets: Firefox (GNOME default), Chromium. Safari optional.

### Font loading

DevTools → Network → "Slow 3G" throttle → reload. Expect a flash of system font swapping to Adwaita Sans; never invisible text.

### Definition of Done (before PR)

- [ ] `npm run build` passes with zero warnings
- [ ] `npm run preview` loads without console errors
- [ ] Lighthouse (mobile): Perf ≥ 95, A11y = 100, BP ≥ 95, SEO = 100
- [ ] Tab navigation reaches every link/button; focus ring visible
- [ ] 360 / 768 / 1024 viewports render without overflow
- [ ] Alt text present on all non-decorative images
- [ ] HTML validator: 0 errors
- [ ] Screenshots of changes attached to the PR

## Open Questions / Assumptions to Confirm

- **Pinned row visibility** in existing `session_list.png`: if absent, add `session_list_pinned_light.png` to the capture list.
- **Adwaita Sans distribution format**: confirm WOFF2 is published at the GNOME fonts repo. If only TTF/OTF, convert using `woff2_compress` at build time and commit the output.
- **Flathub listing URL**: the "Get on Flathub" CTA needs a target. If the app is not yet on Flathub at implementation time, the button should link to the GitHub Releases page with label "Download (Flatpak)" instead.

## References

- Proposal F mockup: [`docs/mockups/landing-page/06-product-atlas.svg`](../../mockups/landing-page/06-product-atlas.svg)
- Exploration doc: [`docs/plans/2026-04-20-landing-page-exploration.md`](../../plans/2026-04-20-landing-page-exploration.md)
- libadwaita named colors: https://gnome.pages.gitlab.gnome.org/libadwaita/doc/main/named-colors.html
- Astro docs: https://docs.astro.build
- GitHub Pages custom domains: https://docs.github.com/en/pages/configuring-a-custom-domain-for-your-github-pages-site
