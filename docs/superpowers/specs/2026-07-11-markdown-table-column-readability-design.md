# Markdown Table Column Readability Design

**Date:** 2026-07-11  
**Status:** Approved, pending implementation plan

## Goal

Make fixed-width markdown table columns readable in production by increasing
their width from 120 px to 160 px and removing the one-character label width
constraints that can make ordinary text wrap character by character.

## Context

Manual verification of the `MarkdownTable` production wiring showed that the
custom widget scrolls horizontally, does not leave blank space below large
tables, and works with the outer transcript scroll. It also exposed unreadable
cells in a nine-column table: values such as `Projet Alpha` could render as one
character per line.

`create_table_label` currently combines wrapping with `width_chars = 1` and
`max_width_chars = 1`. Those properties constrain the label's requested width
to one character and are not needed because `MarkdownTable` measures and
allocates every cell at the fixed column width itself.

The current 120 px column rule also leaves too little text space after cell
padding. Manual comparison favored 160 px fixed columns over content-adaptive
columns. Fixed sizing preserves the custom widget's stable height-for-width
boundary and avoids reintroducing content-dependent column measurement.

## Scope

- Change `COLUMN_MIN_WIDTH` from `120` to `160`.
- Remove `set_width_chars(1)` and `set_max_width_chars(1)` from wrapping table
  labels.
- Keep wrapping enabled with `gtk::pango::WrapMode::WordChar` so genuinely long
  unbroken text can still wrap.
- Keep equal fixed widths for every column.
- Let existing width, row-height, scrollbar, clipping, and allocation logic use
  the revised constant without adding a second sizing path.
- Add regression coverage proving ordinary multi-word text is not constrained
  to a one-character requested width and remains readable at the fixed column
  allocation.
- Repeat focused automated tests and fixture-driven visual verification.

## Non-goals

- Do not add content-adaptive or per-column widths.
- Do not add minimum/maximum width heuristics based on header or cell content.
- Do not change markdown parsing semantics.
- Do not add a vertical table scroller.
- Do not add `Shift`+mouse-wheel handling; track that as a separate issue.
- Do not change the existing horizontal scrollbar interaction.

## Architecture

`MarkdownTable` continues to use one fixed width as the single source of truth:

```text
COLUMN_MIN_WIDTH = 160
  -> total_table_width(column_count)
  -> horizontal minimum/natural measurement
  -> per-cell vertical measurement
  -> per-cell size allocation
  -> horizontal adjustment upper bound
```

No content measurement influences column width. This keeps row height derived
from wrapping at a known width and keeps the table's custom `WidgetImpl`
responsible for the height-for-width boundary.

`create_table_label(..., wraps = true)` configures labels with wrapping and
`WordChar`, but leaves `width_chars` and `max_width_chars` at their GTK defaults.
The custom widget's explicit 160 px measurement and allocation provide the
actual wrapping width.

## Behavior To Preserve

- All columns have equal fixed width.
- The widget remains shrinkable to one column width.
- Natural width remains the full fixed-width table plus column spacing.
- Tables wider than their viewport expose the internal horizontal scrollbar.
- Cells and headers keep their existing CSS classes and Pango highlighting.
- Header separator measurement and pinned allocation remain unchanged.
- Large wrapped tables report their honest content height without blank space.
- Search match counts remain unchanged.

## Testing

Update focused tests in `src/ui/markdown_table.rs`:

- `total_table_width_uses_fixed_column_width_and_spacing` continues to derive
  expectations from `COLUMN_MIN_WIDTH` and therefore verifies the 160 px source
  of truth.
- Wrapping-label coverage asserts `width_chars() == -1` and
  `max_width_chars() == -1`, the GTK defaults, instead of allowing the
  one-character request to return.
- Add a regression test using `Projet Alpha` in a body cell. Allocate the table
  at its full width and assert the cell receives `COLUMN_MIN_WIDTH` bounds and
  its Pango layout does not split the value into one-character lines.
- Keep stable-height, scrollbar visibility, clipping, separator, and horizontal
  adjustment tests unchanged except for expectations derived from the revised
  constant.

Run:

```sh
cargo test markdown_table::tests -- --nocapture
cargo fmt --all -- --check
cargo clippy --all -- -D warnings
cargo test --all --no-fail-fast
```

## Manual Verification

Open the previously reported session and inspect both the short ten-column
table and the prose-heavy nine-column table:

- ordinary values such as `Projet Alpha`, dates, statuses, and tags do not wrap
  character by character;
- prose cells wrap at a visibly more readable width;
- the horizontal scrollbar appears and remains functional for wide tables;
- the pinned separator, absence of blank trailing space, and outer transcript
  scrolling remain correct.

Capture an updated screenshot of the nine-column table for the follow-up issue
or pull request.

## Decision

Use 160 px equal fixed-width columns and remove the one-character GTK label
width hints. This is the smallest change that fixes the observed readability
problem while preserving the stable custom layout architecture. Adaptive
column sizing and `Shift`+mouse-wheel support remain separate follow-ups.
