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

---

## Continuation: Task 7 acceptance completed (2026-10-23)

This section supersedes the partial status above. The original checkpoint `7844ed5` was preserved, not rewritten. All tests named by the brief now exist and run, with additional lifecycle, atomicity, binding, collision, signature, and source-disappearance coverage. No subagents or worktrees were used. `progress.md`, `src/config.rs`, and unrelated untracked user files were not intentionally edited or staged.

### Implementation corrections

- Extracted the actual parse outcome handler so deferral/error/success transitions can be tested directly without fabricating filesystem races.
- Delay publication of successfully parsed children until all root lifecycle outcomes are known. A deleting root suppresses its children regardless of enumeration order; the deterministic order test installs an INSERT trigger which rejects even transient child reinsertion.
- Tree removal gathers owned Unified rows, deduplicates listed/owned paths, clears incoming links, removes all content side rows, and deletes every affected signature in one transaction. Removal counts reflect actual deleted rows. Reset deferrals only after commit. Listed paths are validated and constrained to the same Unified source, avoiding deletion of a legacy collision or traversal outside the root.
- Physical deletion removes only confirmed absent direct directories. Missing/unreadable Unified roots preserve rows. Recheck root accessibility before sweeping: a root which vanishes after initial enumeration is not session-deletion evidence. `try_exists` errors also preserve rows.
- Ephemeral and no-user outcomes now atomically clear prior content, incoming child links, signatures, and deferrals.
- Missing CURRENT in an indexed surviving directory is an `Incomplete` diagnostic, preserving the prior row and signature.
- Optional meta freshness checks appearance, mutation, and disappearance, including errors other than a genuinely absent optional file.
- Child linkage requires an indexed Unified Mistral Vibe depth-one child, matching root ownership, a non-subagent parent, and the exact deterministic spawn-action identity. This validates explicit child annotations too; same-root but wrong-action children cannot bind in either update direction.
- The Unified same-ID guard runs inside the publishing transaction. Both legacy-to-Unified and Unified-to-legacy collision directions have tests.
- Removed unnecessary retained fork lineage, imported legacy lineage, and duplicate snapshot sequence fields; validation remains in the loader. Conversion consumes the already-validated runtime session ID. Existing tests were updated to assert retained generation/journal facts rather than removed duplicate fields. No lint suppressions were added.
- Publicly re-exported `UnifiedSignature`, `UNIFIED_READER_VERSION`, `FileSample`, and `FileStat` so the public parse/signature interface is reachable.

### Honest baseline and RED→GREEN record

The inherited implementation already existed. It was NOT deleted or temporarily reverted to manufacture failing tests. Baseline-green acceptance tests are characterization coverage, not retroactive TDD; the original implementation-first violation remains disclosed above.

1. Initial new indexer inventory: `dbus-run-session -- xvfb-run -a env GDK_BACKEND=x11 GSK_RENDERER=cairo cargo test -p sessions-chronicle-core database::indexer::vibe_unified -- --nocapture` → **17 passed, 4 failed** (`/tmp/opencode/task7-baseline.log`). Three failures were product defects (tree removal accounting, retained ephemeral signature, missing CURRENT error classification); the fourth was an incorrect migration test expectation, corrected to the fixture's French texts rather than changing production behavior.
2. Expanded baseline: same headless command with `database::` filter → **191 passed, 6 failed** (`/tmp/opencode/task7-baseline2.log`). The six genuine regressions were:
   - `deleting_tree_removes_root_and_children`
   - `ephemeral_store_is_not_indexed`
   - `absent_current_in_indexed_dir_is_diagnosed_and_kept`
   - `explicit_child_binding_must_match_spawn_action`
   - `meta_json_appearing_invalidates_signature`
   - `no_user_messages_prunes_contents_links_and_signature`
   All six were verified GREEN after their fixes (`/tmp/opencode/task7-green1.log`; final suites below).
3. Additional regression `unified_root_disappearing_after_enumeration_never_prunes_rows` → **RED: removed 7 instead of 0** (`/tmp/opencode/task7-source-race-red.log`), then GREEN after the accessibility recheck (final suites below).
4. Additional tests added around existing behavior (deferrals, atomicity, reverse collision, malformed/old signatures, missing source, and forced enumeration order) are explicitly baseline-green/characterization or strengthened regression coverage, not claimed as independent RED cycles. The outcome handler/candidate iterator extraction is a behavior-preserving testability refactor.
5. Test setup issues corrected during development: the reverse-collision legacy metadata initially omitted required `start_time`; a strengthened migration preservation seed initially omitted required message `timestamp`. These were test setup failures, not production defects. The first full repository run failed only the migration seed; final full reruns are green. An intermediate extraction syntax error and stale test references to removed fields were corrected before successful verification. Initial Clippy found needless borrows introduced by handler extraction; fixed without suppression.

