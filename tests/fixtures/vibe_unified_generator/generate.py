#!/usr/bin/env python3
"""Mistral Vibe Unified session-store fixture generator.

Independent stdlib oracle that writes Unified stores the way upstream Vibe
3.0.x writes them, following the pinned upstream facts in
``docs/superpowers/plans/2026-10-23-mistral-vibe-unified.md`` (facts A-F, H,
K). It never imports or calls the Rust reader: the two implementations are
deliberately independent so each can validate the other.

Store writing uses only ``json``, ``hashlib`` and ``pathlib`` (plus
``datetime`` to format the fixed fixture timestamps, never the wall clock,
and ``argparse`` for the command line). Running the generator twice produces
byte-identical output.

Usage::

    python3 -I tests/fixtures/vibe_unified_generator/generate.py
        Write every fixture (valid stores, the legacy migrated pair, and the
        variant stores) and print the derived child session IDs.

    python3 -I tests/fixtures/vibe_unified_generator/generate.py --replay DIR
        Independently validate and replay every store under DIR (facts A-C,
        E-F) and print ``<session_id> entries=<n>`` per store. Validation
        failures are reported on stderr and make the run exit 1.
"""

import argparse
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

# --------------------------------------------------------------------------
# Fixture IDs (shared contract; later Rust test modules repeat these as
# constants and re-derive the child IDs with the reader's child_session_id).
# --------------------------------------------------------------------------

R302 = "aaaaaaaa-0000-4000-8000-000000000302"  # 3.0.2 shape, chunked
R300 = "aaaaaaaa-0000-4000-8000-000000000300"  # 3.0.0 shape, unchunked
F301 = "aaaaaaaa-0000-4000-8000-000000000301"  # 3.0.1 shape, fork of R302
MIG = "aaaaaaaa-0000-4000-8000-000000000004"  # legacy import
LEG = "bbbbbbbb-0000-4000-8000-000000000004"  # legacy session MIG imports


def child_session_id(parent: str, action_id: str) -> str:
    """Pinned fact J: child id derived from parent id and spawn action id."""
    digest = hashlib.sha256((parent + "\0" + action_id).encode("utf-8")).hexdigest()
    return "child-" + digest[:24]


CHILD_A = child_session_id(R302, "act-spawn-explicit")
CHILD_B = child_session_id(R302, "act-spawn-derived")
CHILD_C = child_session_id(F301, "act-spawn-fork")

# Variant store IDs (each a minimal user+assistant store unless noted).
V_EPHEMERAL = "cccccccc-0000-4000-8000-000000000001"
V_DELETING = "cccccccc-0000-4000-8000-000000000002"
V_DELETING_CHILD = child_session_id(V_DELETING, "act-spawn-del")
V_ROLLOVER = "cccccccc-0000-4000-8000-000000000003"
V_GROWING = "cccccccc-0000-4000-8000-000000000004"
V_UNKNOWN_OP = "cccccccc-0000-4000-8000-000000000005"
V_UNKNOWN_RECORD = "cccccccc-0000-4000-8000-000000000006"
V_UNKNOWN_ENTRY = "cccccccc-0000-4000-8000-000000000007"
V_UNKNOWN_CHECKPOINT = "cccccccc-0000-4000-8000-000000000008"
V_UNKNOWN_STATUS = "cccccccc-0000-4000-8000-000000000009"
V_DEEPER = "cccccccc-0000-4000-8000-00000000000a"
V_DEEPER_CHILD = child_session_id(V_DEEPER, "act-spawn-child")
V_DEEPER_GRANDCHILD = child_session_id(V_DEEPER_CHILD, "act-spawn-grand")
V_EMPTY = "cccccccc-0000-4000-8000-00000000000b"
V_RECOVERED = "cccccccc-0000-4000-8000-00000000000c"

# --------------------------------------------------------------------------
# Fixed fixture time base: every timestamp derives from T0, never the clock.
# --------------------------------------------------------------------------

T0 = 1792700000000  # ms since the Unix epoch


def ts(entry_number: int) -> int:
    """Timestamp of entry ``entry_number`` (1-based): T0 + 1000 per entry."""
    return T0 + 1000 * entry_number


