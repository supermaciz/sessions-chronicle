# Multiple highlight_query Clones Issue

## Location
`src/ui/session_detail.rs:354-360`

## Issue Description
The search query string is cloned multiple times when loading messages:

```rust
let highlight = self.search_query.clone();  // Clone once
let mut guard = self.messages.guard();
for preview in previews {
    guard.push_back(MessageRowInit {
        preview,
        highlight_query: highlight.clone(),  // Clone for EACH preview
    });
}
```

## Impact
- Up to 200 clones per "Load More" operation (when page_size = 200)
- Search queries are typically short strings, so the memory impact is minimal
- The allocation overhead is acceptable for current usage patterns

## Potential Solutions

### Option 1: Use Arc<String>
```rust
use std::sync::Arc;

let highlight = Arc::new(self.search_query.clone());
let mut guard = self.messages.guard();
for preview in previews {
    guard.push_back(MessageRowInit {
        preview,
        highlight_query: highlight.clone(),  // Cheap Arc::clone
    });
}
```
**Pros:** Cheap cloning (pointer copy)
**Cons:** Requires changes to MessageRowInit type and lifetime management

### Option 2: Keep current implementation
The current approach is acceptable because:
- Query strings are small (typically 1-50 characters)
- 200 small string clones per page load is negligible
- Simpler code without Arc complexity

## Recommendation
Keep current implementation. Only consider optimization if profiling shows this as a performance bottleneck.

## References
- Identified in code review: https://github.com/supermaciz/sessions-chronicle/pull/...
- Design doc: docs/plans/2026-02-07-search-highlighting-design.md
