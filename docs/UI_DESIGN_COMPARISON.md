# UI Design Comparison: List vs Cards

## List View (`mockup-list-view.svg`)

### Pros

- ✅ **Information density** - See more sessions at once (5+ visible)
- ✅ **Scanning efficiency** - Eyes move vertically, easy to scan titles
- ✅ **Familiar pattern** - Matches GNOME Files, Mail, Calendar
- ✅ **Consistent with GNOME HIG** - Standard pattern for content browsers
- ✅ **Quick navigation** - Keyboard shortcuts work naturally (↑↓ arrows)
- ✅ **Better for search results** - When you have 50+ matches, list works better

### Cons

- ❌ Less visual distinction between sessions
- ❌ Metadata must be compact (single line)
- ❌ Harder to show rich previews

---

## Cards View (`mockup-cards-view.svg`)

### Pros

- ✅ **Visual appeal** - More modern, colorful
- ✅ **Room for metadata** - Can show more info per session (tags, previews)
- ✅ **Visual grouping** - Color-coded by AI tool
- ✅ **Touch-friendly** - Better for tablets (if that matters)

### Cons

- ❌ **Lower density** - Only 4-6 sessions visible at once
- ❌ **More scrolling** - When you have 100+ sessions
- ❌ **Less common in GNOME** - GNOME Software uses cards, but most browsers use lists
- ❌ **Keyboard nav** - Grid navigation (arrow keys) more complex

---

## Recommendation

**Start with List View** for v1:

1. More information on screen
2. Better for power users (keyboard navigation)
3. Scales better with large session counts
4. Matches GNOME HIG patterns (Files, Calendar, Logs)
5. Easier to implement in GTK4

**Future enhancement**: Add a view toggle button to switch between list/cards
- Similar to GNOME Files (list/grid toggle)
- User preference saved in settings

---

## Color Coding

Both views use accent colors for AI tools:
- **Claude Code**: Blue (#3584e4) - GNOME accent blue
- **OpenCode**: Green (#26a269) - GNOME success green
- **Codex**: Orange (#e66100) - Distinct from blue/green

These follow GNOME color palette for consistency.

---

**Status**: List view implemented in UI; cards view not implemented.
**Last Updated**: 2026-01-19
