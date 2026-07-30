# Final Strict Kimi Inputs Report

## Outcome

Implemented the final three Important Kimi findings on `feat/kimi-code`.

- Agent evidence now has explicit absent, valid-ID, and invalid states. Every recognized carrier must be a nonblank string or valid anchored textual ID, all present evidence must agree, and the ID must identify one unused immediate child. Invalid, unknown, conflicting, ambiguous, and consumed evidence remains a generic `Agent` tool call and cannot use chronological fallback.
- Kimi discovery failures from `read_dir`, directory-entry iteration, and `file_type` now pass through one accounting helper. Each diagnostic increments discovery errors exactly once, and production source results derive `Degraded` or `Failed` status from that count.
- Kimi dependency validation now requires expected agent path components to be real directories and declared journals to be regular non-symlink files before fingerprinting or opening. Invalid child paths report the offending path and preserve the previously indexed bundle.

## Root Causes

- Agent evidence used `(present, HashSet<String>)`, so a null structured carrier plus one valid text carrier collapsed to a matchable singleton ID.
- Three directory-enumeration error branches pushed diagnostics but did not increment `KimiDiscovery.errors`.
- Child journals were checked for path containment and symlinks but not regular-file type, allowing a FIFO to reach blocking journal-open logic.

## TDD Evidence

The new regressions were observed failing before production changes:

- `null_structured_and_valid_text_agent_evidence_remains_generic` linked the child unexpectedly.
- `discovery_marks_file_type_failures_incomplete_and_reports_them` reported `discovery.errors == 0` instead of `1`.
- `declared_child_fifo_is_diagnosed_without_blocking_and_preserves_bundle` hit the two-second timeout.

After implementation, focused parser tests passed 28/28 per crate target, the discovery regression passed per crate target, and the bounded FIFO regression passed 1/1.

## Verification

- `cargo test kimi -- --nocapture`: passed; 91 matching tests across unit and integration targets, 0 failed.
- `cargo test --test kimi_code_integration -- --nocapture`: 12 passed, 0 failed.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --all -- -D warnings`: passed.
- `xvfb-run -a env GDK_BACKEND=x11 GSK_RENDERER=cairo cargo test --all --no-fail-fast`: 1,693 passed, 0 failed, plus 0 doc tests.

## Commit

Commit message: `fix: enforce strict Kimi bundle inputs`

## Concerns

- The Unix FIFO regression invokes the standard `mkfifo` executable and is gated with `#[cfg(unix)]`.
- The full headless suite still emits pre-existing GTK/Adwaita runtime warnings; they did not cause test failures and are unrelated to these Kimi changes.