### Complete integration-test inventory and coverage

Tests live under `crates/core/src/database/indexer/vibe_unified/tests.rs`, included by `indexer/vibe_unified.rs`. Temporary copies are used throughout; source fixtures are never modified.

| Test | Coverage / baseline |
|---|---|
| `indexes_unified_and_legacy_side_by_side` | 10 rows; R302 clean ending and activity counts (2,1,1); baseline GREEN |
| `explicit_and_derived_children_are_linked` | R302 A/B/NULL and F301 C; baseline GREEN |
| `child_indexed_before_parent_is_linked` | Both arrival orders, including skipped parent later linked by new child; baseline GREEN |
| `deeper_nesting_is_not_linked` | Grandchild row exists but child cannot link to it; baseline GREEN |
| `incremental_noop_skips_all_unified` | All 10 skipped, none indexed, seven Unified signatures; baseline GREEN |
| `journal_append_reindexes_only_that_session` | Only growing session reparsed, Late answer visible once, next pass no-op; baseline GREEN |
| `meta_json_change_reindexes` | Existing meta changes and removal; baseline GREEN |
| `saved_result_touch_reindexes` | Identical bytes rewritten and explicit mtime bump, only R302 reparsed; baseline GREEN |
| `favorite_survives_reindex` | Public toggle_pin, append, Late answer and pin retained; baseline GREEN |
| `failure_keeps_previous_row` | Corrupt R302 chunk, exactly one path-specific error, full row/messages/signature retained, nine unaffected sessions indexed, restored full pass indexes ten; baseline GREEN |
| `corrupt_child_never_indexed_is_not_linked` | CHILD_B's temporary inline projection is first pooled into a valid chunk and successfully parsed, then that chunk is corrupted; no child row or link, exactly one diagnostic; baseline GREEN |
| `ignores_scratchpad_catalog_and_staging_entries` | Scratchpad, catalog file, hidden staging, invalid directory ID ignored without errors; baseline GREEN |
| `missing_saved_result_then_restored` | Required percent-encoded result disappearance keeps previous contents/signature and reports once; restoration reparses only affected session; baseline GREEN |
| `unified_pointer_flip_during_read_defers_then_reports` | Real handler: first Unstable skips, second and third report, previous data/signature preserved, successful Ok resets to zero, next race defers again, deletion clears counter; existing behavior GREEN |
| `deleted_dir_removes_row_and_clears_parent_link` | CHILD_B row/all side contents/signature removed, parent link cleared; missing or non-directory Unified root prunes nothing; baseline GREEN |
| `deleting_tree_removes_root_and_children` | Active→deleting, incremental and full modes, exact removal count, no retained contents/signatures, no reinsertion on later pass; RED→GREEN |
| `ephemeral_store_is_not_indexed` | Fresh ephemeral ignored and persistent→ephemeral transition clears old contents/signature; RED→GREEN |
| `absent_current_in_indexed_dir_is_diagnosed_and_kept` | Incomplete diagnostic; messages and signature retained; RED→GREEN |
| `legacy_id_collision_keeps_prior_record` | Legacy indexed first; Unified collision diagnostic names legacy path, prior path/message retained; baseline GREEN |
| `migrated_legacy_and_unified_are_both_indexed` | LEG two messages, MIG exactly four expected texts once each; baseline GREEN after correcting test text |
| `meta_json_appearing_invalidates_signature` | No-meta→meta appearance invalidates freshness; RED→GREEN |
| `explicit_child_binding_must_match_spawn_action` | Same-root, wrong-action explicit child rejected by parent and child updates; RED→GREEN |
| `no_user_messages_prunes_contents_links_and_signature` | Empty child projection prunes every side table/signature and incoming parent link; RED→GREEN |
| `clear_all_sessions_clears_unified_signatures` | All seven signatures cleared; next incremental indexes ten; baseline GREEN |
| `unified_signature_and_contents_commit_atomically` | Signature-write abort rolls back prior content replacement; successful retry indexes appended answer; baseline GREEN |
| `deleting_tree_is_atomic_and_cleans_unlisted_owned_children` | Abort child deletion rolls back root/content/signatures; empty lifecycle list still deletes indexed owned child; baseline GREEN |
| `source_root_absence_never_prunes_unified_rows` | Entire Vibe source missing preserves ten rows/seven signatures; baseline GREEN |
| `legacy_collision_with_existing_unified_keeps_prior_record` | Reverse collision guarded in legacy parser path; Unified contents/signature retained; baseline GREEN after fixing legacy metadata seed |
| `malformed_or_old_reader_signature_forces_reindex` | Invalid JSON and reader version drift each invalidate only one candidate; baseline GREEN |
| `deleting_tree_never_reinserts_children_in_either_enumeration_order` | Forced parent-first AND child-first full parsing with trigger rejecting transient child INSERT; strengthened regression GREEN |
| `unified_root_disappearing_after_enumeration_never_prunes_rows` | Real successful read_dir followed by source rename before candidate processing/sweep; seven rows must not be pruned; RED→GREEN |

