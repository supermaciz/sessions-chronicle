# Heatmap Axis Labels - Design

**Date:** 2026-03-08
**Status:** Design
**Parent:** `2026-03-07-basic-analytics-design.md`

## Problem

The activity heatmap renders only colored cells with no axis labels.
Users cannot tell which day of the week or month a cell represents without hovering for the tooltip.

## Solution

Add day-of-week labels on the left and month labels on top, matching GitHub's contribution heatmap style.

### Layout

```
         Jan        Feb        Mar  ...
Mon  [ ][ ][ ][ ][ ][ ][ ][ ][ ][ ]...
     [ ][ ][ ][ ][ ][ ][ ][ ][ ][ ]...
Wed  [ ][ ][ ][ ][ ][ ][ ][ ][ ][ ]...
     [ ][ ][ ][ ][ ][ ][ ][ ][ ][ ]...
Fri  [ ][ ][ ][ ][ ][ ][ ][ ][ ][ ]...
     [ ][ ][ ][ ][ ][ ][ ][ ][ ][ ]...
     [ ][ ][ ][ ][ ][ ][ ][ ][ ][ ]...
```

### Day-of-Week Labels (Left Axis)

- Show 3 labels: **Mon** (row 0), **Wed** (row 2), **Fri** (row 4)
- The grid already aligns to Monday as the first row, so row indices map directly
- Labels are vertically centered on their corresponding cell row
- Rendered using Pango layout via `widget.create_pango_layout()` + `snapshot.append_layout()`

### Month Labels (Top Axis)

- For each week column, check if a new month starts within that week's days
- If so, render the abbreviated month name (Jan, Feb, Mar...) above that column
- Month is derived by parsing the first day of each `HeatmapWeek` from `ActivityDay.day` (format `YYYY-MM-DD`)
- Skip rendering a month label if there is not enough horizontal space since the previous label (avoid overlap)

### Constants and Sizing

- `LABEL_LEFT_MARGIN`: ~30px, accommodates 3-character day abbreviations at small font size
- `LABEL_TOP_MARGIN`: ~16px, accommodates one line of month text
- Grid drawing offset: `(LABEL_LEFT_MARGIN, LABEL_TOP_MARGIN)` added to existing `PADDING`
- `measure()` updated to include both margins in width and height calculations

### Text Rendering

- Use `widget.create_pango_layout(Some("Mon"))` to create Pango layouts
- Use `snapshot.save()` / `snapshot.translate()` / `snapshot.append_layout()` / `snapshot.restore()` for positioning
- Font: use Pango font description at ~9px to match the 12px cell scale
- Color: use a dim foreground color (e.g. `RGBA(0.5, 0.5, 0.5, 0.8)`) for subtlety; refine after visual testing

### What Changes

- `src/ui/analytics_heatmap.rs`:
  - `draw_heatmap()`: offset grid by margins, add label rendering
  - `measure()`: add margins to size calculations
  - New helper: `draw_day_labels()` renders Mon/Wed/Fri
  - New helper: `draw_month_labels()` detects month boundaries and renders abbreviations
- No model, database, CSS, or analytics_view changes

### Testing

- Unit test: month boundary detection logic (given a list of weeks, which columns get a label)
- Unit test: day label positions (rows 0, 2, 4)
- Manual verification: visual check in light and dark themes, narrow and wide windows
