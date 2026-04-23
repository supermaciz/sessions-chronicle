# Website Hero Supported AI Assistants

**Date:** 2026-04-23
**Scope:** `website/` only

## Problem

The landing page hero currently jumps from the product subtitle directly to the
single `View on GitHub` CTA. The page copy already claims support for multiple
AI assistants, but that support is not surfaced as a clear visual element in
the hero.

The site needs a compact compatibility treatment that makes the supported AI
assistants visible without weakening the existing CTA hierarchy.

## Goal

Add a non-interactive row of supported AI assistant pills in the hero, placed
between the subtitle and the `View on GitHub` button. Each pill should show the
assistant name and its symbolic logo.

The pills must feel secondary to the CTA, fit the existing light Adwaita-like
visual language, and wrap cleanly on smaller viewports.

## Non-goals

- Changing hero copy beyond inserting the supported-assistant pills.
- Adding new outbound links from the hero pills.
- Introducing assistant-specific brand colors.
- Redesigning the hero layout, screenshot stack, or CTA treatment.

## Decision

Use a single inline row of neutral outlined pills, with no additional label such
as `Supported AI assistants`.

This is preferred over accent-tinted pills because the current hero already has
a single strong blue CTA, and the compatibility row should remain informative
rather than promotional. It is also preferred over a plain text list because
the pill shape better communicates a supported-platform style inventory.

## Changes by file

### `website/src/components/sections/Hero.astro`

- Add a local data array describing the four supported AI assistants:
  - Claude Code
  - OpenCode
  - Codex
  - Mistral Vibe
- Render a new `hero__assistants` block between `hero__sub` and `hero__ctas`.
- Render the pills from the local array instead of hardcoding four separate
  blocks.
- Each pill contains:
  - a symbolic logo marked `aria-hidden="true"`
  - the visible assistant name as text
- Pills are informational only and are not links or buttons.
- Keep the display order aligned with the hero copy: Claude Code, OpenCode,
  Codex, Mistral Vibe.

### Website asset handling

- Reuse the existing symbolic icons already present in `data/icons/`:
  - `claude-code-symbolic.svg`
  - `opencode-symbolic.svg`
  - `codex-symbolic.svg`
  - `mistral-vibe-symbolic.svg`
- Expose those icons to the website as web assets instead of recreating them in
  CSS or inlined SVG markup.
- Keep the icons monochrome to match the symbolic asset style.

## Visual behavior

### Layout

- The supported-assistant row sits directly below the hero subtitle and directly
  above the GitHub CTA.
- The pill container uses flex layout with wrapping enabled.
- Desktop and tablet layouts may use one or two lines depending on available
  width.
- Mobile keeps the pills in normal flow, wrapping naturally above the CTA.
- No carousel, slider, marquee, or separate support section is introduced.

### Pill styling

- Neutral visual treatment:
  - white or near-white background
  - subtle border using a low-opacity neutral color
  - fully rounded pill radius
  - compact internal padding
- Logo and label are horizontally aligned with a small gap.
- Text uses the existing sans family with medium-to-semibold emphasis so the row
  reads as metadata rather than body copy.
- Logo color should stay close to the main foreground color, but slightly softer
  than the primary heading to avoid visual heaviness.

### Hierarchy

- The `View on GitHub` button must remain the strongest visual action in the
  hero.
- Pills must read as compatibility information, not as competing calls to
  action.
- No hover treatment should imply clickability beyond a subtle static polish if
  needed for consistency.

## Accessibility

- Each assistant name remains visible text in the DOM.
- Symbolic logos are decorative because the label already names the assistant;
  therefore the icon element should be `aria-hidden="true"`.
- Because the pills are not interactive, they should not receive focus and must
  not be announced as controls.

## Verification

- `cd website && npm run build` completes successfully.
- Visual review on desktop verifies:
  - the assistant pills appear between subtitle and CTA
  - the CTA remains more visually prominent than the pills
  - the row spacing matches the hero rhythm
- Visual review on mobile verifies:
  - pills wrap cleanly
  - no overlap with the CTA or screenshot area occurs
  - hero height remains balanced

## Out of scope / follow-up

- Adding support badges elsewhere on the website.
- Converting the pills into filter links, documentation links, or install links.
- Extending the row to future assistants until parser support actually exists.
