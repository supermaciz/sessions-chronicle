# Session Detail Issue 143 Benchmark Report

## Context

- Issue: [#143 — Keep large transcript opens responsive by deferring heavy row content](https://github.com/supermaciz/sessions-chronicle/issues/143)
- Pull request under evaluation: [#144 — feat: lazily hydrate session detail transcript rows](https://github.com/supermaciz/sessions-chronicle/pull/144)
- Branch: `lazy-rows`
- What the branch implements:
  - A row hydration contract that creates transcript rows as lightweight shells and defers heavy content (TextView, Markdown, syntax highlighting) until the row enters the viewport.
  - Viewport-driven hydration queue with epoch tracking and stale-state guards.
  - Anchor compensation so deferred hydration does not visually jump or lose scroll position.
  - Hydration of search targets before scroll-to-match (#130 interaction).
  - A new ignored shell-weight gate test (`session_detail_issue_142_shell_weight_gate_reference_session`) used as the dedicated benchmark for issue #143.

This report compares the branch behavior on the reference Codex session against the historical #142 baseline, both end-to-end (manual full-app rerun) and through the new shell-only gate.

## Scope

- Date: 2026-05-05
- Target session: `019dc51a-f0cd-79c1-ba79-45fedac889c2`
- AI assistant: Codex
- Historical baseline: `docs/reports/2026-05-02-session-detail-issue-142-investigation-report.md`
- Branch under test: `lazy-rows`
- Branch HEAD during measurement: `6500b06`
- Measurement types:
  - real full-app manual rerun
  - shell-only gate test added for issue #143

## Protocol

### A. Full-app manual rerun

Run command pattern used for each run:

```bash
RUST_LOG=info,sessions_chronicle=debug cargo run > /tmp/opencode/issue143-manual-runN.log 2>&1
```

Scenario:

1. Launch the application from the current branch with debug logs redirected to a file.
2. Wait for background indexing to complete.
3. Click session `019dc51a-f0cd-79c1-ba79-45fedac889c2` exactly once.
4. Do not perform search before the click.
5. Wait for the detail view to settle, then close the app.

Log files:

- `/tmp/opencode/issue143-manual-run1.log`
- `/tmp/opencode/issue143-manual-run2.log`
- `/tmp/opencode/issue143-manual-run3.log`

### B. Shell-only gate rerun

Run command pattern used for each run:

```bash
SESSION_DETAIL_ISSUE_142_SOURCE_FILE=/home/mcizo/.codex/sessions/2026/04/25/rollout-2026-04-25T16-46-10-019dc51a-f0cd-79c1-ba79-45fedac889c2.jsonl \
  cargo test session_detail_issue_142_shell_weight_gate_reference_session -- --ignored --nocapture
```

This path exercises the new ignored shell-weight gate test introduced by issue #143. It measures the first-page push budget for the current lazy-row implementation without the extra variability of a manual click path.

Log files:

- `/tmp/opencode/issue143-benchmark/run-1.log`
- `/tmp/opencode/issue143-benchmark/run-2.log`
- `/tmp/opencode/issue143-benchmark/run-3.log`

## Historical Baseline (#142)

From `docs/reports/2026-05-02-session-detail-issue-142-investigation-report.md`:

| Variant | Run | Max schedule gap | First-page load to factory push | Max post-drop residual |
| --- | ---: | ---: | ---: | ---: |
| baseline | 1 | 1335 ms | 7855 ms | 1334 ms |
| baseline | 2 | 1346 ms | 7652 ms | 1346 ms |
| baseline | 3 | 1377 ms | 7885 ms | 1376 ms |
| baseline | median | 1346 ms | 7855 ms | 1346 ms |

## Current Branch Results

### Full-app manual rerun

| Variant | Run | Max schedule gap | First-page load to factory push | Max post-drop residual |
| --- | ---: | ---: | ---: | ---: |
| issue-143-full-app | 1 | 1364 ms | 7909 ms | 1364 ms |
| issue-143-full-app | 2 | 1346 ms | 7914 ms | 1346 ms |
| issue-143-full-app | 3 | 1412 ms | 8146 ms | 1412 ms |
| issue-143-full-app | median | 1364 ms | 7914 ms | 1364 ms |

### Median comparison vs historical baseline

| Variant | Median max schedule gap | Delta vs baseline | Median first-page load to factory push | Delta vs baseline | Verdict |
| --- | ---: | ---: | ---: | ---: | --- |
| baseline (#142) | 1346 ms | 0.0% | 7855 ms | 0.0% | Historical reference |
| issue-143-full-app | 1364 ms | +1.3% | 7914 ms | +0.8% | No full-app improvement in manual rerun |

The manual rerun stays effectively flat against the historical #142 baseline. The current lazy-row implementation does not show a measurable win on the full end-to-end click path under this protocol.

## Shell-Only Gate

| Variant | Run | Max schedule gap | First-page load to factory push | Max post-drop residual |
| --- | ---: | ---: | ---: | ---: |
| issue-143-shell-only | 1 | 18 ms | 455 ms | 18 ms |
| issue-143-shell-only | 2 | 17 ms | 450 ms | 17 ms |
| issue-143-shell-only | 3 | 18 ms | 455 ms | 18 ms |
| issue-143-shell-only | median | 18 ms | 455 ms | 18 ms |

Gate verdict:

- Required by the design: `median max_schedule_gap_ms <= 50 ms`
- Observed: `18 ms`
- Result: pass

## Supporting Metrics

The new metrics added by this PR support a narrower interpretation than the manual full-app rerun alone:

- The shell-only gate is decisively green. The first-page push budget on the reference session dropped from the historical `1346 ms` median baseline to `18 ms` in the dedicated shell-only path.
- The shell-only runs show `display_item_count=33`, `batch_count=11`, and `total_row_build_duration_ms=0`, which confirms that the current row shells are cheap enough under the dedicated test harness.
- The manual full-app reruns still show `max_post_drop_residual_ms` equal to `max_schedule_gap_ms` (`1364`, `1346`, `1412`). As in earlier investigations, the dominant cost still looks like GTK or broader main-loop work after the factory guard is dropped, not synchronous row-build time.
- The new lazy hydration metrics are therefore useful as supporting evidence that the shell itself is no longer the blocking cost, even though the complete interactive app path still does not improve under this manual protocol.

## Interpretation

This rerun splits issue #143 into two distinct outcomes:

1. The new shell-only gate passes comfortably. The lazy-row shell design achieves the explicit shell-weight target from the issue #143 design.
2. The real full-app manual click path does not improve versus the historical #142 baseline under the protocol used here.

Taken together, that means the PR successfully removes the shell construction cost as the dominant problem in the dedicated gate, but it does not yet move the end-to-end manual benchmark for the reference session.

The strongest reading is:

- row shells are now cheap enough;
- deferred hydration mechanics are functionally in place;
- but the manual full-app path is still dominated by scheduling or other GTK main-loop behavior outside the narrow shell-only gate.

## Verdict

- Shell-only gate for issue #143: pass
- Manual full-app rerun vs #142 baseline: no measurable improvement
- Recommendation: keep the shell-only gate result as proof that the PR meets its narrow shell-weight goal, but do not present the PR as a full end-to-end benchmark win on the reference click path.

## Decision

PR #144 is **closed without merging**.

Rationale:

- Acceptance criterion #1 of issue #143 targets the reference session under realistic open conditions (median `max_schedule_gap_ms` ≤ 50 ms). The full-app manual rerun on this branch stays at 1364 ms median, statistically indistinguishable from the 1346 ms #142 baseline. The criterion is not met.
- The shell-only gate passes comfortably (18 ms), which is useful evidence that row shell construction is no longer the dominant cost — but that gate was introduced by this same PR and is narrower than the issue's stated acceptance condition. Merging on the strength of a self-introduced synthetic test would mislabel the work as a fix for #143.
- The remaining cost looks like GTK or main-loop work that occurs after the factory guard is dropped, which is outside the scope this PR was designed to address.

Issue #143 stays open. The lazy-row infrastructure on this branch is preserved on the `lazy-rows` branch and in this report; if a future PR revisits the problem, the shell-weight gate and the deferred hydration scaffolding can be reused or rebuilt with the lessons captured here.

### Suggested follow-ups

- [#138 — Explore off-main-thread transcript data preparation for SessionDetail responsiveness](https://github.com/supermaciz/sessions-chronicle/issues/138): the most likely owner of the residual main-loop cost.
- [#132 — Use adaptive render batches in SessionDetail](https://github.com/supermaciz/sessions-chronicle/issues/132): complementary mitigation for scheduling gaps when a large initial batch is pushed.
- [#134 — Explore transcript virtualization or windowing for very large SessionDetail views](https://github.com/supermaciz/sessions-chronicle/issues/134): structural alternative if deferred hydration alone cannot hit the 50 ms target.

## Hygiene

- The reference session file used for the shell-only gate was `/home/mcizo/.codex/sessions/2026/04/25/rollout-2026-04-25T16-46-10-019dc51a-f0cd-79c1-ba79-45fedac889c2.jsonl`.
- No transcript content, tool call payload, command output, or Markdown body text was copied into this report.
