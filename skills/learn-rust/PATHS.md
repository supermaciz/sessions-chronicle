# Interactive Learning Paths

Each path is a guided sequence of short steps.
The agent asks questions, not dump lectures.
Steps within a path build on each other; paths can be taken independently.

## Companion References by Path

Use these as optional support material while following the step-by-step exercises.

| Path | Most Relevant Sources |
|---|---|
| **A** Bootstrap & App Launch | [The Rust Book](https://doc.rust-lang.org/book/title-page.html), [Rust by Example](https://doc.rust-lang.org/rust-by-example/), [Rustlings](https://rustlings.rust-lang.org/), [Rust Standard Library](https://doc.rust-lang.org/std/), [Too Many Lists](https://rust-unofficial.github.io/too-many-lists/) |
| **B** Relm4 UI State & Messages | [The Rust Book](https://doc.rust-lang.org/book/title-page.html), [Rust by Example](https://doc.rust-lang.org/rust-by-example/), [GTK4 Rust Book](https://gtk-rs.org/gtk4-rs/stable/latest/book/), [Rust Standard Library](https://doc.rust-lang.org/std/) |
| **C** Database & Search | [The Rust Book](https://doc.rust-lang.org/book/title-page.html), [Rust by Example](https://doc.rust-lang.org/rust-by-example/), [Rust Standard Library](https://doc.rust-lang.org/std/) |
| **D** Parsing & Serde | [The Rust Book](https://doc.rust-lang.org/book/title-page.html), [Rust by Example](https://doc.rust-lang.org/rust-by-example/), [Rustlings](https://rustlings.rust-lang.org/), [Rust Standard Library](https://doc.rust-lang.org/std/) |
| **E** CLI with Clap | [The Rust Book](https://doc.rust-lang.org/book/title-page.html), [Rust by Example](https://doc.rust-lang.org/rust-by-example/), [Rustlings](https://rustlings.rust-lang.org/), [Rust Standard Library](https://doc.rust-lang.org/std/) |
| **F** Errors & Tracing | [The Rust Book](https://doc.rust-lang.org/book/title-page.html), [Rust by Example](https://doc.rust-lang.org/rust-by-example/), [Rust Standard Library](https://doc.rust-lang.org/std/), [GTK4 Rust Book](https://gtk-rs.org/gtk4-rs/stable/latest/book/), [Too Many Lists](https://rust-unofficial.github.io/too-many-lists/) |

---

## PATH A — Bootstrap & App Launch

**Goal:** Understand how a Rust GTK4/Relm4 app starts up.

### Step 1: The entrypoint
- **File:** [src/main.rs](src/main.rs) — `fn main()`
- **Question:** What does `Args::parse()` return and why is it called before `gtk::init()`?
- **Rust concepts:** `fn main()`, return types, execution order
- **Rust Book:** [Ch 3.3 Functions](https://doc.rust-lang.org/book/ch03-03-how-functions-work.html)
- **Exercise:** Add a `println!` before and after `gtk::init()` — predict and verify the output order.
- **Verify:** `cargo check`

### Step 2: Option threading
- **File:** [src/main.rs](src/main.rs) — `args.sessions_dir` passed to `app.run::<App>(args.sessions_dir)`
- **Question:** `sessions_dir` is `Option<PathBuf>`. What happens when the user doesn't pass `--sessions-dir`?
- **Rust concepts:** `Option<T>`, `None`, generic type parameters
- **Rust Book:** [Ch 6.1 Enums](https://doc.rust-lang.org/book/ch06-01-defining-an-enum.html), [Ch 10.1 Generics](https://doc.rust-lang.org/book/ch10-01-syntax.html)
- **Exercise:** Trace `Option<PathBuf>` from `main.rs` through `App::init` to `SessionSources::resolve()`. List each function signature.
- **Verify:** `cargo check`

### Step 3: The App component
- **File:** [src/app.rs](src/app.rs) — `struct App`, `enum AppMsg`
- **Question:** `App` implements `Component`. What are the associated types it must define?
- **Rust concepts:** Traits, associated types, `impl Trait for Struct`
- **Rust Book:** [Ch 10.2 Traits](https://doc.rust-lang.org/book/ch10-02-traits.html), [Ch 20.2 Advanced Traits](https://doc.rust-lang.org/book/ch20-02-advanced-traits.html)
- **Exercise:** Find all associated types in `App`'s `Component` impl. Write a comment explaining each.
- **Verify:** `cargo check`

### Step 4: Resource loading and shared ownership
- **Files:** [src/main.rs](src/main.rs) — `gio::Resource::load(...)`, [src/app.rs](src/app.rs) — `Arc<PathBuf>`
- **Question:** Why is `db_path` wrapped in `Arc<PathBuf>` instead of just `PathBuf`?
- **Rust concepts:** Ownership, `Arc<T>`, `Clone`, shared references
- **Rust Book:** [Ch 4.1 Ownership](https://doc.rust-lang.org/book/ch04-01-what-is-ownership.html), [Ch 15.4 Rc/Arc](https://doc.rust-lang.org/book/ch15-04-rc.html)
- **Exercise:** Find every `.clone()` on `db_path` in `app.rs`. Explain why each clone is needed.
- **Verify:** `cargo clippy --all -- -D warnings`

---

## PATH B — Relm4 UI State & Messages

**Goal:** Understand the Relm4 Model-View-Update pattern through real widgets.

### Step 1: State structs
- **File:** [src/ui/session_list.rs](src/ui/session_list.rs) — `struct SessionList`
- **Question:** This struct has `sessions: FactoryVecDeque<SessionRow>`. What is a `FactoryVecDeque` and why not a plain `Vec`?
- **Rust concepts:** Structs, generic containers, owned vs. managed collections
- **Rust Book:** [Ch 5.1 Structs](https://doc.rust-lang.org/book/ch05-01-defining-structs.html), [Ch 8.1 Vectors](https://doc.rust-lang.org/book/ch08-01-vectors.html)
- **Exercise:** List every field of `SessionList` and its type. Identify which are primitives, which are collections, which are smart pointers.
- **Verify:** `cargo check`

### Step 2: Message enums
- **File:** [src/ui/session_list.rs](src/ui/session_list.rs) — `enum SessionListMsg`, `enum SessionListOutput`
- **Question:** What's the difference between `SessionListMsg` (input) and `SessionListOutput` (output)? Which direction does each flow?
- **Rust concepts:** Enums with data, enum variants as messages, type-safe communication
- **Rust Book:** [Ch 6.1 Enums](https://doc.rust-lang.org/book/ch06-01-defining-an-enum.html), [Ch 6.2 Match](https://doc.rust-lang.org/book/ch06-02-match.html)
- **Exercise:** Add a new `SessionListMsg::DebugPrint` variant. Handle it in `update()` with a `tracing::debug!` call.
- **Verify:** `cargo check && cargo clippy --all -- -D warnings`

### Step 3: The update loop
- **File:** [src/ui/session_list.rs](src/ui/session_list.rs) — `fn update(&mut self, msg, sender)`
- **Question:** Find the `match msg { ... }` block. How does Rust guarantee every variant is handled?
- **Rust concepts:** Pattern matching, exhaustiveness checking, `match` arms
- **Rust Book:** [Ch 6.2 Match](https://doc.rust-lang.org/book/ch06-02-match.html), [Ch 18.3 Pattern Syntax](https://doc.rust-lang.org/book/ch18-03-pattern-syntax.html)
- **Exercise:** Comment out one `match` arm and observe the compiler error. What does it say?
- **Verify:** `cargo check` (expect error, then restore)

### Step 4: View macros and reactive bindings
- **File:** [src/ui/session_detail.rs](src/ui/session_detail.rs) — `view!` macro, `#[watch]` attribute
- **Question:** What does `#[watch]` do on a widget property? How does it differ from a property set once at init?
- **Rust concepts:** Procedural macros, attribute macros, reactive UI patterns
- **Rust Book:** [Ch 19.5 Macros](https://doc.rust-lang.org/book/ch19-06-macros.html)
- **Exercise:** Find a `#[watch]` usage in `session_detail.rs`. Trace what model field it depends on.
- **Verify:** `cargo check`

### Step 5: Interior mutability
- **File:** [src/ui/session_detail.rs](src/ui/session_detail.rs) — `scroll_to_item: Cell<Option<usize>>`
- **Question:** `post_view` takes `&self` (immutable). How can it modify `scroll_to_item`?
- **Rust concepts:** `Cell<T>`, interior mutability, borrow checker escape hatches
- **Rust Book:** [Ch 15.5 RefCell](https://doc.rust-lang.org/book/ch15-05-interior-mutability.html)
- **Exercise:** Find every `Cell` usage in `session_detail.rs`. Explain why `Cell` is needed instead of `&mut self`.
- **Verify:** `cargo check`

---

## PATH C — Database & Search (SQLite/FTS5)

**Goal:** Understand data persistence, SQL in Rust, and error handling.

### Step 1: Schema and migrations
- **File:** [src/database/schema.rs](src/database/schema.rs) — `fn initialize_database(conn: &Connection)`
- **Question:** The function reads `PRAGMA user_version` and applies migrations. What happens if the DB is at version 2 and code expects version 4?
- **Rust concepts:** `match` on integers, sequential logic, `&Connection` borrows
- **Rust Book:** [Ch 4.2 References](https://doc.rust-lang.org/book/ch04-02-references-and-borrowing.html)
- **Exercise:** Read `apply_v2_migration`. Why does it DROP and recreate the FTS5 table instead of ALTER? Write a comment explaining.
- **Verify:** `cargo test --all --no-fail-fast`

### Step 2: Row-to-domain mapping
- **File:** [src/database/mod.rs](src/database/mod.rs) — `fn session_from_row(row: &Row) -> rusqlite::Result<Session>`
- **Question:** What does `row.get("tool")` return? What trait must `Tool` implement?
- **Rust concepts:** `Result<T, E>`, `?` operator, `FromSql` trait
- **Rust Book:** [Ch 9.2 Result](https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html)
- **Exercise:** Find `Tool::from_storage()`. Trace how a string from SQLite becomes a `Tool` enum variant.
- **Verify:** `cargo test --all --no-fail-fast`

### Step 3: Insert paths (upsert)
- **File:** [src/database/mod.rs](src/database/mod.rs) — `fn insert_tool_call(conn, tc, session_id)`
- **Question:** The SQL uses `INSERT OR REPLACE`. Why not plain `INSERT`? What does `rusqlite::params![]` do?
- **Rust concepts:** Macros, `&dyn ToSql` trait objects, SQL parameter binding
- **Rust Book:** [Ch 17.2 Trait Objects](https://doc.rust-lang.org/book/ch17-02-trait-objects.html)
- **Rust by Example:** [Macros](https://doc.rust-lang.org/rust-by-example/macros.html)
- **Exercise:** Find `Vec<&dyn ToSql>` in the search function. Explain why dynamic dispatch is used here.
- **Verify:** `cargo check`

### Step 4: Search with graceful degradation
- **File:** [src/database/mod.rs](src/database/mod.rs) — `fn search_sessions(db_path, tools, query)`
- **Question:** The search has a fallback: if FTS5 query fails, it sanitizes and retries. If that fails too, it returns `Ok(Vec::new())`. Why never `Err`?
- **Rust concepts:** Nested `match`, error recovery, `Ok` wrapping
- **Rust Book:** [Ch 9 Error Handling](https://doc.rust-lang.org/book/ch09-00-error-handling.html)
- **Exercise:** Add a `tracing::info!` that logs the number of results returned. Verify with `cargo check`.
- **Verify:** `cargo check`

---

## PATH D — Parsing & Serde

**Goal:** Understand streaming data processing, JSON parsing, and error types.

### Step 1: Streaming JSONL
- **File:** [src/parsers/claude_code.rs](src/parsers/claude_code.rs) — `ClaudeCodeParser::parse()`
- **Question:** The parser uses `BufReader::new(file)` and `reader.lines()`. Why not `fs::read_to_string()`?
- **Rust concepts:** `BufRead` trait, iterators, lazy evaluation, memory efficiency
- **Rust Book:** [Ch 13.2 Iterators](https://doc.rust-lang.org/book/ch13-02-iterators.html), [Ch 12.2 Reading a File](https://doc.rust-lang.org/book/ch12-02-reading-a-file.html)
- **Exercise:** Find where malformed lines are handled. What happens on `serde_json::from_str` failure?
- **Verify:** `cargo test --all --no-fail-fast`

### Step 2: Untyped JSON with `serde_json::Value`
- **File:** [src/parsers/claude_code.rs](src/parsers/claude_code.rs) — `serde_json::from_str::<Value>(&line)`
- **Question:** Why parse into `Value` (untyped) instead of a `#[derive(Deserialize)]` struct?
- **Rust concepts:** Dynamic vs. static typing trade-offs, `Value` enum, indexing with `["key"]`
- **Rust Book:** [Ch 6.1 Enums](https://doc.rust-lang.org/book/ch06-01-defining-an-enum.html)
- **Rust by Example:** [Enums](https://doc.rust-lang.org/rust-by-example/custom_types/enum.html)
- **Exercise:** Find 3 places where `Value` is indexed with `["key"]`. What happens if the key doesn't exist?
- **Verify:** `cargo check`

### Step 3: Custom error types with thiserror
- **File:** [src/parsers/codex.rs](src/parsers/codex.rs) — `enum ParseError`
- **Question:** `ParseError` uses `#[derive(thiserror::Error)]`. What does `#[error("...")]` generate?
- **Rust concepts:** Derive macros, `Display` trait, `Error` trait, `downcast_ref`
- **Rust Book:** [Ch 10.2 Traits](https://doc.rust-lang.org/book/ch10-02-traits.html)
- **Exercise:** Add a new variant `InvalidTimestamp(String)` to `ParseError`. What trait impls does `thiserror` generate?
- **Verify:** `cargo check`

### Step 4: Option chaining
- **File:** [src/parsers/model.rs](src/parsers/model.rs) — `fn normalize_model(raw: Option<&Value>)`
- **Question:** The function starts with `let value = raw?;`. What does `?` do on an `Option`?
- **Rust concepts:** `Option` combinators, early return with `?`, `as_str()`, method chaining
- **Rust Book:** [Ch 6.3 If Let](https://doc.rust-lang.org/book/ch06-03-if-let.html), [Ch 9.2 ? Operator](https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html)
- **Exercise:** Rewrite `normalize_model` using `and_then` and `filter` instead of `?`. Compare readability.
- **Verify:** `cargo test --all --no-fail-fast`

### Step 5: Fixture-driven tests
- **Files:** [tests/load_session.rs](tests/load_session.rs), [tests/fixtures/](tests/fixtures/)
- **Question:** Integration tests use real fixture files. Why not mock the file system?
- **Rust concepts:** `#[test]`, `assert_eq!`, `Path` manipulation, test organization
- **Rust Book:** [Ch 11 Testing](https://doc.rust-lang.org/book/ch11-00-testing.html)
- **Exercise:** Write a test for `ClaudeCodeParser` using `tempfile::NamedTempFile`. Parse a 3-line JSONL and verify the session title.
- **Verify:** `cargo test --all --no-fail-fast`

---

## PATH E — CLI with Clap

**Goal:** Understand derive-based CLI parsing and how flags wire into the app.

### Step 1: The Args struct
- **File:** [src/main.rs](src/main.rs) — `struct Args`
- **Question:** `#[derive(Parser)]` generates a CLI parser. What does `#[arg(long, value_name = "DIR")]` do to `sessions_dir`?
- **Rust concepts:** Derive macros, attributes, `Option<PathBuf>`
- **Rust Book:** [Ch 12.1 Accepting Arguments](https://doc.rust-lang.org/book/ch12-01-accepting-command-line-arguments.html)
- **Exercise:** Add a `--verbose` flag (`bool`) to `Args`. Wire it to set `tracing::Level::DEBUG` in main.
- **Verify:** `cargo check`

### Step 2: Trailing variadic args
- **File:** [src/main.rs](src/main.rs) — `gtk_options: Vec<String>` with `trailing_var_arg = true`
- **Question:** Why `allow_hyphen_values = true`? What happens if GTK passes `--display :0`?
- **Rust concepts:** `Vec<String>`, argument parsing edge cases, `String` vs `&str`
- **Rust Book:** [Ch 8.1 Vectors](https://doc.rust-lang.org/book/ch08-01-vectors.html), [Ch 4.3 Slices](https://doc.rust-lang.org/book/ch04-03-slices.html)
- **Exercise:** Run `cargo run -- --help`. Read the generated help text. Then run `cargo run -- --sessions-dir tests/fixtures`.
- **Verify:** `cargo run -- --help`

### Step 3: Wiring into startup
- **File:** [src/session_sources.rs](src/session_sources.rs) — `SessionSources::resolve(override_root)`
- **Question:** How does `override_root: Option<&Path>` become the `override_mode` flag? What's the `&Path` vs `PathBuf` distinction?
- **Rust concepts:** Borrowing, `&Path` vs `PathBuf`, `Option` as control flow
- **Rust Book:** [Ch 4.2 References](https://doc.rust-lang.org/book/ch04-02-references-and-borrowing.html)
- **Exercise:** Trace how `--sessions-dir /tmp/test` flows from `Args::parse()` to `select_db_filename()`. Draw the call chain.
- **Verify:** `cargo check`

---

## PATH F — Errors & Tracing

**Goal:** Understand Rust error handling idioms and structured logging.

### Step 1: Tracing setup
- **File:** [src/main.rs](src/main.rs) — `tracing_subscriber::fmt()...init()`
- **Question:** What does `FmtSpan::FULL` do? What would you change to enable `RUST_LOG` env var filtering?
- **Rust concepts:** Builder pattern, method chaining, library configuration
- **Rust Book:** [Ch 12.5 Env Variables](https://doc.rust-lang.org/book/ch12-05-working-with-environment-variables.html)
- **Exercise:** Change `with_max_level(Level::INFO)` to `Level::DEBUG`. Rebuild and observe extra output.
- **Verify:** `cargo check`

### Step 2: anyhow vs. thiserror
- **Files:** [src/database/mod.rs](src/database/mod.rs) (uses `anyhow`), [src/parsers/codex.rs](src/parsers/codex.rs) (uses `thiserror`)
- **Question:** When do you use `anyhow::Result` vs. a custom `thiserror` enum? Find one example of each.
- **Rust concepts:** Error trait, `anyhow::Context`, `downcast_ref`, when to type errors
- **Rust Book:** [Ch 9 Error Handling](https://doc.rust-lang.org/book/ch09-00-error-handling.html)
- **Exercise:** Find `downcast_ref::<ParseError>()` in the indexer. Explain why the caller needs the concrete error type.
- **Verify:** `cargo check`

### Step 3: The unwrap audit
- **Files:** [src/main.rs](src/main.rs), [src/app.rs](src/app.rs)
- **Question:** Find every `unwrap()` and `expect()` in `main.rs` and `app.rs`. Which are justified? Which could be improved?
- **Rust concepts:** `unwrap` vs `expect` vs `?`, panic safety, startup vs runtime
- **Rust Book:** [Ch 9.3 To Panic or Not](https://doc.rust-lang.org/book/ch09-03-to-panic-or-not-to-panic.html)
- **Exercise:** Replace one `unwrap()` in `app.rs` with proper error handling using `?` or `if let`.
- **Verify:** `cargo check && cargo clippy --all -- -D warnings`

### Step 4: Structured logging fields
- **Files:** [src/parsers/model.rs](src/parsers/model.rs) — `tracing::debug!(?value, "...")`, [src/database/mod.rs](src/database/mod.rs) — elapsed timing
- **Question:** What does `?value` mean in a `tracing::debug!` call? How is it different from `%value`?
- **Rust concepts:** `Debug` vs `Display` traits, structured logging, `std::time::Instant`
- **Rust Book:** [Ch 5.2 Debug Trait](https://doc.rust-lang.org/book/ch05-02-example-structs.html)
- **Exercise:** Add `tracing::debug!(count = results.len(), "search complete")` to the search function. Verify the structured field appears.
- **Verify:** `cargo check`
