//! Opt-in timing harness for the Claude Code parser.
//!
//! Ignored by default. Run with:
//!
//! ```sh
//! CLAUDE_PARSE_TIMING_FILE=/path/to/session.jsonl \
//! CLAUDE_PARSE_TIMING_RUNS=20 \
//! dbus-run-session -- xvfb-run -a env GDK_BACKEND=x11 GSK_RENDERER=cairo \
//!     cargo test --release -p sessions-chronicle-core --test claude_parse_timing -- --ignored --nocapture
//! ```
//!
//! Without `CLAUDE_PARSE_TIMING_FILE` the test prints a skip note and passes.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use sessions_chronicle_core::parsers::claude_code::ClaudeCodeParser;

const FILE_VAR: &str = "CLAUDE_PARSE_TIMING_FILE";
const RUNS_VAR: &str = "CLAUDE_PARSE_TIMING_RUNS";
const DEFAULT_RUNS: usize = 20;

/// Top-level `type` values the Claude parser consumes; any other value is
/// workload the parser largely skips.
const CONSUMED_TYPES: [&str; 3] = ["user", "assistant", "ai-title"];

#[derive(Debug, Default)]
struct WorkloadStats {
    file_bytes: u64,
    lines: u64,
    nonblank_lines: u64,
    skipped_type_lines: u64,
    skipped_type_bytes: u64,
    malformed_lines: u64,
    malformed_bytes: u64,
}

fn parse_runs(value: Option<std::ffi::OsString>) -> Result<usize> {
    let Some(value) = value else {
        return Ok(DEFAULT_RUNS);
    };
    let text = value
        .into_string()
        .map_err(|_| anyhow::anyhow!("{RUNS_VAR} must be valid UTF-8"))?;
    let runs: usize = text
        .trim()
        .parse()
        .with_context(|| format!("{RUNS_VAR} must be a positive integer, got {text:?}"))?;
    if runs == 0 {
        bail!("{RUNS_VAR} must be a positive integer, got 0");
    }
    Ok(runs)
}

fn collect_workload(path: &Path) -> Result<WorkloadStats> {
    let file = File::open(path).context("Failed to open timing session file")?;
    let mut reader = BufReader::new(file);
    let mut stats = WorkloadStats::default();
    let mut line = String::new();

    loop {
        line.clear();
        // `read_line` keeps line endings, so byte totals match the file size.
        let read = reader
            .read_line(&mut line)
            .context("Failed to read timing session file")?;
        if read == 0 {
            break;
        }
        stats.file_bytes += read as u64;
        stats.lines += 1;

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        stats.nonblank_lines += 1;

        match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(serde_json::Value::Object(object)) => {
                let consumed = object
                    .get("type")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|kind| CONSUMED_TYPES.contains(&kind));
                if !consumed {
                    stats.skipped_type_lines += 1;
                    stats.skipped_type_bytes += read as u64;
                }
            }
            _ => {
                stats.malformed_lines += 1;
                stats.malformed_bytes += read as u64;
            }
        }
    }

    Ok(stats)
}

fn print_workload(path: &Path, stats: &WorkloadStats) {
    let share = if stats.file_bytes == 0 {
        0.0
    } else {
        100.0 * stats.skipped_type_bytes as f64 / stats.file_bytes as f64
    };
    println!("workload file={}", path.display());
    println!(
        "workload file_bytes={} lines={} nonblank_lines={}",
        stats.file_bytes, stats.lines, stats.nonblank_lines
    );
    println!(
        "workload skipped_type_lines={} skipped_type_bytes={} skipped_type_byte_share={share:.2}%",
        stats.skipped_type_lines, stats.skipped_type_bytes
    );
    println!(
        "workload malformed_lines={} malformed_bytes={} (excluded from type share)",
        stats.malformed_lines, stats.malformed_bytes
    );
}

fn median(sorted: &[Duration]) -> f64 {
    let n = sorted.len();
    let ms = |d: &Duration| d.as_secs_f64() * 1000.0;
    if n % 2 == 1 {
        ms(&sorted[n / 2])
    } else {
        (ms(&sorted[n / 2 - 1]) + ms(&sorted[n / 2])) / 2.0
    }
}

#[test]
#[ignore = "opt-in timing run; set CLAUDE_PARSE_TIMING_FILE"]
fn claude_parse_timing() -> Result<()> {
    let Some(file) = std::env::var_os(FILE_VAR) else {
        eprintln!("claude_parse_timing: skipped, {FILE_VAR} is not set");
        return Ok(());
    };
    let path = PathBuf::from(file);
    let runs = parse_runs(std::env::var_os(RUNS_VAR))?;
    let parser = ClaudeCodeParser;

    let stats = collect_workload(&path)?;
    print_workload(&path, &stats);

    // Untimed warm-up parse; it must succeed before any sample is taken.
    let warmup = parser
        .parse(&path)
        .with_context(|| format!("warm-up parse failed for {}", path.display()))?;
    std::hint::black_box(warmup);

    let mut durations = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        let parsed = parser
            .parse(&path)
            .with_context(|| format!("timed parse failed for {}", path.display()))?;
        durations.push(start.elapsed());
        std::hint::black_box(parsed);
    }

    durations.sort();
    let min = durations[0].as_secs_f64() * 1000.0;
    let max = durations[durations.len() - 1].as_secs_f64() * 1000.0;
    let median = median(&durations);
    println!(
        "timing samples={} median_ms={median:.3} min_ms={min:.3} max_ms={max:.3}",
        durations.len()
    );

    Ok(())
}