Additional required named tests:

- `schema::tests::migration_v21_creates_vibe_unified_signatures`: migrate v20→v21; assert `CURRENT_DB_VERSION == 21`, actual user_version 21, signature table primary-key behavior, preserved fingerprints, session pin, message, and idempotent reopening. Baseline migration implementation GREEN; strengthened data-preservation seed passes after setup correction.
- `indexer::tests::explicit_parser_ending_status_wins_over_last_tool_call`: parser-authoritative Clean/Error override pending final tool call; Abrupt overrides completed final call. Baseline GREEN.
- Existing `insert_parsed_session_*` ending-status tests pass unchanged.
- Existing `mistral_vibe_indexing_indexes_sessions` remains at 10 and passes.
- Existing signature storage/current-byte unit test passes.

### Rulings / scope notes

- The brief's command `cargo test -p sessions-chronicle-core vibe_unified schema` has two positional filters and is not valid Cargo syntax. Used valid single-filter runs plus the entire headless core and repository suites instead.
- Chunk corruption is deliberately tested via nonincremental Vibe indexing (`index_vibe_sessions_internal(..., false, ...)`), preserving the prior database snapshot. The global `index_all_full_reindex` first clears the database, so it cannot test failure preservation. Immutable chunk-only mutations outside CURRENT/journal/meta/result freshness are outside incremental guarantees, per spec.
- CHILD_B is inline in checked-in fixtures; its temp copy is converted to a valid pooled projection before corrupting an actual chunk. This preserves the exact chunk-corruption acceptance scenario without changing fixture generation or committed fixture data.
- Spec-confirmed child binding is stronger than the brief's abbreviated SQL snippets: also verify deterministic spawn identity and Unified source. Otherwise a valid child from the wrong spawn can be linked by an explicit annotation; the RED test demonstrates the defect.
- Cleanup uses the shared transactional content-deletion helper rather than duplicating the legacy nontransactional parse-skip wrapper, so links, all side tables, and signatures remain atomic.
- No Flatpak/build dependency changes were made by this continuation. Plan Task 9's packaging verification remains with Task 9, not a Task 7 blocker.
- Review was self-review only, as the dispatch explicitly prohibited subagents. No unresolved Task 7 blocker or deferred correctness issue was found. Broader branch review belongs to the controller.

### Final verification

All final commands exited **0**, after the final transactional cleanup refactor:

| Command | Result / log |
|---|---|
| `dbus-run-session -- xvfb-run -a env GDK_BACKEND=x11 GSK_RENDERER=cairo cargo test -p sessions-chronicle-core` | **587 core unit + 10 shell-search = 597 passed**, 0 failed/ignored; doc tests pass; `/tmp/opencode/task7-core-final.log` |
| `dbus-run-session -- xvfb-run -a env GDK_BACKEND=x11 GSK_RENDERER=cairo cargo test --all --no-fail-fast` | **1,703 passed across 31 target results**, 0 failed/ignored; app lib 450, app binary 519, core 587, shell-search 10, search-provider 9, private-bus 2, other integration suites 126; doc tests pass; `/tmp/opencode/task7-full-final.log` |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --all -- -D warnings` | PASS, no warnings; `/tmp/opencode/task7-clippy-final.log` |
| `git diff --check` | PASS |

Only Task 7 source/test paths were staged for the authorized implementation commit on `vibe-v3`. This tracked report is committed separately, without editing `progress.md`.

### Authorized commit

- **`06c58c8` — `fix: complete Unified Vibe indexing lifecycle and test coverage`** on `vibe-v3` (continuation of preserved checkpoint `7844ed5`). Commit hook style check passed. Ten explicitly staged Task 7 source/test paths; no user configuration/untracked files included.
- A separate `docs: record Task 7 acceptance and verification results` commit contains only this tracked task report.
- **Final status: Task 7 complete**, with all named acceptance tests and added regressions verified, full headless suites green, fmt/Clippy clean. No Task 7 concerns remain beyond the disclosed inherited implementation-first TDD limitation and lack of independent review in this no-subagent dispatch.
