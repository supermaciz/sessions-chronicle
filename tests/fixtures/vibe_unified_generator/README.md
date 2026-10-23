# Mistral Vibe Unified fixture generator

`generate.py` is the **independent stdlib oracle** for the Unified session
fixtures: it writes stores the way upstream Vibe 3.0.x writes them, following
the pinned upstream facts in
`docs/superpowers/plans/2026-10-23-mistral-vibe-unified.md` (facts A–F, H, K).
It never imports or calls the Rust reader — the two implementations are
deliberately independent so each can validate the other.

Store writing uses only `json`, `hashlib` and `pathlib` (plus `datetime` to
format the fixed fixture timestamps — never the wall clock — and `argparse`
for the command line). Running it twice produces byte-identical output:
re-running after committing yields **no `git diff`**.

## Usage

From the repository root:

```console
$ python3 -I tests/fixtures/vibe_unified_generator/generate.py
```

Writes (overwriting in place, never deleting):

- 7 valid stores under `tests/fixtures/vibe_sessions/unified/`
- the legacy pair `tests/fixtures/vibe_sessions/session_20261020_100000_bbbbbbbb/`
  (`meta.json` + `messages.jsonl`) that the migrated session's
  `import_provenance` points at
- 18 variant stores under `tests/fixtures/vibe_unified_variants/<name>/unified/`
  plus the sibling `tests/fixtures/vibe_unified_variants/growing/append.jsonl`

and prints the derived child session IDs (pinned fact J:
`child_session_id(parent, action_id) = "child-" + sha256(parent + "\0" +
action_id)[:24]`):

```text
child session ids:
  CHILD_A (aaaaaaaa-0000-4000-8000-000000000302 / act-spawn-explicit) = child-ea9b00b3ad4061f4ca36fd81
  CHILD_B (aaaaaaaa-0000-4000-8000-000000000302 / act-spawn-derived) = child-600cf6121578955c8405618d
  CHILD_C (aaaaaaaa-0000-4000-8000-000000000301 / act-spawn-fork)    = child-4c966d1f1f932580b090cfb5
```

Rust test modules repeat the store IDs as constants and re-derive the child
IDs with the reader's `child_session_id`; the generator output is the
cross-check.

## Replay

```console
$ python3 -I tests/fixtures/vibe_unified_generator/generate.py --replay tests/fixtures/vibe_sessions/unified
aaaaaaaa-0000-4000-8000-000000000004 entries=4
aaaaaaaa-0000-4000-8000-000000000300 entries=4
aaaaaaaa-0000-4000-8000-000000000301 entries=3
aaaaaaaa-0000-4000-8000-000000000302 entries=24
child-4c966d1f1f932580b090cfb5 entries=2
child-600cf6121578955c8405618d entries=2
child-ea9b00b3ad4061f4ca36fd81 entries=3
```

The replay is an independent validation (facts A–C, E–F; no Rust, no shared
store code): it checks the canonical `CURRENT` pointer, the manifest digest
and version fields, document and pooled-chunk digests, the journal digest
chain by the rule-E slicing, sequence continuity, and replays every
projection record (`projection_advanced`, `projection_delta`,
`session_relocated` ops, watermark monotonicity) before printing the final
entry count.

`aaaaaaaa-0000-4000-8000-000000000302` replays to **24 entries**: 17 snapshot
entries plus `e18`, `e18b`, `effect-act-gh`, `assistant-act-c`, `e20b`,
`effect-act-write`, `effect-act-todo`; the provisional
`reasoning-act-c-0` was removed by journal record 8. The final unterminated
journal fragment (`{"payload":{"delta":[{"entry":{"type":"message"`) is
present in the segment and ignored, as upstream does.

Replaying `tests/fixtures/vibe_unified_variants` intentionally reports the
two corruption variants (`unknown-op`, `unknown-record`) on stderr and exits
1; every other variant replays cleanly. `rollover/gen1` (journal replay) and
`rollover/gen2` (snapshot) both end at 4 entries — the rollover invariant.

## Store layout produced

```
<store>/
├── CURRENT                                  # canonical pointer + \n
├── meta.json                                # optional, pretty (never indexed by digests)
├── chunks/<sha256>.json                     # pooled history chunks (chunked stores only)
├── journal/<first-sequence:016d>.jsonl      # records + optional unterminated tail
├── tool-results/<percent-encoded name>      # exact text, no trailing newline
└── generations/<generation:016d>/
    ├── manifest.json                        # canonical; digest goes into CURRENT
    ├── projection-state.json                # snapshot; history.entries [] when chunked
    ├── runtime-state.json                   # identity, session_metadata, lifecycle, …
    └── checkpoint.json                      # {"checkpoint_version":1,"context":{"messages":[]}}
```

The projection watermark starts at the snapshot sequence and every projection
record carries its own sequence number as the watermark (monotonic). All
entry timestamps derive from `T0 = 1792700000000` ms, `+1000` per entry —
never from the clock.

## Spot-check with the upstream algorithm

Re-derive the digests of one store from its bytes only (manifest digest from
`CURRENT`, chunk digests, every journal record digest by the rule-E slicing —
not by rebuilding records). Recorded verbatim; expected output below the
command:

