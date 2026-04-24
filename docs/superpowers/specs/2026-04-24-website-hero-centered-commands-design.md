# Website Hero Centered Commands

**Date:** 2026-04-24  
**Scope:** `website/` only

## Problem

The landing page hero currently renders the Flatpak install and run commands inside the left text column. They sit below the GitHub CTA and align with the column, which makes them feel attached to the text block instead of presented as a centered install step beneath the hero.

## Decision

Move the command block below the full hero grid and center it across the hero container. This matches the requested layout: the shell commands should be below the hero content and centered.

## Design

Update `website/src/components/sections/Hero.astro` only.

The hero structure should become:

1. `.hero__grid` containing the existing text column and visual column.
2. `.hero__commands` rendered after `.hero__grid`, still inside `.hero > .container`.

The command block should:

- Use `margin-inline: auto` so the block is centered within the page container.
- Keep a bounded `max-width` so commands remain readable on desktop.
- Keep each `<code>` line text left-aligned for terminal readability.
- Preserve `white-space: pre` and `overflow-x: auto` so the long install command remains usable on small screens.

The existing CTA, assistant pills, screenshots, and metadata should not change behavior.

## Testing

Run `npm run build` from `website/`.

Manual visual check:

- On desktop, the commands appear below both the text and screenshot columns and are horizontally centered.
- On mobile, the commands remain below the hero content, fit the viewport, and scroll horizontally if needed.
