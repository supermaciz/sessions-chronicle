# Task 7 Report: Unified signature and indexer integration

## Status

**Partial; not ready to claim Task 7 complete.** Core discovery/indexing and signature wiring are present and the legacy fixture count is updated. The brief's complete required integration-test inventory, several lifecycle edge cases, the status precedence regression test, and full lint cleanliness remain unfinished. This checkpoint preserves the work for continuation rather than misrepresenting acceptance.

## Changes made

- Added `crates/core/src/parsers/mistral_vibe_unified/signature.rs` with `UNIFIED_READER_VERSION`, serializable signature fields, storage conversion, and freshness checks for CURRENT/journal/meta/results.
- Exposed `FileSample` and `FileStat` as public types (without lint suppression) so public signature fields have reachable interfaces. `FileStat::for_path` is shared with consumed-result stats.
- `UnifiedParse` now carries a signature created inside each stable-load attempt from CURRENT bytes, the pre-read journal sample, optional meta stat, and consumed saved-result stats.
- Added schema migration v21 and signature clearing to `clear_all_sessions`.
- Added Unified discovery/indexing, incremental signature skipping, transaction-wrapped upsert/content replacement/signature save, bidirectional child linking, lifecycle/error handling, and deletion sweep in `crates/core/src/database/indexer/vibe_unified.rs`.
- Added legacy Vibe same-ID collision protection and explicit parsed ending-status precedence in `indexer.rs`.
- Updated the fixture count from 2 to 10.

## TDD / verification

- RED: the existing count test was run against the integrated indexer and failed as expected (`left: 10, right: 2`); updating its expected count then made it pass.
- The signature unit test verifies storage round-trip, successful current signature, and rejection after CURRENT changes. It was added during implementation rather than verified as a clean isolated RED-first cycle. The full brief's named integration tests were not written, so this report does not claim full TDD coverage.
- `cargo test -p sessions-chronicle-core mistral_vibe_unified`: PASS, 102 passed.
- `cargo test -p sessions-chronicle-core mistral_vibe_indexing_indexes_sessions`: PASS.
- Full required headless suite: **PASS**, core lib 554 passed; shell search 10 passed; search provider 9 passed; integration tests 2 passed; doc tests passed. The first run caught an accidental expected-count edit in `opencode_incremental_detects_new_rows_in_sqlite_wal`; after restoring that unrelated assertion to 2, the full rerun passed.
- `cargo fmt --all -- --check`: passed as the first half of the combined fmt/clippy command.
- `cargo clippy --all -- -D warnings`: fails on the three inherited dead-code findings in Unified store (`Identity::Fork.parent`; `RuntimeView.session_id` and `imported_from_legacy`; `LoadedStore.snapshot_sequence`). Per the controller request, no blanket allow was added; resolving them naturally remains open.

## Remaining acceptance work / concerns

- Add the brief's indexer integration tests, including realistic temporary fixture copies, corruption/reindex setup, skipped-parent linking, explicit child identity, saved-result mutation, race/deletion-order coverage, collision and pin preservation.
- Verify lifecycle cleanup does not retain stale side rows/signatures on every path. Deletion removal accounting and explicit folder-enumeration failure behavior need focused tests.
- Add `explicit_parser_ending_status_wins_over_last_tool_call` and migration test for v21.
- Re-run full headless suite after fixing the accidental unrelated assertion edit; run clippy after naturally consuming/removing the three unused fields.
- `progress.md` was not modified. `src/config.rs` and user untracked files were left untouched.

## Interface notes

Task 6's `SavedResultResolver::into_consumed() -> Vec<(String, FileStat)>` feeds the signature inside the `load_stable` closure, preserving per-attempt isolation. Public signature members require their component types to be publicly reachable; therefore `FileSample` and `FileStat` were made public rather than suppressing private-interface lints.