def iso_ms(milliseconds: int) -> str:
    """Deterministic ISO-8601 UTC string (upstream manifest/meta.json style)."""
    moment = datetime.fromtimestamp(milliseconds // 1000, tz=timezone.utc)
    return moment.strftime("%Y-%m-%dT%H:%M:%S") + f".{milliseconds % 1000:03d}Z"


VIBE_ROOT = "/home/fixture/.vibe/logs/session"
CWD_ORIGINAL = "/home/fixture/projects/original"
CWD_MOVED = "/home/fixture/projects/moved"
CWD_VARIANT = "/home/fixture/projects/variant"
PROJECTION_FORMAT = "harness.public-session-state/v1"
STORE_FORMAT = "mistral.vibe.unified-session-store/v1"
STORE_FORMAT_MINOR = 9

# The 14 journal record kinds (pinned fact E).
RECORD_KINDS = (
    "command_reserved",
    "core_input",
    "action_intent",
    "action_result",
    "process_operation_dispatched",
    "process_state_changed",
    "process_notification_submitted",
    "callback_registered",
    "callback_resolved",
    "receipt_succeeded",
    "receipt_failed",
    "projection_advanced",
    "projection_delta",
    "session_relocated",
)


# --------------------------------------------------------------------------
# Canonical JSON (the domain where this equals upstream canonical_json:
# no floats, integers within the canonical safe range, ASCII-only keys).
# --------------------------------------------------------------------------

_MAX_SAFE_INTEGER = 2**53 - 1


def _validate_canonical_domain(value, key=False):
    if value is None or isinstance(value, bool):
        return
    if isinstance(value, float):
        raise ValueError("floats are outside the canonical fixture domain")
    if isinstance(value, int):
        if not -_MAX_SAFE_INTEGER <= value <= _MAX_SAFE_INTEGER:
            raise ValueError(f"integer outside the canonical safe range: {value}")
        return
    if isinstance(value, str):
        if key and not value.isascii():
            raise ValueError(f"non-ASCII object key: {value!r}")
        return
    if isinstance(value, list):
        for item in value:
            _validate_canonical_domain(item)
        return
    if isinstance(value, dict):
        for key_text, item in value.items():
            if not isinstance(key_text, str):
                raise ValueError("object keys must be strings")
            _validate_canonical_domain(key_text, key=True)
            _validate_canonical_domain(item)
        return
    raise ValueError(f"unsupported JSON value: {type(value).__name__}")


def canonical(value) -> bytes:
    """Canonical JSON bytes (sorted keys, no whitespace, raw UTF-8 text)."""
    _validate_canonical_domain(value)
    return json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode("utf-8")


def sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def utf16_length(text: str) -> int:
    """Length of ``text`` in UTF-16 code units (pinned fact K receipt size)."""
    return len(text.encode("utf-16-le")) // 2


def _write_bytes(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)


def _write_pretty(path: Path, value) -> None:
    """Pretty JSON document (meta.json style; never digest-checked)."""
    _write_bytes(path, (json.dumps(value, indent=2, ensure_ascii=False) + "\n").encode("utf-8"))


# --------------------------------------------------------------------------
# Journal records (pinned fact E: exact upstream line shape and digest rule).
# --------------------------------------------------------------------------

_RECORD_SHA_MARKER = b',"record_sha256":"'


def journal_record_line(payload, previous_digest, sequence: int, record_type: str):
    """Build one canonical journal record line exactly as upstream writes it.

    The record is ``b'{"payload":' + canonical(payload) + b"," +
    canonical(header)[1:]`` and the digest is computed with the rule-E
    slicing over the finished line (last ``,"record_sha256":"`` marker,
    64 digest bytes, hash of the line with those bytes spliced out).
    """
    header = {
        "previous_record_sha256": previous_digest,
        "record_sha256": "0" * 64,  # placeholder with the real digest length
        "recovery_journal_record_version": 1,
        "sequence": sequence,
        "type": record_type,
    }
    line = b'{"payload":' + canonical(payload) + b"," + canonical(header)[1:]
    start = line.rfind(_RECORD_SHA_MARKER) + len(_RECORD_SHA_MARKER)
    if line[start + 64 : start + 65] != b'"':
        raise ValueError("record_sha256 placeholder is not a 64-char string")
    digest = sha256_hex(line[:start] + line[start + 65 :])
    line = line[:start] + digest.encode("ascii") + line[start + 64 :]
    return line, digest


# --------------------------------------------------------------------------
# Store writer (facts A, B, C, E, K).
# --------------------------------------------------------------------------


def write_store(
    root: Path,
    session_id: str,
    generation: int,
    snapshot_sequence: int,
    runtime_state: dict,
    snapshot: dict,
    journal: list,
    chunk_split,
    meta,
    saved_results: dict,
    journal_tail: bytes = b"",
):
    """Write one Unified session store under ``root/<session_id>/``.

    Returns the journal segment info (``first_sequence``, ``records``,
    ``last_digest``) so follow-up records can chain onto the segment.
    """
    store = root / session_id
    generation_text = f"{generation:016d}"
    gen_dir = store / "generations" / generation_text

    # Pooled history chunks: split the entries into runs of the given
    # lengths, store each run as its own chunk, empty the document history.
    chunk_digests = None
    if chunk_split is not None:
        entries = list(snapshot["history"]["entries"])
        if sum(chunk_split) != len(entries):
            raise ValueError("chunk_split must cover every history entry exactly")
        chunk_digests = []
        offset = 0
        for run in chunk_split:
            body = canonical(entries[offset : offset + run])
            offset += run
            digest = sha256_hex(body)
            chunk_digests.append(digest)
            _write_bytes(store / "chunks" / f"{digest}.json", body + b"\n")
        snapshot = {**snapshot, "history": {"entries": []}}

    # Generation documents. The projection watermark starts at the snapshot
    # sequence and every projection record carries its own sequence number.
    projection_doc = {
        "projection_state_version": 1,
        "session_id": session_id,
        "snapshot_sequence": snapshot_sequence,
        "watermark": snapshot_sequence,
        "snapshot": snapshot,
    }
    runtime_doc = dict(runtime_state)
    if runtime_doc.get("session_id") != session_id:
        raise ValueError("runtime_state session_id mismatch")
    if runtime_doc.get("runtime_state_version") != 3:
        raise ValueError("runtime_state_version must be 3")
    runtime_doc.setdefault("snapshot_sequence", snapshot_sequence)
    checkpoint_doc = {"checkpoint_version": 1, "context": {"messages": []}}

    projection_body = canonical(projection_doc)
    runtime_body = canonical(runtime_doc)
    checkpoint_body = canonical(checkpoint_doc)
    _write_bytes(gen_dir / "projection-state.json", projection_body + b"\n")
    _write_bytes(gen_dir / "runtime-state.json", runtime_body + b"\n")
    _write_bytes(gen_dir / "checkpoint.json", checkpoint_body + b"\n")

    # Journal segment: one \n-terminated record per list item, then the
    # unterminated tail fragment (an upstream crash mid-write looks like this).
    first_sequence = snapshot_sequence + 1
    journal_path = f"journal/{first_sequence:016d}.jsonl"
    journal_data = bytearray()
    previous_digest = None
    last_digest = None
    for index, record in enumerate(journal):
        line, last_digest = journal_record_line(
            record["payload"], previous_digest, first_sequence + index, record["type"]
        )
        journal_data += line + b"\n"
        previous_digest = last_digest
    journal_data += journal_tail
    _write_bytes(store / journal_path, bytes(journal_data))

    # Manifest (canonical; digest goes into the CURRENT pointer).
    manifest = {
        "checkpoint": {
            "checkpoint_version": 1,
            "chunks": None,
            "path": "checkpoint.json",
            "sha256": sha256_hex(checkpoint_body),
        },
        "created_at": iso_ms(snapshot["session"]["createdAt"]),
        "execution_state": "quiescent",
        "generation": generation_text,
        "interop_export": None,
        "manifest_version": 1,
        "projection_state": {
            "chunks": chunk_digests,
            "path": "projection-state.json",
            "sha256": sha256_hex(projection_body),
        },
        "recovery_journal_segment": {
            "first_sequence": first_sequence,
            "path": journal_path,
        },
        "runtime_state": {
            "chunks": None,
            "path": "runtime-state.json",
            "sha256": sha256_hex(runtime_body),
        },
        "session_id": session_id,
        "snapshot_sequence": snapshot_sequence,
    }
    manifest_body = canonical(manifest)
    _write_bytes(gen_dir / "manifest.json", manifest_body + b"\n")

    # CURRENT pointer (canonical object plus one final \n).
    pointer = {
        "generation": generation_text,
        "manifest_sha256": sha256_hex(manifest_body),
        "session_id": session_id,
        "snapshot_sequence": snapshot_sequence,
        "store_format": STORE_FORMAT,
        "store_format_minor": STORE_FORMAT_MINOR,
    }
    _write_bytes(store / "CURRENT", canonical(pointer) + b"\n")

    # Optional meta.json (pretty, like upstream) and saved tool results
    # (exact text, no trailing newline).
    if meta is not None:
        _write_pretty(store / "meta.json", meta)
    for name, text in saved_results.items():
        _write_bytes(store / "tool-results" / name, text.encode("utf-8"))

    return {
        "first_sequence": first_sequence,
        "records": len(journal),
        "last_digest": last_digest,
        "store": store,
    }


# --------------------------------------------------------------------------
# Entry builders (pinned fact H shapes).
# --------------------------------------------------------------------------

_OMIT = object()


def text_block(text: str) -> dict:
    return {"type": "text", "text": text}


def message(
    entry_id,
    role,
    turn,
    created,
    blocks,
    *,
    source=_OMIT,
    outcome=_OMIT,
    generation_status="completed",
    user_display=_OMIT,
):
    entry = {
        "type": "message",
        "id": entry_id,
        "turnId": turn,
        "createdAt": created,
        "updatedAt": created,
        "generationStatus": generation_status,
        "role": role,
        "content": blocks,
    }
    if source is not _OMIT:
        entry["source"] = source
    if outcome is not _OMIT:
        entry["outcome"] = outcome
    if user_display is not _OMIT:
        entry["userDisplayContent"] = user_display
    return entry


def reasoning(
    entry_id,
    turn,
    created,
    text,
    *,
    summary=None,
    outcome=None,
    generation_status="completed",
):
    return {
        "type": "reasoning",
        "id": entry_id,
        "turnId": turn,
        "createdAt": created,
        "updatedAt": created,
        "generationStatus": generation_status,
        "text": text,
        "summary": summary if summary is not None else [],
        "outcome": outcome,
    }


def checkpoint(entry_id, kind, created, details, *, turn=None, message_text=_OMIT):
    entry = {
        "type": "checkpoint",
        "id": entry_id,
        "turnId": turn,
        "createdAt": created,
        "updatedAt": created,
        "generationStatus": "completed",
        "kind": kind,
        "details": details,
    }
    if message_text is not _OMIT:
        entry["message"] = message_text
    return entry


def notice(entry_id, turn, created, message_text, detail):
    return {
        "type": "notice",
        "id": entry_id,
        "turnId": turn,
        "createdAt": created,
        "updatedAt": created,
        "generationStatus": "completed",
        "message": message_text,
        "detail": detail,
    }


def effect(action_id, tool, status, turn, created, **state):
    """Build a fact-H effect entry; ``**state`` feeds the per-status fields.

    Recognized kwargs: ``input`` (detail input), ``agent_name`` and
    ``child_session_id`` (subagent detail), ``title``, plus the per-status
    state fields (``outputText``, ``output``, ``display``, ``error``,
    ``reason``).
    """
    state = dict(state)
    input_value = state.pop("input")
    title = state.pop("title", tool)
    detail = {
        "kind": "subagent" if "agent_name" in state else "tool",
        "toolName": tool,
        "input": input_value,
        "display": {
            "message": title,
            "settledMessage": title,
            "settledVerb": "Ran",
            "statusText": f"Running {tool}",
            "summary": tool,
            "verb": "Running",
        },
    }
    if "agent_name" in state:
        detail["agentName"] = state.pop("agent_name")
        detail["childSessionId"] = state.pop("child_session_id")
    return {
        "type": "effect",
        "id": "effect-" + action_id,
        "turnId": turn,
        "createdAt": created,
        "updatedAt": created,
        "generationStatus": "completed",
        "title": title,
        "detail": detail,
        "state": {"status": status, **state},
    }


def receipt(path: str, n: int, variant: str) -> str:
    """Receipt text per pinned fact K; ``n`` is the UTF-16 length of the
    saved file content. 3.0.0 stops after the max-size line; 3.0.1/3.0.2
    continue with "The excerpt below" and carry the truncated-output body."""
    prefix = (
        "<system>The tool output is large. To preserve your context window, "
        "it was saved to the file system under " + path + ".\n"
        "Output size: " + str(n) + " characters\n"
        "Max size before saving: 16000 characters"
    )
    if variant == "3.0.0":
        return (
            prefix + ".</system>\n"
            "<truncated-output>\nThe full output is available in the saved file.\n</truncated-output>"
        )
    if variant in ("3.0.1", "3.0.2"):
        return (
            prefix + "\n"
            "The excerpt below keeps about 8000 characters of the tool response: "
            "the start and end of the whole response, with the middle omitted.</system>\n"
            "<truncated-output>\nThe full output is available in the saved file.\n</truncated-output>"
        )
    raise ValueError(f"unknown receipt variant: {variant}")


# --------------------------------------------------------------------------
# Runtime state and snapshot envelope helpers (facts D, G).
# --------------------------------------------------------------------------


def runtime(
    kind,
    session_id,
    root_id,
    parent_id,
    depth,
    cwd,
    active_model,
    *,
    storage_lifetime="persistent",
    spawn_key=None,
    lifecycle=None,
    import_provenance=None,
):
    return {
        "runtime_state_version": 3,
        "session_id": session_id,
        "storage_lifetime": storage_lifetime,
        "identity": {
            "depth": depth,
            "kind": kind,
            "parent_session_id": parent_id,
            "root_session_id": root_id,
            "session_id": session_id,
        },
        "session_metadata": {
            "active_model": active_model,
            "agent_name": None,
            "cwd": cwd,
            "origin_cwd": None,
            "parent_session_id": parent_id,
            "root_session_id": root_id,
            "subagent_spawn_key": spawn_key,
        },
        "lifecycle": lifecycle if lifecycle is not None else {"type": "active"},
        "import_provenance": import_provenance,
    }


def envelope(session_id, root_id, parent_id, title, created, updated, status,
             token_usage, context_usage, latest_turn):
    return {
        "format": PROJECTION_FORMAT,
        "session": {
            "id": session_id,
            "rootSessionId": root_id,
            "parentSessionId": parent_id,
            "title": title,
            "status": status,
            "createdAt": created,
            "updatedAt": updated,
            "tokenUsage": token_usage,
            "contextUsage": context_usage,
        },
        "latestTurn": latest_turn,
    }


def with_history(env, entries):
    return {**env, "history": {"entries": entries}}


def turn(status, turn_id, started, completed=None):
    return {"completedAt": completed, "id": turn_id, "startedAt": started, "status": status}


def token_usage(input_tokens, output_tokens, total_tokens, cached_input_tokens):
    return {
        "inputTokens": input_tokens,
        "outputTokens": output_tokens,
        "totalTokens": total_tokens,
        "cachedInputTokens": cached_input_tokens,
    }


# --------------------------------------------------------------------------
# Valid fixtures under tests/fixtures/vibe_sessions/unified/.
# --------------------------------------------------------------------------


def _bash_saved_text(sentinel: str) -> str:
    """Pretty JSON saved-result body (~2 KB stdout) without final newline."""
    payload = {
        "returncode": 0,
        "stdout": sentinel
        + "\n"
        + "\n".join(
            f"fixture output line {i:03d} of the saved bash payload" for i in range(1, 41)
        ),
        "stderr": "",
    }
    return json.dumps(payload, indent=2, ensure_ascii=False)


def build_r302(unified_root: Path) -> None:
    title = "Résumé du projet et correctif de test"
    initial_tokens = token_usage(500, 120, 620, 80)
    initial_context = token_usage(500, 120, 620, 0)

    e1 = message(
        "e1",
        "user",
        "t1",
        ts(1),
        [
            text_block("/vibe-skill Résume le projet 🚀"),
            text_block('<skill_content name="vibe-skill">\nSkill body\n</skill_content>'),
        ],
        source="turn_start",
        user_display={
            "version": "1",
            "host": "vibe",
            "content": [{"type": "vibe.skill_invocation", "name": "vibe-skill"}],
        },
    )
    e2 = reasoning("e2", "t1", ts(2), "Je regarde le dépôt", outcome={"type": "committed"})
    e3 = effect(
        "act-read",
        "file_system.read_file",
        "completed",
        "t1",
        ts(3),
        title="Read Cargo.toml",
        input={"path": "Cargo.toml"},
        output="[package]",
        outputText="[package]",
        display={"message": "file_system.read_file", "success": True},
    )
    e4 = message(
        "e4",
        "assistant",
        "t1",
        ts(4),
        [text_block("Voici le résumé.")],
        source="harness",
        outcome={"type": "committed"},
    )
    e5 = checkpoint(
        "e5",
        "model_change",
        ts(5),
        {"model": "mistral-large-4"},
        message_text="Model changed to mistral-large-4",
    )
    e6 = message(
        "e6",
        "user",
        "t2",
        ts(6),
        [
            text_block("Corrige le test"),
            {
                "type": "image",
                "attachment": {"mimeType": "image/png", "path": "/home/fixture/screenshot.png"},
            },
        ],
        source="turn_start",
    )
    e7 = reasoning(
        "e7",
        "t2",
        ts(7),
        "[redacted]",
        summary=["Résumé visible"],
        outcome={"type": "committed"},
    )
    e8 = effect(
        "act-patch",
        "file_system.search_replace",
        "failed",
        "t2",
        ts(8),
        title="Patch src/lib.rs",
        input={"path": "src/lib.rs", "search": "fn old()", "replace": "fn new()"},
        error={"message": "patch did not apply"},
        outputText="",
        display={"message": "patch did not apply", "success": False},
    )
    bash_text = _bash_saved_text("FULLOUTPUT-SENTINEL")
    bash_receipt = receipt(
        f"{VIBE_ROOT}/unified/{R302}/tool-results/bash-call%3A9.json",
        utf16_length(bash_text),
        "3.0.2",
    )
    e9 = effect(
        "act-bash",
        "file_system.bash",
        "completed",
        "t2",
        ts(9),
        title="Run cargo test",
        input={"command": "cargo test --workspace"},
        output={"content": [{"type": "text", "text": bash_receipt}], "type": "success"},
        outputText=bash_receipt,
        display={"message": "cargo test --workspace", "success": True},
    )
    e10 = effect(
        "act-spawn-explicit",
        "agents.spawn",
        "completed",
        "t2",
        ts(10),
        title="Spawn scout",
        agent_name="scout",
        child_session_id=CHILD_A,
        input={"task": "Explore the parser", "agent": "explore"},
        outputText="Found 3 parsers",
    )
    e11 = effect(
        "act-spawn-derived",
        "agents.spawn",
        "completed",
        "t2",
        ts(11),
        title="Spawn auditor",
        agent_name="auditor",
        child_session_id=None,
        input={"task": "Audit", "agent": "generic"},
        outputText="Audit done",
    )
    e12 = effect(
        "act-spawn-missing",
        "agents.spawn",
        "failed",
        "t2",
        ts(12),
        title="Spawn ghost",
        agent_name="ghost",
        child_session_id=None,
        input={"task": "Ghost", "agent": "generic"},
        error={"message": "spawn failed"},
        outputText="",
    )
    e13 = notice("e13", "t2", ts(13), "Hooks completed", {"point": "post_tool_call", "accepted": 2})
    e14 = checkpoint(
        "e14",
        "agent_change",
        ts(14),
        {"agent": "plan"},
        turn="t2",
        message_text="Agent changed to plan",
    )
    e15 = checkpoint("e15", "compaction", ts(15), {"summaryLength": 120}, turn="t2")
    e16 = message(
        "e16",
        "assistant",
        "t2",
        ts(16),
        [text_block("WRONG ANSWER")],
        source="harness",
        outcome={"type": "discarded", "cause": "retry"},
    )
    e17 = message(
        "e17",
        "assistant",
        "t2",
        ts(17),
        [text_block("Test corrigé.")],
        source="harness",
        outcome={"type": "committed"},
    )
    snapshot_entries = [e1, e2, e3, e4, e5, e6, e7, e8, e9, e10, e11, e12, e13, e14, e15, e16, e17]

    # Record 5: open turn t3.
    e18 = message("e18", "user", "t3", ts(18), [text_block("Ajoute la doc")], source="turn_start")
    env_running = envelope(
        R302,
        R302,
        None,
        title,
        T0,
        ts(18),
        {"type": "running", "activeTurnId": "t3"},
        initial_tokens,
        initial_context,
        turn("in_progress", "t3", ts(18)),
    )
    record5 = {
        "type": "projection_delta",
        "payload": {
            "watermark": 5,
            "delta": [
                {"op": "append_entry", "entry": e18},
                {"op": "set_envelope", "state": with_history(env_running, [])},
            ],
        },
    }

    # Record 6: a reserved client command (not a projection record).
    record6 = {
        "type": "command_reserved",
        "payload": {"client_command_id": "cmd-1", "method": "turn/start", "params": {}},
    }

    # Record 7: steering message, running effect, provisional thinking/answer.
    e18b = message(
        "e18b", "user", "t3", ts(19), [text_block("Et le README")], source="turn_steer"
    )
    e19 = effect(
        "act-gh",
        "connector_github_app.search_code",
        "running",
        "t3",
        ts(20),
        title="Search GitHub code",
        input={"query": "docs"},
        outputText="",
    )
    e20 = reasoning(
        "reasoning-act-c-0",
        "t3",
        ts(21),
        "partial thinking",
        outcome=None,
        generation_status="in_progress",
    )
    e21 = message(
        "assistant-act-c",
        "assistant",
        "t3",
        ts(22),
        [text_block("partial")],
        source="harness",
        outcome=None,
        generation_status="in_progress",
    )
    record7 = {
        "type": "projection_delta",
        "payload": {
            "watermark": 7,
            "delta": [
                {"op": "append_entry", "entry": e18b},
                {"op": "append_entry", "entry": e19},
                {"op": "append_entry", "entry": e20},
                {"op": "append_entry", "entry": e21},
            ],
        },
    }

    # Record 8: settle the effect, drop the provisional reasoning, settle the
    # answer, then a cancelled write and a skipped todo.
    e19_done = effect(
        "act-gh",
        "connector_github_app.search_code",
        "completed",
        "t3",
        ts(20),
        title="Search GitHub code",
        input={"query": "docs"},
        output="2 results",
        outputText="2 results",
        display={"message": "connector_github_app.search_code", "success": True},
    )
    e21_done = message(
        "assistant-act-c",
        "assistant",
        "t3",
        ts(22),
        [text_block("Doc ajoutée.")],
        source="harness",
        outcome={"type": "committed"},
    )
    e20b = reasoning("e20b", "t3", ts(23), "Plan: write docs", outcome={"type": "committed"})
    e22 = effect(
        "act-write",
        "file_system.write_file",
        "cancelled",
        "t3",
        ts(24),
        title="Write docs/NEW.md",
        input={"path": "docs/NEW.md"},
        reason="Interrupted",
        outputText="",
    )
    e23 = effect(
        "act-todo",
        "vibe.todo",
        "skipped",
        "t3",
        ts(25),
        title="Update todo list",
        input={"action": "add", "text": "Document"},
        reason="Hook denied",
    )
    record8 = {
        "type": "projection_delta",
        "payload": {
            "watermark": 8,
            "delta": [
                {"op": "replace_entry", "id": "effect-act-gh", "entry": e19_done},
                {"op": "remove_entry", "id": "reasoning-act-c-0"},
                {"op": "replace_entry", "id": "assistant-act-c", "entry": e21_done},
                {"op": "append_entry", "entry": e20b},
                {"op": "append_entry", "entry": e22},
                {"op": "append_entry", "entry": e23},
            ],
        },
    }

    # Record 9: the session moved (cwd change) and the turn completed.
    env_final = envelope(
        R302,
        R302,
        None,
        title,
        T0,
        T0 + 60000,
        {"type": "idle"},
        token_usage(1200, 300, 1500, 800),
        token_usage(999999, 1, 1000000, 0),
        turn("completed", "t3", ts(18), T0 + 60000),
    )
    record9 = {
        "type": "session_relocated",
        "payload": {
            "watermark": 9,
            "metadata": {
                "active_model": "mistral-large-4",
                "agent_name": None,
                "cwd": CWD_MOVED,
                "origin_cwd": CWD_ORIGINAL,
                "parent_session_id": None,
                "root_session_id": R302,
                "subagent_spawn_key": None,
            },
            "plugin_lock": {"lock_version": 1, "entries": []},
            "delta": [{"op": "set_envelope", "state": with_history(env_final, [])}],
        },
    }

    # Record 10: action result bookkeeping (not a projection record).
    record10 = {
        "type": "action_result",
        "payload": {"action_id": "act-c", "state": "succeeded", "result": {}},
    }

    snapshot = with_history(
        envelope(
            R302,
            R302,
            None,
            title,
            T0,
            ts(17),
            {"type": "idle"},
            initial_tokens,
            initial_context,
            turn("completed", "t2", ts(6), ts(17)),
        ),
        snapshot_entries,
    )
    write_store(
        unified_root,
        R302,
        generation=4,
        snapshot_sequence=4,
        runtime_state=runtime("root", R302, R302, None, 0, CWD_ORIGINAL, "mistral-large-4"),
        snapshot=snapshot,
        journal=[record5, record6, record7, record8, record9, record10],
        chunk_split=[9, 8],
        meta={
            "session_id": R302,
            "start_time": iso_ms(T0),
            "end_time": iso_ms(T0 + 60000),
            "environment": {"working_directory": CWD_ORIGINAL},
            "config": {"active_model": "mistral-medium-3"},
        },
        saved_results={"bash-call%3A9.json": bash_text},
        journal_tail=b'{"payload":{"delta":[{"entry":{"type":"message"',
    )


def build_child_a(unified_root: Path) -> None:
    saved_text = json.dumps(
        {"returncode": 0, "stdout": "SANDBOX-SENTINEL sandbox output payload", "stderr": ""},
        indent=2,
        ensure_ascii=False,
    )
    sandbox_receipt = receipt(
        "/tmp/vibe-sandbox-1a2b/tool-results/bash-call-7.json",
        utf16_length(saved_text),
        "3.0.2",
    )
    entries = [
        message("u1", "user", "t1", ts(1), [text_block("Explore the parser")], source="turn_start"),
        effect(
            "act-sbx",
            "file_system.bash",
            "completed",
            "t1",
            ts(2),
            title="Run sandbox command",
            input={"command": "find . -name '*.rs' | head"},
            output={"content": [{"type": "text", "text": sandbox_receipt}], "type": "success"},
            outputText=sandbox_receipt,
            display={"message": "sandbox bash", "success": True},
        ),
        message(
            "a1",
            "assistant",
            "t1",
            ts(3),
            [text_block("Found 3 parsers")],
            source="harness",
            outcome={"type": "committed"},
        ),
    ]
    snapshot = with_history(
        envelope(
            CHILD_A,
            R302,
            R302,
            "Explore the parser",
            T0,
            ts(3),
            {"type": "idle"},
            token_usage(50, 10, 60, 0),
            token_usage(50, 10, 60, 0),
            turn("completed", "t1", ts(1), ts(3)),
        ),
        entries,
    )
    write_store(
        unified_root,
        CHILD_A,
        generation=1,
        snapshot_sequence=1,
        runtime_state=runtime(
            "subagent", CHILD_A, R302, R302, 1, CWD_ORIGINAL, "mistral-large-4",
            spawn_key="act-spawn-explicit",
        ),
        snapshot=snapshot,
        journal=[],
        chunk_split=None,
        meta=None,
        saved_results={"bash-call-7.json": saved_text},
    )


def build_child_b(unified_root: Path) -> None:
    entries = [
        message("u1", "user", "t1", ts(1), [text_block("Audit")], source="turn_start"),
        message(
            "a1",
            "assistant",
            "t1",
            ts(2),
            [text_block("Audit done")],
            source="harness",
            outcome={"type": "committed"},
        ),
    ]
    snapshot = with_history(
        envelope(
            CHILD_B,
            R302,
            R302,
            "Audit",
            T0,
            ts(2),
            {"type": "idle"},
            None,
            token_usage(0, 0, 0, 0),
            turn("completed", "t1", ts(1), ts(2)),
        ),
        entries,
    )
    write_store(
        unified_root,
        CHILD_B,
        generation=1,
        snapshot_sequence=1,
        runtime_state=runtime(
            "subagent", CHILD_B, R302, R302, 1, CWD_ORIGINAL, "mistral-large-4",
            spawn_key="act-spawn-derived",
        ),
        snapshot=snapshot,
        journal=[],
        chunk_split=None,
        meta=None,
        saved_results={},
    )


def build_child_c(unified_root: Path) -> None:
    entries = [
        message("u1", "user", "t1", ts(1), [text_block("Fork child task")], source="turn_start"),
        message(
            "a1",
            "assistant",
            "t1",
            ts(2),
            [text_block("Fork child done")],
            source="harness",
            outcome={"type": "committed"},
        ),
    ]
    snapshot = with_history(
        envelope(
            CHILD_C,
            F301,
            F301,
            "Fork child task",
            T0,
            ts(2),
            {"type": "idle"},
            token_usage(20, 5, 25, 0),
            token_usage(20, 5, 25, 0),
            turn("completed", "t1", ts(1), ts(2)),
        ),
        entries,
    )
    write_store(
        unified_root,
        CHILD_C,
        generation=1,
        snapshot_sequence=1,
        runtime_state=runtime(
            "subagent", CHILD_C, F301, F301, 1, CWD_ORIGINAL, "mistral-large-4",
            spawn_key="act-spawn-fork",
        ),
        snapshot=snapshot,
        journal=[],
        chunk_split=None,
        meta=None,
        saved_results={},
    )


def build_r300(unified_root: Path) -> None:
    tokens = token_usage(200, 60, 260, 0)
    u0 = message("u0", "user", "t0", ts(1), [text_block("Ancienne amorce")], source="turn_start")
    initial_snapshot = with_history(
        envelope(
            R300,
            R300,
            None,
            "Ancienne amorce",
            T0,
            ts(1),
            {"type": "idle"},
            tokens,
            tokens,
            turn("completed", "t0", ts(1), ts(1)),
        ),
        [u0],
    )

    # Record 2: the whole snapshot is replaced by an advanced generation.
    u1 = message(
        "u1", "user", "t1", ts(2), [text_block("Première question")], source="turn_start"
    )
    a1 = message(
        "a1",
        "assistant",
        "t1",
        ts(3),
        [text_block("Réponse")],
        source="harness",
        outcome={"type": "committed"},
    )
    advanced_tokens = token_usage(300, 80, 380, 0)
    advanced_env = envelope(
        R300,
        R300,
        None,
        "Ancienne amorce",
        T0,
        ts(3),
        {"type": "idle"},
        advanced_tokens,
        advanced_tokens,
        turn("failed", "t1", ts(2), ts(3)),
    )
    record2 = {
        "type": "projection_advanced",
        "payload": {"watermark": 2, "snapshot": with_history(advanced_env, [u1, a1])},
    }

    # Record 3: history replaced with a duplicated entry id.
    record3 = {
        "type": "projection_delta",
        "payload": {
            "watermark": 3,
            "delta": [
                {"op": "set_history_entries", "entries": [u1, a1, u1]},
            ],
        },
    }

    # Record 4: only the FIRST entry with id u1 is replaced.
    u1_edited = message(
        "u1", "user", "t1", ts(2), [text_block("Première question (éditée)")], source="turn_start"
    )
    record4 = {
        "type": "projection_delta",
        "payload": {"watermark": 4, "delta": [{"op": "replace_entry", "id": "u1", "entry": u1_edited}]},
    }

    # Record 5: a large result saved with a 3.0.0 receipt.
    saved_text = json.dumps(
        {"returncode": 0, "stdout": "OLDRECEIPT-SENTINEL legacy output payload", "stderr": ""},
        indent=2,
        ensure_ascii=False,
    )
    old_receipt = receipt(
        f"{VIBE_ROOT}/unified/{R300}/tool-results/bash-call-300.json",
        utf16_length(saved_text),
        "3.0.0",
    )
    big_effect = effect(
        "act-big",
        "file_system.bash",
        "completed",
        "t1",
        ts(4),
        title="Read big.txt",
        input={"command": "cat big.txt"},
        output={"content": [{"type": "text", "text": old_receipt}], "type": "success"},
        outputText=old_receipt,
        display={"message": "cat big.txt", "success": True},
    )
    record5 = {
        "type": "projection_delta",
        "payload": {"watermark": 5, "delta": [{"op": "append_entry", "entry": big_effect}]},
    }

    write_store(
        unified_root,
        R300,
        generation=1,
        snapshot_sequence=1,
        runtime_state=runtime("root", R300, R300, None, 0, CWD_ORIGINAL, "mistral-medium-3"),
        snapshot=initial_snapshot,
        journal=[record2, record3, record4, record5],
        chunk_split=None,
        meta={
            "session_id": R300,
            "start_time": iso_ms(T0),
            "end_time": iso_ms(ts(4)),
            "environment": {"working_directory": CWD_ORIGINAL},
            "config": {"active_model": "mistral-medium-3"},
        },
        saved_results={"bash-call-300.json": saved_text},
    )


def build_f301(unified_root: Path) -> None:
    spawn = effect(
        "act-spawn-fork",
        "agents.spawn",
        "completed",
        "t1",
        ts(2),
        title="Spawn fork child",
        agent_name="fork-child",
        child_session_id=CHILD_C,
        input={"task": "Fork child task", "agent": "generic"},
        outputText="Fork child done",
    )
    entries = [
        message("u1", "user", "t1", ts(1), [text_block("Fork question")], source="turn_start"),
        spawn,
        message(
            "a1",
            "assistant",
            "t1",
            ts(3),
            [text_block("Fork answer")],
            source="harness",
            outcome={"type": "committed"},
        ),
    ]
    tokens = token_usage(80, 20, 100, 0)
    snapshot = with_history(
        envelope(
            F301,
            R302,
            R302,
            "Fork question",
            T0,
            ts(3),
            {"type": "idle"},
            tokens,
            tokens,
            turn("interrupted", "t1", ts(1), ts(3)),
        ),
        entries,
    )
    write_store(
        unified_root,
        F301,
        generation=2,
        snapshot_sequence=2,
        runtime_state=runtime("fork", F301, R302, R302, 0, CWD_ORIGINAL, "mistral-large-4"),
        snapshot=snapshot,
        journal=[],
        chunk_split=None,
        meta={
            "session_id": F301,
            "start_time": iso_ms(T0),
            "end_time": iso_ms(ts(3)),
            "environment": {"working_directory": CWD_ORIGINAL},
            "config": {"active_model": "mistral-large-4"},
        },
        saved_results={},
    )


def build_mig(unified_root: Path) -> None:
    entries = [
        message("u1", "user", "t0", ts(1), [text_block("Ancienne question")], source="turn_start"),
        message(
            "a1",
            "assistant",
            "t0",
            ts(2),
            [text_block("Ancienne réponse")],
            source="harness",
            outcome={"type": "committed"},
        ),
        message("u2", "user", "t1", ts(3), [text_block("Nouvelle question")], source="turn_start"),
        message(
            "a2",
            "assistant",
            "t1",
            ts(4),
            [text_block("Nouvelle réponse")],
            source="harness",
            outcome={"type": "committed"},
        ),
    ]
    tokens = token_usage(400, 100, 500, 0)
    snapshot = with_history(
        envelope(
            MIG,
            MIG,
            None,
            "Ancienne question",
            T0,
            ts(4),
            {"type": "idle"},
            tokens,
            tokens,
            turn("completed", "t1", ts(3), ts(4)),
        ),
        entries,
    )
    write_store(
        unified_root,
        MIG,
        generation=1,
        snapshot_sequence=1,
        runtime_state=runtime(
            "root",
            MIG,
            MIG,
            None,
            0,
            CWD_ORIGINAL,
            "mistral-medium-3",
            import_provenance={
                "source": {"backend": "legacy", "session_id": LEG, "store_revision": "rev-1"},
                "history_sha256": "0" * 64,
                "imported_at": "2026-10-20T10:00:00.000Z",
            },
        ),
        snapshot=snapshot,
        journal=[],
        chunk_split=None,
        meta={
            "session_id": MIG,
            "start_time": iso_ms(T0),
            "end_time": iso_ms(ts(4)),
            "environment": {"working_directory": CWD_ORIGINAL},
            "config": {"active_model": "mistral-medium-3"},
        },
        saved_results={},
    )


def write_legacy_pair(fixtures_root: Path) -> None:
    """Legacy session directory that MIG's import_provenance points at."""
    legacy_dir = fixtures_root / "vibe_sessions" / "session_20261020_100000_bbbbbbbb"
    _write_pretty(
        legacy_dir / "meta.json",
        {
            "session_id": LEG,
            "start_time": "2026-10-20T10:00:00Z",
            "environment": {"working_directory": CWD_ORIGINAL},
        },
    )
    lines = [
        {"role": "user", "content": "Ancienne question"},
        {"role": "assistant", "content": "Ancienne réponse"},
    ]
    data = b"".join(
        json.dumps(line, separators=(",", ":"), ensure_ascii=False).encode("utf-8") + b"\n"
        for line in lines
    )
    _write_bytes(legacy_dir / "messages.jsonl", data)


# --------------------------------------------------------------------------
# Variant stores under tests/fixtures/vibe_unified_variants/<name>/unified/.
# --------------------------------------------------------------------------


def _pair(question: str, answer: str):
    return [
        message("u1", "user", "t1", ts(1), [text_block(question)], source="turn_start"),
        message(
            "a1",
            "assistant",
            "t1",
            ts(2),
            [text_block(answer)],
            source="harness",
            outcome={"type": "committed"},
        ),
    ]


def _variant_store(unified_root: Path, session_id: str, entries, *, runtime_state=None,
                    journal=None, storage_lifetime="persistent", lifecycle=None):
    tokens = token_usage(30, 8, 38, 0)
    title = next(
        (
            entry["content"][0]["text"]
            for entry in entries
            if entry["type"] == "message" and entry.get("role") == "user"
        ),
        "Empty session",
    )
    latest_turn = (
        turn("completed", "t1", ts(1), ts(2))
        if any(entry["type"] == "message" for entry in entries)
        else None
    )
    snapshot = with_history(
        envelope(
            session_id,
            session_id,
            None,
            title,
            T0,
            ts(max(len(entries), 1)),
            {"type": "idle"},
            tokens,
            tokens,
            latest_turn,
        ),
        entries,
    )
    write_store(
        unified_root,
        session_id,
        generation=1,
        snapshot_sequence=1,
        runtime_state=runtime_state
        or runtime("root", session_id, session_id, None, 0, CWD_VARIANT, "mistral-large-4",
                   storage_lifetime=storage_lifetime, lifecycle=lifecycle),
        snapshot=snapshot,
        journal=journal or [],
        chunk_split=None,
        meta=None,
        saved_results={},
    )


def _variant_root_with_child(unified_root: Path, root_id: str, child_id: str, *,
                             lifecycle=None):
    spawn = effect(
        "act-spawn-del",
        "agents.spawn",
        "completed",
        "t1",
        ts(3),
        title="Spawn worker",
        agent_name="worker",
        child_session_id=child_id,
        input={"task": "Deleting child task", "agent": "generic"},
        outputText="Deleting child done",
    )
    entries = _pair("Deleting question", "Deleting answer") + [spawn]
    _variant_store(
        unified_root,
        root_id,
        entries,
        runtime_state=runtime("root", root_id, root_id, None, 0, CWD_VARIANT,
                             "mistral-large-4", lifecycle=lifecycle),
    )
    _variant_store(
        unified_root,
        child_id,
        _pair("Deleting child task", "Deleting child done"),
        runtime_state=runtime(
            "subagent", child_id, root_id, root_id, 1, CWD_VARIANT, "mistral-large-4",
            spawn_key="act-spawn-del",
        ),
    )


def build_variants(variants_root: Path) -> int:
    count = 0

    def unified(name: str) -> Path:
        return variants_root / name / "unified"

    # ephemeral: skipped by indexing (storage_lifetime).
    _variant_store(unified("ephemeral"), V_EPHEMERAL, _pair("Ephemeral question", "Ephemeral answer"),
                   storage_lifetime="ephemeral")
    count += 1

    # deleting/active and deleting/deleting: same root + child, the second
    # marks the tree as being deleted.
    _variant_root_with_child(unified("deleting/active"), V_DELETING, V_DELETING_CHILD)
    _variant_root_with_child(
        unified("deleting/deleting"),
        V_DELETING,
        V_DELETING_CHILD,
        lifecycle={
            "type": "deleting_tree",
            "ordered_child_session_ids": [V_DELETING_CHILD],
            "deleted_child_session_ids": [],
        },
    )
    count += 4

    # rollover/gen1: generation 1 with two deltas adding the second exchange.
    u1 = message("u1", "user", "t1", ts(1), [text_block("First question")], source="turn_start")
    a1 = message("a1", "assistant", "t1", ts(2), [text_block("First answer")], source="harness",
                 outcome={"type": "committed"})
    u2 = message("u2", "user", "t2", ts(3), [text_block("Second question")], source="turn_start")
    a2 = message("a2", "assistant", "t2", ts(4), [text_block("Second answer")], source="harness",
                 outcome={"type": "committed"})
    tokens = token_usage(30, 8, 38, 0)
    _variant_store(
        unified("rollover/gen1"),
        V_ROLLOVER,
        [u1, a1],
        journal=[
            {
                "type": "projection_delta",
                "payload": {"watermark": 2, "delta": [{"op": "append_entry", "entry": u2}]},
            },
            {
                "type": "projection_delta",
                "payload": {"watermark": 3, "delta": [{"op": "append_entry", "entry": a2}]},
            },
        ],
    )
    count += 1

    # rollover/gen2: generation 4 with the same final history and no journal.
    gen2_env = envelope(V_ROLLOVER, V_ROLLOVER, None, "First question", T0, ts(4),
                        {"type": "idle"}, tokens, tokens, turn("completed", "t2", ts(3), ts(4)))
    write_store(
        unified("rollover/gen2"),
        V_ROLLOVER,
        generation=4,
        snapshot_sequence=4,
        runtime_state=runtime("root", V_ROLLOVER, V_ROLLOVER, None, 0, CWD_VARIANT,
                              "mistral-large-4"),
        snapshot=with_history(gen2_env, [u1, a1, u2, a2]),
        journal=[],
        chunk_split=None,
        meta=None,
        saved_results={},
    )
    count += 1

    # growing: one journal record, plus the sibling append.jsonl carrying the
    # next valid record (chained onto the segment) for growth tests.
    growing_dir = variants_root / "growing"
    info = write_store(
        unified("growing"),
        V_GROWING,
        generation=1,
        snapshot_sequence=1,
        runtime_state=runtime("root", V_GROWING, V_GROWING, None, 0, CWD_VARIANT,
                              "mistral-large-4"),
        snapshot=with_history(
            envelope(V_GROWING, V_GROWING, None, "Growing question", T0, ts(1),
                     {"type": "idle"}, tokens, tokens, turn("in_progress", "t1", ts(1))),
            [message("u1", "user", "t1", ts(1), [text_block("Growing question")],
                     source="turn_start")],
        ),
        journal=[
            {
                "type": "projection_delta",
                "payload": {
                    "watermark": 2,
                    "delta": [
                        {
                            "op": "append_entry",
                            "entry": message("a1", "assistant", "t1", ts(2),
                                             [text_block("Growing answer")], source="harness",
                                             outcome={"type": "committed"}),
                        }
                    ],
                },
            }
        ],
        chunk_split=None,
        meta=None,
        saved_results={},
    )
    late = message("a2", "assistant", "t1", ts(3), [text_block("Late answer")], source="harness",
                   outcome={"type": "committed"})
    append_line, _ = journal_record_line(
        {"watermark": 3, "delta": [{"op": "append_entry", "entry": late}]},
        info["last_digest"],
        info["first_sequence"] + info["records"],
        "projection_delta",
    )
    _write_bytes(growing_dir / "append.jsonl", append_line + b"\n")
    count += 1

    # unknown-op: a projection delta with an op the reader must reject.
    _variant_store(
        unified("unknown-op"),
        V_UNKNOWN_OP,
        _pair("Unknown op question", "Unknown op answer"),
        journal=[
            {
                "type": "projection_delta",
                "payload": {
                    "watermark": 2,
                    "delta": [{"op": "move_entry", "id": "u1", "index": 0}],
                },
            }
        ],
    )
    count += 1

    # unknown-record: a well-formed record of an unknown kind (valid digest).
    _variant_store(
        unified("unknown-record"),
        V_UNKNOWN_RECORD,
        _pair("Unknown record question", "Unknown record answer"),
        journal=[{"type": "future_record", "payload": {"future": True}}],
    )
    count += 1

    # unknown-entry: an entry type the reader has never seen.
    widget = {
        "type": "widget",
        "id": "w1",
        "turnId": "t1",
        "createdAt": ts(3),
        "updatedAt": ts(3),
        "generationStatus": "completed",
        "widget": "clock",
    }
    _variant_store(
        unified("unknown-entry"),
        V_UNKNOWN_ENTRY,
        [
            message("u1", "user", "t1", ts(1), [text_block("Widget question")],
                    source="turn_start"),
            widget,
            message("a1", "assistant", "t1", ts(2), [text_block("Widget answer")],
                    source="harness", outcome={"type": "committed"}),
        ],
    )
    count += 1

    # unknown-checkpoint: an unknown checkpoint kind.
    _variant_store(
        unified("unknown-checkpoint"),
        V_UNKNOWN_CHECKPOINT,
        _pair("Checkpoint question", "Checkpoint answer")
        + [checkpoint("c1", "teleport", ts(3), {"where": "mars"}, turn="t1")],
    )
    count += 1

    # unknown-status: an unknown effect status.
    _variant_store(
        unified("unknown-status"),
        V_UNKNOWN_STATUS,
        _pair("Status question", "Status answer")
        + [
            effect("act-paused", "file_system.bash", "paused", "t1", ts(3),
                   title="Run paused command", input={"command": "sleep 60"})
        ],
    )
    count += 1

    # deeper: root + CHILD_A-style child + grandchild spawned by the child.
    deeper_root = variants_root / "deeper"
    child_spawn = effect(
        "act-spawn-child",
        "agents.spawn",
        "completed",
        "t1",
        ts(3),
        title="Spawn child",
        agent_name="explorer",
        child_session_id=V_DEEPER_CHILD,
        input={"task": "Child task", "agent": "generic"},
        outputText="Child done",
    )
    _variant_store(
        unified("deeper"),
        V_DEEPER,
        _pair("Deeper question", "Deeper answer") + [child_spawn],
    )
    count += 1
    saved_text = json.dumps(
        {"returncode": 0, "stdout": "DEEPER-CHILD-SENTINEL deeper child output", "stderr": ""},
        indent=2,
        ensure_ascii=False,
    )
    deeper_receipt = receipt(
        "/tmp/vibe-sandbox-9f2e/tool-results/bash-call-9.json",
        utf16_length(saved_text),
        "3.0.2",
    )
    grand_spawn = effect(
        "act-spawn-grand",
        "agents.spawn",
        "completed",
        "t1",
        ts(4),
        title="Spawn grandchild",
        agent_name="grand",
        child_session_id=V_DEEPER_GRANDCHILD,
        input={"task": "Grandchild task", "agent": "generic"},
        outputText="Grandchild done",
    )
    write_store(
        unified("deeper"),
        V_DEEPER_CHILD,
        generation=1,
        snapshot_sequence=1,
        runtime_state=runtime(
            "subagent", V_DEEPER_CHILD, V_DEEPER, V_DEEPER, 1, CWD_VARIANT,
            "mistral-large-4", spawn_key="act-spawn-child",
        ),
        snapshot=with_history(
            envelope(V_DEEPER_CHILD, V_DEEPER, V_DEEPER, "Child task", T0, ts(4),
                     {"type": "idle"}, token_usage(15, 4, 19, 0), token_usage(15, 4, 19, 0),
                     turn("completed", "t1", ts(1), ts(4))),
            [
                message("u1", "user", "t1", ts(1), [text_block("Child task")],
                        source="turn_start"),
                effect(
                    "act-sbx",
                    "file_system.bash",
                    "completed",
                    "t1",
                    ts(2),
                    title="Run sandbox command",
                    input={"command": "pwd"},
                    output={"content": [{"type": "text", "text": deeper_receipt}],
                            "type": "success"},
                    outputText=deeper_receipt,
                    display={"message": "sandbox bash", "success": True},
                ),
                message("a1", "assistant", "t1", ts(3), [text_block("Child done")],
                        source="harness", outcome={"type": "committed"}),
                grand_spawn,
            ],
        ),
        journal=[],
        chunk_split=None,
        meta=None,
        saved_results={"bash-call-9.json": saved_text},
    )
    count += 1
    _variant_store(
        unified("deeper"),
        V_DEEPER_GRANDCHILD,
        _pair("Grandchild task", "Grandchild done"),
        runtime_state=runtime(
            "subagent", V_DEEPER_GRANDCHILD, V_DEEPER_CHILD, V_DEEPER_CHILD, 1, CWD_VARIANT,
            "mistral-large-4", spawn_key="act-spawn-grand",
        ),
    )
    count += 1

    # empty: no user message at all.
    _variant_store(
        unified("empty"),
        V_EMPTY,
        [checkpoint("c1", "model_change", ts(1), {"model": "mistral-large-4"},
                    message_text="Model changed to mistral-large-4")],
    )
    count += 1

    # recovered: history ends with a recovery checkpoint.
    _variant_store(
        unified("recovered"),
        V_RECOVERED,
        _pair("Recovered question", "Recovered answer")
        + [
            checkpoint("c1", "recovered", ts(3), {"discardedEntries": 3}, turn="t1",
                      message_text="Recovered 3 discarded entries")
        ],
    )
    count += 1

    return count


# --------------------------------------------------------------------------
# Independent replay (facts A-C, E-F; no Rust, no shared store code).
# --------------------------------------------------------------------------


class ReplayError(Exception):
    pass


def _read_document(store: Path, relative: str, what: str):
    """Read a document that must end with exactly one final newline."""
    raw = (store / relative).read_bytes()
    if not raw.endswith(b"\n"):
        raise ReplayError(f"{what} does not end with a newline")
    body = raw[:-1]
    if body.endswith(b"\n"):
        raise ReplayError(f"{what} has more than one trailing newline")
    return body


def _parse_canonical(body: bytes, what: str):
    try:
        value = json.loads(body)
    except ValueError as err:
        raise ReplayError(f"{what} is not valid JSON: {err}") from err
    if canonical(value) != body:
        raise ReplayError(f"{what} is not canonical")
    return value


def _check_stored_file(stored, what: str):
    if not isinstance(stored, dict):
        raise ReplayError(f"{what} is not an object")
    path = stored.get("path")
    if (
        not isinstance(path, str)
        or path in ("", ".", "..")
        or "/" in path
        or "\\" in path
    ):
        raise ReplayError(f"{what}.path is not a single file name: {path!r}")
    chunks = stored.get("chunks")
    if chunks is not None and not (
        isinstance(chunks, list) and all(isinstance(c, str) for c in chunks)
    ):
        raise ReplayError(f"{what}.chunks must be null or a digest list")
    return path, chunks


def replay_store(store: Path):
    """Validate one store and replay its public projection.

    Returns ``(session_id, entry_count)``.
    """
    # --- fact A: the CURRENT pointer ---
    pointer = _parse_canonical(_read_document(store, "CURRENT", "CURRENT"), "CURRENT")
    if set(pointer) != {
        "generation",
        "manifest_sha256",
        "session_id",
        "snapshot_sequence",
        "store_format",
        "store_format_minor",
    }:
        raise ReplayError("CURRENT has unexpected keys")
    generation = pointer["generation"]
    if not (isinstance(generation, str) and len(generation) == 16
            and generation.isdigit()):
        raise ReplayError(f"CURRENT.generation is not 16 digits: {generation!r}")
    if pointer["store_format"] != STORE_FORMAT or pointer["store_format_minor"] != STORE_FORMAT_MINOR:
        raise ReplayError("CURRENT store_format is unsupported")
    session_id = pointer["session_id"]
    snapshot_sequence = pointer["snapshot_sequence"]

    # --- fact B: the manifest ---
    manifest_body = _read_document(
        store, f"generations/{generation}/manifest.json", "manifest"
    )
    if sha256_hex(manifest_body) != pointer["manifest_sha256"]:
        raise ReplayError("manifest digest does not match CURRENT.manifest_sha256")
    manifest = _parse_canonical(manifest_body, "manifest")
    if manifest.get("manifest_version") != 1:
        raise ReplayError("manifest_version is not 1")
    for field in ("session_id", "generation", "snapshot_sequence"):
        if manifest.get(field) != pointer[field]:
            raise ReplayError(f"manifest {field} does not match the pointer")
    projection_path, projection_chunks = _check_stored_file(
        manifest["projection_state"], "manifest.projection_state"
    )
    runtime_path, _ = _check_stored_file(
        manifest["runtime_state"], "manifest.runtime_state"
    )
    segment = manifest["recovery_journal_segment"]
    segment_path = segment["path"]
    if (
        not isinstance(segment_path, str)
        or len(segment_path) != len("journal/0000000000000000.jsonl")
        or not segment_path.startswith("journal/")
        or not segment_path.endswith(".jsonl")
        or not segment_path[8:24].isdigit()
    ):
        raise ReplayError(f"recovery journal path is invalid: {segment_path!r}")
    first_sequence = segment["first_sequence"]
    if first_sequence != snapshot_sequence + 1:
        raise ReplayError("recovery journal first_sequence != snapshot_sequence + 1")

    # --- fact C: projection document and pooled chunks ---
    projection_body = _read_document(
        store, f"generations/{generation}/{projection_path}", "projection-state"
    )
    if sha256_hex(projection_body) != manifest["projection_state"]["sha256"]:
        raise ReplayError("projection-state digest mismatch")
    projection = json.loads(projection_body)
    if projection.get("projection_state_version") != 1:
        raise ReplayError("projection_state_version is not 1")
    if projection.get("session_id") != session_id:
        raise ReplayError("projection session_id mismatch")
    if projection.get("snapshot_sequence") != snapshot_sequence:
        raise ReplayError("projection snapshot_sequence mismatch")
    snapshot = projection["snapshot"]
    if snapshot.get("format") != PROJECTION_FORMAT:
        raise ReplayError("snapshot format is unsupported")
    entries = snapshot["history"]["entries"]
    if projection_chunks is not None:
        if entries != []:
            raise ReplayError("chunked snapshot must have empty history.entries")
        entries = []
        for chunk_digest in projection_chunks:
            chunk_body = _read_document(
                store, f"chunks/{chunk_digest}.json", f"chunk {chunk_digest}"
            )
            if sha256_hex(chunk_body) != chunk_digest:
                raise ReplayError(f"chunk {chunk_digest} digest mismatch")
            chunk = json.loads(chunk_body)
            if not isinstance(chunk, list):
                raise ReplayError(f"chunk {chunk_digest} is not a JSON array")
            entries.extend(chunk)
        snapshot["history"]["entries"] = entries

    runtime_body = _read_document(
        store, f"generations/{generation}/{runtime_path}", "runtime-state"
    )
    if sha256_hex(runtime_body) != manifest["runtime_state"]["sha256"]:
        raise ReplayError("runtime-state digest mismatch")
    runtime_state = json.loads(runtime_body)
    if runtime_state.get("runtime_state_version") != 3:
        raise ReplayError("runtime_state_version is not 3")
    if runtime_state.get("session_id") != session_id:
        raise ReplayError("runtime-state session_id mismatch")

    # --- facts E and F: stream the journal and replay the projection ---
    watermark = projection["watermark"]
    journal_data = (store / segment_path).read_bytes()
    expected_sequence = first_sequence
    previous_digest = None
    position = 0
    while True:
        newline = journal_data.find(b"\n", position)
        if newline == -1:
            break  # a final fragment without \n is ignored
        line = journal_data[position:newline]
        position = newline + 1
        try:
            record = json.loads(line)
        except ValueError as err:
            raise ReplayError(f"journal line is not valid JSON: {err}") from err
        if canonical(record) != line:
            raise ReplayError("journal record is not canonical")
        if set(record) != {
            "payload",
            "previous_record_sha256",
            "record_sha256",
            "recovery_journal_record_version",
            "sequence",
            "type",
        }:
            raise ReplayError("journal record has unexpected keys")
        marker_start = line.rfind(_RECORD_SHA_MARKER)
        if marker_start == -1:
            raise ReplayError("journal record has no record_sha256 marker")
        digest_start = marker_start + len(_RECORD_SHA_MARKER)
        digest_end = digest_start + 64
        if line[digest_end : digest_end + 1] != b'"':
            raise ReplayError("record_sha256 digest is not a 64-char string")
        sliced_digest = sha256_hex(line[:digest_start] + line[digest_end + 1 :])
        embedded = line[digest_start:digest_end].decode("ascii")
        if sliced_digest != embedded or embedded != record["record_sha256"]:
            raise ReplayError(f"journal record {record.get('sequence')} digest mismatch")
        if record["recovery_journal_record_version"] != 1:
            raise ReplayError("recovery_journal_record_version is not 1")
        if record["sequence"] != expected_sequence:
            raise ReplayError(
                f"journal sequence {record['sequence']} != expected {expected_sequence}"
            )
        expected_sequence += 1
        if record["previous_record_sha256"] != previous_digest:
            raise ReplayError(f"journal record {record['sequence']} chain mismatch")
        previous_digest = embedded
        if record["type"] not in RECORD_KINDS:
            raise ReplayError(f"unknown journal record type: {record['type']}")

        record_type = record["type"]
        if record_type not in ("projection_advanced", "projection_delta", "session_relocated"):
            continue
        payload = record["payload"]
        record_watermark = payload["watermark"]
        if not isinstance(record_watermark, int) or record_watermark < watermark:
            raise ReplayError(
                f"journal record {record['sequence']} watermark went backwards"
            )
        watermark = record_watermark
        if record_type == "projection_advanced":
            snapshot = payload["snapshot"]
            if snapshot.get("format") != PROJECTION_FORMAT:
                raise ReplayError("advanced snapshot format is unsupported")
            entries = snapshot["history"]["entries"]
        else:
            for operation in payload["delta"]:
                op = operation["op"]
                if op == "append_entry":
                    entries.append(operation["entry"])
                elif op == "replace_entry":
                    for index, existing in enumerate(entries):
                        if existing["id"] == operation["id"]:
                            entries[index] = operation["entry"]
                            break
                    else:
                        raise ReplayError(
                            f"replace_entry id {operation['id']} matched nothing"
                        )
                elif op == "remove_entry":
                    kept = [e for e in entries if e["id"] != operation["id"]]
                    if len(kept) == len(entries):
                        raise ReplayError(
                            f"remove_entry id {operation['id']} matched nothing"
                        )
                    entries = kept
                elif op == "set_history_entries":
                    entries = list(operation["entries"])
                elif op == "set_envelope":
                    state = {
                        key: value
                        for key, value in operation["state"].items()
                        if key != "history"
                    }
                    state["history"] = snapshot["history"]
                    snapshot = state
                else:
                    raise ReplayError(f"unknown projection op: {op}")
                snapshot["history"]["entries"] = entries
            if record_type == "session_relocated":
                # payload.metadata replaces the runtime session_metadata.
                runtime_state["session_metadata"] = payload["metadata"]

    return session_id, len(entries)


def replay_dir(root: Path) -> int:
    """Replay every store under DIR (sorted traversal) and print counts."""
    pointers = sorted(
        current.parent for current in root.rglob("CURRENT") if current.is_file()
    )
    if not pointers:
        print(f"no stores found under {root}", file=sys.stderr)
        return 1
    failures = 0
    for store in pointers:
        try:
            session_id, count = replay_store(store)
        except (ReplayError, OSError, KeyError, ValueError) as err:
            failures += 1
            print(f"{store} error: {err}", file=sys.stderr)
            continue
        print(f"{session_id} entries={count}")
    return 1 if failures else 0


# --------------------------------------------------------------------------
# Entry point.
# --------------------------------------------------------------------------


def generate_all() -> None:
    fixtures_root = Path(__file__).resolve().parent.parent
    unified_root = fixtures_root / "vibe_sessions" / "unified"
    variants_root = fixtures_root / "vibe_unified_variants"

    build_r302(unified_root)
    build_child_a(unified_root)
    build_child_b(unified_root)
    build_child_c(unified_root)
    build_r300(unified_root)
    build_f301(unified_root)
    build_mig(unified_root)
    write_legacy_pair(fixtures_root)
    variant_stores = build_variants(variants_root)

    print("child session ids:")
    print(f"  CHILD_A ({R302} / act-spawn-explicit) = {CHILD_A}")
    print(f"  CHILD_B ({R302} / act-spawn-derived) = {CHILD_B}")
    print(f"  CHILD_C ({F301} / act-spawn-fork)    = {CHILD_C}")
    print(f"wrote 7 unified stores under {unified_root}")
    print(
        "wrote legacy session "
        "tests/fixtures/vibe_sessions/session_20261020_100000_bbbbbbbb"
    )
    print(f"wrote {variant_stores} unified variant stores under {variants_root}")


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Mistral Vibe Unified fixture generator and replay oracle."
    )
    parser.add_argument(
        "--replay",
        metavar="DIR",
        type=Path,
        default=None,
        help="validate and replay every Unified store under DIR instead of writing",
    )
    args = parser.parse_args()
    if args.replay is not None:
        return replay_dir(args.replay)
    generate_all()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
