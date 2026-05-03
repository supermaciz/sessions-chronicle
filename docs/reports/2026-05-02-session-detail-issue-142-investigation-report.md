# Session Detail Issue 142 Investigation Report

## Scope

- Date: 2026-05-02
- Target session: `019dc51a-f0cd-79c1-ba79-45fedac889c2`
- AI assistant: Codex
- Measurement type: full-app local development build
- Source spec: `docs/superpowers/specs/2026-05-02-session-detail-issue-142-investigation-design.md`

## Protocol

Build command:

```bash
meson install -C builddir
```

Run command pattern used for each variant:

```bash
RUST_LOG=info,sessions_chronicle=debug ~/.local/bin/sessions-chronicle > /tmp/sc-issue142-baseline-1.log 2>&1
```

Scenario:

1. Cold launch the application.
2. Wait for `Background indexing complete`.
3. Click session `019dc51a-f0cd-79c1-ba79-45fedac889c2` exactly once.
4. Do not perform search before the click.

## Results

| Variant | Run | Max schedule gap | First-page load to factory push | Max post-drop residual |
| --- | ---: | ---: | ---: | ---: |
| baseline | 1 | 1335 ms | 7855 ms | 1334 ms |
| baseline | 2 | 1346 ms | 7652 ms | 1346 ms |
| baseline | 3 | 1377 ms | 7885 ms | 1376 ms |
| baseline | median | 1346 ms | 7855 ms | 1346 ms |
| a1-nav-animation | 1 | 1345 ms | 7595 ms | 1344 ms |
| a1-nav-animation | 2 | 1348 ms | 7513 ms | 1347 ms |
| a1-nav-animation | 3 | 1335 ms | 7613 ms | 1334 ms |
| a1-nav-animation | median | 1345 ms | 7595 ms | 1344 ms |
| a2-minimal-row | 1 | 21 ms | 503 ms | 21 ms |
| a2-minimal-row | 2 | 51 ms | 620 ms | 51 ms |
| a2-minimal-row | 3 | 19 ms | 529 ms | 19 ms |
| a2-minimal-row | median | 21 ms | 529 ms | 21 ms |
| a3-markdown-highlight | 1 | 1348 ms | 7738 ms | 1346 ms |
| a3-markdown-highlight | 2 | 2767 ms | 11556 ms | 2765 ms |
| a3-markdown-highlight | 3 | 1325 ms | 7627 ms | 1323 ms |
| a3-markdown-highlight | median | 1348 ms | 7738 ms | 1346 ms |

## Median Comparison

| Variant | Median max schedule gap | Reduction vs baseline | Median first-page load to factory push | Verdict |
| --- | ---: | ---: | ---: | --- |
| baseline | 1346 ms | 0.0% | 7855 ms | Baseline reference |
| a1-nav-animation | 1345 ms | 0.1% | 7595 ms | Not confirmed |
| a2-minimal-row | 21 ms | 98.4% | 529 ms | Confirmed |
| a3-markdown-highlight | 1348 ms | -0.1% | 7738 ms | Not confirmed |

## Interpretation

The dominant cause is GTK transcript row realization/layout after the factory guard is dropped, not synchronous Rust row construction. Minimal labels removed enough GTK row complexity to reduce the median max schedule gap by at least 70%, while Markdown/highlight bypass alone did not.

## Recommendation For #132

Rewrite #132 around schedule-gap-driven batching or lazy row realization, because `push_duration_ms` under-measures the real GTK work that happens after `drop(guard)`.

## Hygiene

- Phase A throwaway patches were not committed.
- No fix was implemented.
- No transcript content, tool call payload, command output, or Markdown body text was logged.
