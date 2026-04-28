# Remove Flathub References from the Website

**Date:** 2026-04-23
**Scope:** `website/` only

## Problem

The Sessions Chronicle landing site currently advertises a Flathub listing
(`https://flathub.org/apps/io.github.supermaciz.sessionschronicle`) that does
not exist. The project is unlikely to be accepted on Flathub. A self-hosted
Flatpak repository is planned for the near future but is not yet available.

Until the self-hosted repository ships, the site must not claim or link to a
Flathub distribution channel.

## Goal

Remove every Flathub link, URL constant, and label from the site. Re-balance
the remaining CTAs so the GitHub repository becomes the single primary action.
No placeholder or "coming soon" copy is introduced.

## Non-goals

- Wiring a new "Install" CTA to the future self-hosted repository — handled in a
  separate future task once the repo exists.
- Changes to the README, CI, GSettings schema, or Flatpak manifests.
- Any visual redesign beyond promoting the existing GitHub button.

## Changes by file

### `website/src/components/sections/Hero.astro`

- Remove the `flathubUrl` constant (lines 7–8).
- Remove `<Button variant="pill" href={flathubUrl}>Get on Flathub</Button>`
  (line 23).
- Change the remaining GitHub button from `variant="flat"` to `variant="pill"`
  so the hero keeps a single strong primary CTA.
- Keep the `hero__meta` line `"MIT · Flatpak · Linux"` — still accurate, as
  Flatpak remains the packaging format even without Flathub.

### `website/src/components/sections/TopBar.astro`

- Remove the `flathubUrl` constant (line 4).
- Remove `<Button variant="suggested" href={flathubUrl}>Install</Button>`
  (line 24).
- Remove the now-unused `import Button from '../adwaita/Button.astro';`.
- Final nav contains two text links only: `Features`, `GitHub`.

### `website/src/components/sections/SpecStrip.astro`

- Remove `<div class="spec__value">Flathub-ready</div>` (line 15).
- The "Distribution" column shortens to two values: `Flatpak`, `MIT`. No
  replacement entry is added.

### `website/src/components/sections/Footer.astro`

- Remove `<a href="https://flathub.org/apps/...">Flathub</a>` (line 9).
- Remaining footer links: `GitHub`, `Issues`, `Releases`.

## Verification

- `cd website && npm run build` completes without errors (catches orphaned
  imports or unused constants).
- Local visual pass via `npm run dev`: Hero has one centered pill CTA, TopBar
  nav has no buttons, SpecStrip "Distribution" column renders with two values,
  Footer shows three link entries.

## Out of scope / follow-up

When the self-hosted Flatpak repository is live, a follow-up task will
reintroduce an "Install" CTA in the Hero and/or TopBar pointing at the new
URL, and may restore a "Distribution" entry in the SpecStrip naming the repo.
