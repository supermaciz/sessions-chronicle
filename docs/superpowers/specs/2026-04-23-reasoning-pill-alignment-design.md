# Reasoning Pill Alignment Design

## Problem

Reasoning-related pills in the transcript UI do not follow a single placement
rule. `Thinking`, `Thinking (encrypted)`, and burst-level encrypted badges can
appear on different sides of their headers depending on which transcript row
builder produced them.

That inconsistency reads as layout drift rather than meaningful visual
distinction.

## Goal

Use one placement rule for all reasoning pills: keep them left-aligned inside
the normal header flow.

This applies to:

- clickable `Thinking` pills for visible reasoning
- non-interactive `Thinking (encrypted)` pills for encrypted-only reasoning
- burst summary pills such as `3 encrypted`

## Non-Goals

- Do not rename the pills.
- Do not change pill colors or typography.
- Do not change when a pill is shown or whether it is clickable.
- Do not redesign transcript row spacing beyond what is needed for consistent
  placement.

## Current Behavior

`src/ui/transcript_row.rs` builds reasoning pills in multiple code paths:

- message rows
- tool call rows
- tool burst headers

The message and tool-call variants append pills directly into left-flowing
header boxes. The burst header uses a separate `gtk::FlowBox` layout path. The
result is that reasoning pills do not present as one stable UI pattern across
the transcript.

## Decision

Standardize all reasoning pill placement so the pill is read immediately after
the left-side metadata of the row or group header.

### Placement rule

- Message rows: keep reasoning pills in the existing left-side header flow.
- Tool call rows: keep reasoning pills in the existing left-side header flow.
- Tool burst headers: place reasoning pills in the same left-flowing sequence as
  the other header metadata, without introducing a right-anchored special case.

The product rule is simple: reasoning pills describe the current row or burst,
so they should stay visually attached to that descriptive metadata instead of
floating to the opposite edge.

## Implementation Notes

The fix should stay structural and minimal.

- Prefer unifying header composition in `src/ui/transcript_row.rs`.
- Avoid CSS-only tweaks if the actual inconsistency comes from container
  structure or expansion behavior.
- Preserve existing wrapping behavior for dense headers, especially tool-burst
  headers that already use wrapping to remain narrow-window friendly.

## Files Expected To Change

- `src/ui/transcript_row.rs`

CSS changes in `data/resources/style.css` are not expected unless a small follow
up is required after verifying the structural fix.

## Verification

During implementation, verify:

- `Thinking` pills stay left-aligned in message rows
- `Thinking (encrypted)` pills stay left-aligned in message and tool-call rows
- burst reasoning pills such as `3 encrypted` stay left-aligned within the burst
  header flow
- long headers still wrap cleanly instead of overflowing or collapsing awkwardly

Repo-level verification before completion should still include:

- `cargo fmt --all -- --check`
- `cargo clippy --all -- -D warnings`
- `cargo test --all --no-fail-fast`

## Risks

- Tool-burst headers use a different container from normal rows, so a minimal
  alignment fix must not regress the wrapping behavior added for narrow layouts.
- Small layout shifts may affect tests or screenshots if any assertions depend on
  header structure.
