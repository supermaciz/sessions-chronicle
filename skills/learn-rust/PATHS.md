# Interactive Learning Paths — sessions-chronicle

These paths reflect the current repository structure and current feature set.

Each path is meant to be run interactively:
- inspect code,
- answer a question,
- do one tiny exercise,
- verify,
- continue.

---

## Path A — Startup, crate wiring, and app boot

### Use when
You want to understand how the app starts and how the main flow is assembled.

### Primary targets
- `src/main.rs`
- `src/lib.rs`
- `src/app/`

### What to look for
- binary entrypoint
- app initialization
- error-return style
- module boundaries
- tracing/logging setup
- how the top-level Relm4 flow is entered

### Rust concepts
- `main` and `Result`
- crate/module layout
- visibility
- error propagation with `?`

### Book
- Ch. 7 — Managing Growing Projects
- Ch. 9 — Error Handling

### Rust by Example
- modules
- `Result`
- `?`

### Good first exercise
Add one startup trace and verify it appears.

### Verify
- `cargo check`

---

## Path B — UI state and Relm4 flow

### Use when
You want to understand how state changes drive the UI.

### Primary targets
- `src/ui/`
- `src/app/`

### What to look for
- state/model structs
- messages/events
- update flow
- widget composition
- closure captures
- where selections, filters, or navigation state live

### Rust concepts
- ownership in callbacks
- borrowing vs cloning in UI code
- enums for message dispatch
- struct-based state

### Book
- Ch. 4 — Ownership
- Ch. 6 — Enums and Pattern Matching
- Ch. 15 — Smart Pointers

### Rust by Example
- closures
- enums
- pattern matching

### Good first exercise
Add a tiny UI-facing field or diagnostic label, then thread it through the update flow.

### Verify
- `cargo check`

---

## Path C — Database, indexing, and search

### Use when
You want to understand how sessions are persisted, indexed, and queried.

### Primary targets
- `src/database/`
- `src/indexing_worker.rs`

### What to look for
- schema setup
- FTS-related indexing
- insert/update flow
- search query flow
- row-to-model mapping
- background indexing responsibilities

### Rust concepts
- `Result<T, E>`
- DB boundary design
- iterators over query results
- owned vs borrowed strings at storage boundaries

### Book
- Ch. 8 — Collections
- Ch. 9 — Error Handling
- Ch. 13 — Iterators and Closures

### Rust by Example
- iterators
- `Result`
- hash maps / vectors as supporting concepts

### Good first exercise
Add or inspect one focused query and back it with a narrow test.

### Verify
- `cargo test search_sessions`
- or `cargo test opencode_search`

---

## Path D — Parsing, serde, and domain modeling

### Use when
You want to learn how external assistant session formats become internal Rust models.

### Primary targets
- `src/parsers/`
- `src/models/`
- `tests/load_session.rs`
- `tests/fixtures/`

### What to look for
- assistant-specific parsers
- `serde` derive usage
- normalization into domain structs
- optional fields
- transcript item shapes
- how malformed or partial data is handled

### Rust concepts
- structs and enums
- `Option`
- `Result`
- derive macros
- owned `String` vs borrowed text

### Book
- Ch. 5 — Structs
- Ch. 6 — Enums
- Ch. 10 — Generics, Traits, Lifetimes
- Ch. 18 — Pattern Matching

### Rust by Example
- `Option`
- `Result`
- structs
- enums
- derive

### Good first exercise
Add one optional field to a parser/model pair and cover it with a fixture-based test.

### Verify
- `cargo test load_session`

---

## Path E — Session sources, project resolution, and local environment mapping

### Use when
You want to understand how the app finds sessions and maps them to projects/sources.

### Primary targets
- `src/session_sources.rs`
- `src/project_resolver.rs`
- `tests/project_detection.rs`
- `tests/project_sidebar_filtering.rs`
- `tests/cli_print_db_path.rs`

### What to look for
- local path discovery
- source overrides
- project association
- filtering logic
- small CLI-adjacent helpers

### Rust concepts
- path handling
- error boundaries around IO
- domain-specific enums/structs
- testable pure logic vs side-effectful code

### Book
- Ch. 9 — Error Handling
- Ch. 12 — An I/O Project
- Ch. 13 — Closures and Iterators

### Rust by Example
- paths / filesystem basics
- `Result`
- iterators

### Good first exercise
Add a narrowly scoped project/source resolution test before touching behavior.

### Verify
- `cargo test project_detection`
- `cargo test project_sidebar_filtering`

---

## Path F — Analytics and derived data

### Use when
You want to learn how higher-level product metrics are derived from stored session data.

### Primary targets
- `src/analytics_worker.rs`
- `tests/analytics_integration.rs`
- `tests/analytics_queries.rs`

### What to look for
- query aggregation
- transformation from raw rows to displayable metrics
- date/time handling
- assistant/activity summaries

### Rust concepts
- iterators and aggregation
- transformation pipelines
- chrono/date handling
- separation of raw data from presentation data

### Book
- Ch. 8 — Collections
- Ch. 13 — Iterators
- Ch. 5 — Structs

### Rust by Example
- iterators
- closures
- structs

### Good first exercise
Trace one metric end-to-end: query → aggregation → output shape → test.

### Verify
- `cargo test analytics_queries`
- `cargo test analytics_integration`

---

## Path G — Transcript rendering, previews, tool calls, and reasoning attachments

### Use when
You want to understand how conversations are represented and displayed to the user.

### Primary targets
- `src/ui/`
- `src/models/`
- `tests/message_preview.rs`
- `tests/reasoning_attachments.rs`
- `tests/transcript_items_model.rs`

### What to look for
- transcript item modeling
- preview generation
- markdown/rendering boundaries
- tool call grouping
- reasoning attachment presentation

### Rust concepts
- enums as heterogeneous content models
- borrowing vs cloning while formatting
- string processing
- view-model shaping

### Book
- Ch. 8 — Strings
- Ch. 6 — Enums
- Ch. 18 — Pattern Matching

### Rust by Example
- strings
- enums
- formatting
- pattern matching

### Good first exercise
Pick one transcript item variant and explain how it becomes UI-ready text.

### Verify
- `cargo test message_preview`
- `cargo test reasoning_attachments`
- `cargo test transcript_items_model`

---

## Path H — Errors, tracing, and debugging discipline

### Use when
You are stuck on a bug, borrow-checker failure, unexpected parse issue, or indexing problem.

### Primary targets
- any touched module
- especially parsing, DB, workers, and startup code

### What to look for
- `anyhow`
- `thiserror`
- contextual error messages
- tracing instrumentation
- `unwrap`/`expect` candidates
- boundary cleanup

### Rust concepts
- application errors vs domain errors
- context propagation
- minimal ownership-safe debug probes

### Book
- Ch. 9 — Error Handling

### Rust by Example
- `Result`
- `panic`
- custom error basics

### Good first exercise
Replace one fragile error path with a typed or contextualized one.

### Verify
- `cargo check`
- or the narrowest relevant test

---

## Suggested order

For a first pass:
1. Path A
2. Path B
3. Path D
4. Path C
5. Path E
6. Path G
7. Path F
8. Path H

Why this order:
- it starts with wiring,
- then UI state,
- then parsing/models,
- then persistence/search,
- then environment mapping,
- then transcript presentation,
- then analytics,
- and only then debugging/error refinement.

---

## Session protocol

For each learning session:
1. Pick one path.
2. Inspect one file.
3. Answer one question.
4. Do one exercise.
5. Run one verification command.
6. Record one journal entry.

Do not try to “understand the whole app” in one sitting.