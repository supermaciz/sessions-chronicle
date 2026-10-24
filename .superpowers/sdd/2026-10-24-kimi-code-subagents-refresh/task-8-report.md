# Task 8 report: Real-shape fixtures and integration tests

## Outcome

Added real-shape Kimi fixture coverage for system-triggered child prompts, Git-context stripping, two-item AgentSwarm expansion with quoted tag-like result content, and resumed Agent calls sharing one child. Parser production behavior was already implemented; Task 8 changed fixtures and integration/count assertions only.

The primary `state.json` change is intentional: the SDD ledger ruling says to add the requested `parentAgentId` and `profileName` labels even though the brief's Files list omitted that file.

## TDD and test evidence

### RED: tests before adding swarm/resume fixtures

Command:

```sh
dbus-run-session -- xvfb-run -a env GDK_BACKEND=x11 GSK_RENDERER=cairo cargo test --test kimi_code_integration -- --nocapture
```

Log: `task-8-red.log`.

Observed: **11 passed, 9 failed** against the original fixtures. The intended failures included the expected bundle count (`7` vs `9`), missing swarm transcript rows, no three-call resume link, and no swarm snapshot. `primary_child_title_omits_git_context` passed initially because the original child prompt did not yet contain a Git-context block; this is recorded rather than represented as a RED. No parser implementation was changed.

Actual test summary:

```text
test result: FAILED. 11 passed; 9 failed; 0 ignored
```

The complete failing test names and assertion output are preserved in the RED log.

### GREEN: fixtures and integration assertions

Command:

```sh
dbus-run-session -- xvfb-run -a env GDK_BACKEND=x11 GSK_RENDERER=cairo cargo test --test kimi_code_integration -- --nocapture
```

Log: `task-8-final-integration.log` (the earlier GREEN run is retained as `task-8-green.log`).

Observed: **20 passed, 0 failed**.

Actual test summary:

```text
test result: ok. 20 passed; 0 failed; 0 ignored
```

### Full headless workspace verification

The first full run found three stale fixture-count assertions outside the integration file, all in Kimi indexer tests. The observed assertion values were `10 vs 8` discovery candidates, `8 vs 6` indexed while one fixture is incomplete, and `9 vs 7` initial incremental indexing. The counts were updated; there was no parser behavior failure.

Final command:

```sh
dbus-run-session -- xvfb-run -a env GDK_BACKEND=x11 GSK_RENDERER=cairo cargo test --all --no-fail-fast -- --nocapture
```

Final log: `task-8-final-full-suite.log` (the previous passing rerun is also retained as `task-8-full-suite-green.log`).

Observed: **all workspace targets passed** (including Kimi integration: 20 passed; core library: 689 passed, 1 ignored; shell search: 10 passed; search-provider library: 9 passed; private-bus integration: 2 passed). One real-store test is intentionally ignored. The earlier run and its three count-only failures are in `task-8-full-suite.log`.

Additional checks:

- `cargo fmt --all -- --check` — passed (`task-8-final-fmt.log`).
- `cargo clippy --all -- -D warnings` — passed (`task-8-final-clippy.log`).
- `git diff --check` — passed.

The test harness printed a headless xdg-desktop-portal/secret-service warning after tests; it did not affect results.

## Files changed

- `tests/kimi_code_integration.rs`: bundle totals, generic bundle snapshot helper, and swarm/resume/title/reindex stability integration tests.
- `crates/core/src/database/indexer/kimi.rs`: fixture-derived `indexed`, `skipped`, and discovery candidate expectations only.
- Primary fixture: `tests/fixtures/kimi_home/sessions/wd_primary_aaaaaaaaaaaa/session_00000000-0000-4000-8000-000000000001/state.json` and `agents/{agent-0,agent-1,agent-nested}/wire.jsonl`.
- Swarm fixture: `tests/fixtures/kimi_home/sessions/wd_swarm_999999999999/session_00000000-0000-4000-8000-000000000009/{state.json,agents/main/wire.jsonl,agents/agent-0/wire.jsonl,agents/agent-1/wire.jsonl}`.
- Resume fixture: `tests/fixtures/kimi_home/sessions/wd_resume_101010101010/session_00000000-0000-4000-8000-000000000010/{state.json,agents/main/wire.jsonl,agents/agent-0/wire.jsonl,agents/agent-1/wire.jsonl}`.
- `.superpowers/sdd/2026-10-24-kimi-code-subagents-refresh/progress.md`: marked Task 8 complete and recorded the verification note.
- `.superpowers/sdd/2026-10-24-kimi-code-subagents-refresh/task-8-report.md`: this execution report.

## Concerns / scope

- No production parser code changed.
- Pre-existing `src/config.rs` modification and unrelated untracked files were left untouched and are excluded from the scoped commit.
- Verbose command logs are retained beside this report; they are not staged as source changes.
