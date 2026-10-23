//! Reader for Mistral Vibe 3.0.x Unified session stores
//! (`<vibe-root>/unified/<id>/`, `mistral.vibe.unified-session-store/v1`).

mod canonical;
mod convert;
mod effects;
mod error;
mod journal;
pub(crate) mod store;

use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use store::LoadedStore;

pub use convert::{MistralVibeUnifiedParser, UnifiedParse};
pub use error::UnifiedError;
pub use store::{child_session_id, is_unified_session_dir};

/// Loads one generation, replays its journal segment and hands the
/// loaded store to `build`, guarding the whole read with a retry-once
/// stability loop.
///
/// After `build` runs, `CURRENT` is re-read and the journal segment is
/// re-stat'ed: the read is retried once when the pointer bytes changed,
/// the journal was replaced (different `dev`/`ino`), the segment is
/// shorter than the sampled size, or the attempt failed with
/// [`UnifiedError::Incomplete`] while the pointer moved — a concurrent
/// publication can briefly remove referenced files.  A second
/// instability is [`UnifiedError::Unstable`]; every other error is
/// returned as-is.  The returned store keeps the journal sample taken
/// before the replay, the validated prefix a caller should publish.
pub(crate) fn load_stable<T>(
    session_dir: &Path,
    build: impl Fn(&LoadedStore) -> Result<T, UnifiedError>,
) -> Result<(T, LoadedStore), UnifiedError> {
    let mut instability: Option<String> = None;
    for _ in 0..2 {
        // Baseline the pointer before the attempt so a mid-read failure
        // can be attributed to a publication in flight.
        let attempt_current = fs::read(session_dir.join("CURRENT"));
        match stable_attempt(session_dir, &build) {
            Ok((value, store)) => {
                if let Err(reason) = read_is_stable(session_dir, &store) {
                    instability = Some(reason);
                    continue;
                }
                return Ok((value, store));
            }
            Err(error) => {
                let current_now = fs::read(session_dir.join("CURRENT"));
                if matches!(error, UnifiedError::Incomplete(_))
                    && !same_pointer_read(&current_now, &attempt_current)
                {
                    instability =
                        Some("CURRENT changed while referenced files went missing".to_string());
                    continue;
                }
                return Err(error);
            }
        }
    }
    Err(UnifiedError::Unstable(instability.unwrap_or_else(|| {
        "store changed during the read".to_string()
    })))
}

/// One full read attempt: generation load, journal replay, build.
fn stable_attempt<T>(
    session_dir: &Path,
    build: &impl Fn(&LoadedStore) -> Result<T, UnifiedError>,
) -> Result<(T, LoadedStore), UnifiedError> {
    let mut store = store::load_generation(session_dir)?;
    journal::replay_journal(session_dir, &mut store)?;
    let value = build(&store)?;
    Ok((value, store))
}

/// Re-reads the pointer and the journal metadata, comparing them with
/// the samples taken while loading: byte-identical `CURRENT`, the same
/// journal `dev`/`ino`, and a size at or above the sample.
fn read_is_stable(session_dir: &Path, store: &LoadedStore) -> Result<(), String> {
    let current = fs::read(session_dir.join("CURRENT"))
        .map_err(|_| "CURRENT became unreadable during the read".to_string())?;
    if current != store.current_bytes {
        return Err("CURRENT changed during the read".to_string());
    }
    let journal_path = session_dir.join(&store.journal.relative_path);
    let metadata = fs::symlink_metadata(&journal_path).map_err(|_| {
        format!(
            "journal segment {} disappeared during the read",
            store.journal.relative_path
        )
    })?;
    if metadata.dev() != store.journal.dev || metadata.ino() != store.journal.ino {
        return Err(format!(
            "journal segment {} was replaced during the read",
            store.journal.relative_path
        ));
    }
    if metadata.len() < store.journal.size {
        return Err(format!(
            "journal segment {} shrank during the read",
            store.journal.relative_path
        ));
    }
    Ok(())
}

/// Compares two pointer reads: byte-identical bodies, or the same
/// failure to produce them.
fn same_pointer_read(
    current: &Result<Vec<u8>, io::Error>,
    baseline: &Result<Vec<u8>, io::Error>,
) -> bool {
    match (current, baseline) {
        (Ok(current), Ok(baseline)) => current == baseline,
        (Err(_), Err(_)) => true,
        _ => false,
    }
}
