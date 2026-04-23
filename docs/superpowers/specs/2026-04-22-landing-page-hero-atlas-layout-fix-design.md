# Landing Page Hero/Atlas Layout Fix Design

## Problem

The current landing page rendering breaks down with the updated screenshots:

- The `Hero` composition uses three absolutely-positioned screenshots sized mostly by width, with no stable display frame ratio.
- `analytics_light.png` is much taller than the other images, which stretches the visual stack and makes the hero feel vertically collapsed rather than layered.
- Atlas screenshot cards use a fixed outer frame, but the inner image treatment is inconsistent, so screenshots are not visually normalized across the grid.

The result is a hero that feels like a vertical stack of distorted screenshots instead of a deliberate product composition.

## Goals

- Make the `Hero` read as one dominant product shot with one supporting overlay.
- Keep screenshot presentation visually consistent regardless of source image ratio.
- Preserve the existing copy, section order, and overall Astro architecture.
- Keep the change minimal and local to the website components.

## Non-Goals

- No copy rewrite.
- No redesign of the atlas information architecture.
- No new sections, primitives, or JavaScript.
- No broad CSS refactor outside the affected components.

## Chosen Direction

Use a two-image hero composition:

- One primary screenshot as the main product frame.
- One secondary screenshot as a smaller overlay anchored to the lower-right area.
- Remove `analytics_light.png` from the hero.
- Keep `analytics_light.png` in the atlas only.

For atlas screenshots:

- Use a single stable landscape frame treatment for every screenshot card.
- Normalize image rendering with `object-fit: cover`.
- Allow per-image `object-position` tweaks where the useful content is not centered.

## Component Changes

### `website/src/components/sections/Hero.astro`

- Remove the third screenshot layer from the hero.
- Keep `session_list.png` as the dominant screenshot.
- Keep `session_detail.png` as the secondary overlay screenshot.
- Replace the current three-layer absolute layout with a two-frame composition:
  - a main framed window
  - a smaller overlay frame
- Give the two frames explicit landscape ratios instead of letting source image dimensions drive the height.
- Apply stable image rendering inside those frames with `object-fit: cover`.
- Keep the existing copy, CTA block, banner, and toast unless layout pressure requires hiding/de-emphasizing them at narrower sizes.

### `website/src/components/sections/AtlasGrid.astro`

- Keep the current atlas structure and cards.
- Update screenshot rendering so every screenshot card uses the same frame behavior.
- Replace the current `max-width` / `max-height` style treatment with true fill behavior inside the visual frame.
- Use `object-fit: cover` for screenshot images inside `.plate__visual`.
- Add targeted `object-position` rules where needed:
  - analytics aligned closer to top-center
  - other product screenshots centered by default unless testing shows a better focal point

## Layout Behavior

### Hero

Desktop:

- Two-column layout remains.
- Left column keeps the copy and CTAs.
- Right column becomes a controlled composition area with a bounded height.
- Main screenshot carries most of the visual weight.
- Secondary screenshot overlays the main frame instead of forming a third stacked layer.

Tablet and mobile:

- Preserve hierarchy, not exact geometry.
- If the overlay becomes too aggressive, reduce its size and/or move it lower in the composition.
- The hero should never become a tall screenshot stack.

### Atlas

- Every screenshot card keeps the same outer frame ratio.
- Images fill the frame consistently.
- Vertical screenshots are intentionally cropped rather than letterboxed.
- The grid should feel uniform even when source screenshots are heterogeneous.

## CSS Strategy

- Keep the change local to `Hero.astro` and `AtlasGrid.astro`.
- Prefer container-controlled ratios over image-controlled dimensions.
- Use `object-fit: cover` for screenshot consistency.
- Use small, explicit selector-based overrides for focal-point adjustments instead of introducing new abstraction layers.

## Verification Plan

- Run `cd website && npm run build`.
- Run `cd website && npx --yes html-validate "dist/**/*.html"`.
- Run `cd website && npm run preview -- --host 127.0.0.1 --port 4321` and visually verify:
  - the hero has one dominant screenshot and one secondary overlay
  - the hero no longer reads as a vertical screenshot stack
  - screenshots are not visually stretched
  - atlas cards feel visually uniform despite differing source image ratios
  - no obvious clipping of important screenshot content

## Scope

This design is intentionally narrow and can be implemented in a single follow-up change set touching only the landing page website files.