```console
$ python3 -I - <<'PY'
import hashlib, json, pathlib
store = pathlib.Path("tests/fixtures/vibe_sessions/unified/aaaaaaaa-0000-4000-8000-000000000302")
sha = lambda b: hashlib.sha256(b).hexdigest()
pointer = json.loads((store / "CURRENT").read_bytes())
manifest_raw = (store / "generations" / pointer["generation"] / "manifest.json").read_bytes()
assert sha(manifest_raw[:-1]) == pointer["manifest_sha256"], "manifest digest mismatch"
print("manifest digest OK:", pointer["manifest_sha256"])
manifest = json.loads(manifest_raw)
for digest in manifest["projection_state"]["chunks"]:
    raw = (store / "chunks" / f"{digest}.json").read_bytes()
    assert sha(raw[:-1]) == digest, f"chunk digest mismatch: {digest}"
    print("chunk digest OK:", digest)
segment = manifest["recovery_journal_segment"]
data = (store / segment["path"]).read_bytes()
expected, previous, position, records = segment["first_sequence"], None, 0, 0
while True:
    newline = data.find(b"\n", position)
    if newline == -1:
        break
    line = data[position:newline]; position = newline + 1
    record = json.loads(line)
    start = line.rfind(b',"record_sha256":"') + len(b',"record_sha256":"')
    embedded = line[start:start + 64].decode("ascii")
    assert line[start + 64:start + 65] == b'"'
    assert sha(line[:start] + line[start + 65:]) == embedded == record["record_sha256"]
    assert record["sequence"] == expected
    assert record["previous_record_sha256"] == previous
    previous, expected, records = embedded, expected + 1, records + 1
print(f"journal records OK: {records} records, sequences "
      f"{segment['first_sequence']}..{expected - 1}")
print("tail fragment (ignored):", data[position:])
print("ALL MATCH")
PY
manifest digest OK: 9b4ce556cc23b208f642a29be352b8945709c2e5f4ce0d65cea651cbe6542c64
chunk digest OK: ee04c94e9aa8764458e75c29670637d2da3122491650415692f419bf19aef571
chunk digest OK: edec72bd40d066ff5d065f04650bd31ee206ce5cc2f784107e976d4d9e5b7af3
journal records OK: 6 records, sequences 5..10
tail fragment (ignored): b'{"payload":{"delta":[{"entry":{"type":"message"'
ALL MATCH
```

## Growing-store drill

`tests/fixtures/vibe_unified_variants/growing/append.jsonl` is the next valid
journal record (sequence and previous digest chained onto the segment,
newline-terminated). Copy the variant to a temporary directory, replay
(2 entries), append the file to the store's journal segment, replay again
(3 entries):

```console
$ cp -r tests/fixtures/vibe_unified_variants/growing /tmp/growing
$ cd /tmp/growing
$ python3 -I <repo>/tests/fixtures/vibe_unified_generator/generate.py --replay unified
cccccccc-0000-4000-8000-000000000004 entries=2
$ cat append.jsonl >> unified/cccccccc-0000-4000-8000-000000000004/journal/0000000000000002.jsonl
$ python3 -I <repo>/tests/fixtures/vibe_unified_generator/generate.py --replay unified
cccccccc-0000-4000-8000-000000000004 entries=3
```

## Fixture inventory

| Store (full ID) | Purpose |
|---|---|
| `aaaaaaaa-0000-4000-8000-000000000302` | 3.0.2 shape: chunked history (9+8), skill invocation, redacted reasoning, failed/completed/cancelled/skipped effects, subagent spawns, 3.0.2 receipt with percent-encoded name, notices, checkpoints, discarded assistant message, journal records 5–10 + unterminated tail |
| `child-ea9b00b3ad4061f4ca36fd81` | subagent of `…302` (`act-spawn-explicit`), sandbox receipt, token usage, no `meta.json` |
| `child-600cf6121578955c8405618d` | subagent of `…302` (`act-spawn-derived`), `tokenUsage: null` |
| `aaaaaaaa-0000-4000-8000-000000000300` | 3.0.0 shape: unchunked, `projection_advanced`, duplicate-id `set_history_entries`, `replace_entry`, 3.0.0 receipt (`bash-call-300.json`), latestTurn failed |
| `aaaaaaaa-0000-4000-8000-000000000301` | fork (root = parent = `…302`), latestTurn interrupted, spawn of `child-4c966d1f1f932580b090cfb5` |
| `child-4c966d1f1f932580b090cfb5` | subagent of the fork |
| `aaaaaaaa-0000-4000-8000-000000000004` | legacy import (`import_provenance.source.backend = "legacy"`, session `bbbbbbbb-0000-4000-8000-000000000004`) |
| `session_20261020_100000_bbbbbbbb/` | legacy pair the import points at (outside `unified/`) |
| variants `ephemeral`, `deleting/active`, `deleting/deleting`, `rollover/gen1`, `rollover/gen2`, `growing`, `unknown-op`, `unknown-record`, `unknown-entry`, `unknown-checkpoint`, `unknown-status`, `deeper`, `empty`, `recovered` | minimal stores (IDs `cccccccc-0000-4000-8000-0000000000NN`) exercising one behavior each; `unknown-*` are intentionally rejected by a correct reader |

Note: these fixtures temporarily change the legacy Vibe session count under
`tests/fixtures/vibe_sessions/` (the unified reader ships in a later task),
so legacy count assertions are updated by the indexer task, not here.
