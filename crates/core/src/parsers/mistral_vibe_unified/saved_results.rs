//! Receipt detection and local saved-result resolution (fact K).
//!
//! A large tool result is replaced in the projection by a receipt text
//! pointing at the record the host saved; the host always writes that
//! record into the session's own `tool-results/` directory, so resolution
//! takes only the receipt's percent-encoded basename and stays inside the
//! local store's symlink boundary.

use std::fs;
use std::io;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

use regex::Regex;
use std::sync::LazyLock;

use super::error::UnifiedError;
use super::store::reject_symlink;

pub use super::file_stats::FileStat;

/// Receipt sentence (fact K): the saving host names the absolute path and
/// the record's UTF-16 size.
static RECEIPT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"saved to the file system under (?P<path>[^\n]+)\.\nOutput size: (?P<n>[0-9]+) characters",
    )
    .expect("saved-result receipt regex compiles")
});

/// Encoded saved-result file name (fact K): `encode` keeps
/// `[A-Za-z0-9-_.!~*'()]` and writes other bytes as `%XX` hex triplets,
/// and the record always ends in `.json`.  Malformed escapes (`%`,
/// `%Q1`, `%1`) are rejected.
static ENCODED_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:[A-Za-z0-9\-_.!~*'()]|%[0-9A-Fa-f]{2})+\.json$")
        .expect("encoded saved-result name regex compiles")
});

/// Resolves receipt texts against a session's local `tool-results/`
/// records and collects the consumed files' stats for the freshness
/// signature.
pub(crate) struct SavedResultResolver {
    pub(crate) tool_results_dir: PathBuf,
    pub(crate) consumed: Vec<(String, FileStat)>,
}

impl SavedResultResolver {
    pub(crate) fn new(tool_results_dir: PathBuf) -> Self {
        Self {
            tool_results_dir,
            consumed: Vec::new(),
        }
    }

    /// Resolves the saved local record behind `receipt_text`.
    ///
    /// `Ok(None)` — the text is not a receipt and the caller falls back
    /// to the regular output mapping.  A receipt whose path does not point
    /// into a `tool-results` directory, whose encoded file name is
    /// invalid, or whose record fails validation is an error.
    pub(crate) fn resolve(&mut self, receipt_text: &str) -> Result<Option<String>, UnifiedError> {
        let Some((path, expected_utf16_len)) = parse_receipt(receipt_text) else {
            return Ok(None);
        };
        // The receipt names the host that saved the record (fact K: an
        // absolute path); only its encoded basename is trusted and used,
        // so resolution always stays inside the local tool-results
        // directory whatever host path the receipt carries.
        let path = Path::new(path);
        if !path.is_absolute() {
            return Err(UnifiedError::Corrupt(format!(
                "saved-result receipt path {path:?} is not absolute"
            )));
        }
        if path
            .components()
            .any(|component| component == Component::ParentDir)
        {
            return Err(UnifiedError::Corrupt(format!(
                "saved-result receipt path {path:?} contains a traversal component"
            )));
        }
        let parent_is_tool_results = path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "tool-results");
        if !parent_is_tool_results {
            return Err(UnifiedError::Corrupt(format!(
                "saved-result receipt path {path:?} does not point into a tool-results directory"
            )));
        }
        let Some(basename) = path.file_name().and_then(|name| name.to_str()) else {
            return Err(UnifiedError::Corrupt(format!(
                "saved-result receipt path {path:?} has no file name"
            )));
        };
        if !ENCODED_NAME.is_match(basename) {
            return Err(UnifiedError::Corrupt(format!(
                "saved-result name {basename:?} is not an encoded .json file name"
            )));
        }
        // The store's symlink boundary applies to the directory; the
        // record itself is validated, opened and read through one handle
        // below. Device/inode comparison detects substitution of a different
        // file; a same-inode symlink/hardlink ABA can still evade the literal
        // no-symlink rule (it cannot substitute another inode's contents).
        reject_symlink(&self.tool_results_dir)?;
        let local_path = self.tool_results_dir.join(basename);
        // No-follow validation of the record: a symlink is corrupt, a
        // missing record is incomplete data.
        let validated = fs::symlink_metadata(&local_path).map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => UnifiedError::MissingSavedResult(format!(
                "saved result {basename} referenced by a receipt is missing"
            )),
            _ => UnifiedError::Io(error),
        })?;
        if validated.file_type().is_symlink() {
            return Err(UnifiedError::Corrupt(format!(
                "saved result {basename} is a symlink"
            )));
        }
        #[cfg(test)]
        test_hooks::run_between_check_and_open();
        // Open the record and fstat that same handle: whatever the path
        // points at now, only the validated file itself may be read.
        let mut file = fs::File::open(&local_path).map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => UnifiedError::MissingSavedResult(format!(
                "saved result {basename} referenced by a receipt is missing"
            )),
            _ => UnifiedError::Io(error),
        })?;
        let opened = file.metadata().map_err(UnifiedError::Io)?;
        if opened.dev() != validated.dev() || opened.ino() != validated.ino() {
            return Err(UnifiedError::Corrupt(format!(
                "saved result {basename} changed while it was being read"
            )));
        }
        let mut raw = Vec::new();
        file.read_to_end(&mut raw).map_err(UnifiedError::Io)?;
        let content = String::from_utf8(raw).map_err(|_| {
            UnifiedError::Corrupt(format!("saved result {basename} is not valid UTF-8"))
        })?;
        let utf16_len = content.encode_utf16().count() as u64;
        if utf16_len != expected_utf16_len {
            return Err(UnifiedError::Corrupt(format!(
                "saved result {basename} holds {utf16_len} UTF-16 characters, the receipt says {expected_utf16_len}"
            )));
        }
        // Fact K: the record holds pretty JSON; validate before it is
        // recorded as consumed.
        if let Err(error) = serde_json::from_str::<serde_json::Value>(&content) {
            return Err(UnifiedError::Corrupt(format!(
                "saved result {basename} is not valid JSON: {error}"
            )));
        }
        self.consumed.push((
            basename.to_string(),
            // The stat belongs to the handle that produced the consumed
            // bytes, not to whatever the path points at now.
            FileStat::from_metadata(&opened),
        ));
        Ok(Some(content))
    }

    /// The saved results consumed by the conversion, for freshness
    /// signatures.
    pub(crate) fn into_consumed(self) -> Vec<(String, FileStat)> {
        self.consumed
    }
}

/// Receipt shape (fact K): the text carries the truncation marker and the
/// exact saving sentence with the absolute path and the record's UTF-16
/// size.  Anything else is not a receipt.
fn parse_receipt(text: &str) -> Option<(&str, u64)> {
    if !text.contains("<truncated-output>") {
        return None;
    }
    let captures = RECEIPT.captures(text)?;
    let path = captures.name("path")?.as_str();
    let size = captures.name("n")?.as_str().parse().ok()?;
    Some((path, size))
}

/// Test seam for the check/use race: runs once, between the record's
/// no-follow validation and its open, so tests can swap the record (or
/// the tool-results directory) deterministically at the exact point an
/// attacker would.  Thread-local so parallel test threads cannot consume
/// each other's hooks.
#[cfg(test)]
mod test_hooks {
    use std::cell::RefCell;

    type Hook = Box<dyn FnOnce() + Send>;

    thread_local! {
        static BETWEEN_CHECK_AND_OPEN: RefCell<Option<Hook>> = const { RefCell::new(None) };
    }

    /// Installs the hook for the current thread's next `resolve` that
    /// reaches the seam.
    pub(crate) fn on_between_check_and_open(hook: Hook) {
        BETWEEN_CHECK_AND_OPEN.with(|slot| *slot.borrow_mut() = Some(hook));
    }

    pub(crate) fn run_between_check_and_open() {
        let hook = BETWEEN_CHECK_AND_OPEN.with(|slot| slot.borrow_mut().take());
        if let Some(hook) = hook {
            hook();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsers::mistral_vibe_unified::store::test_support::error_variant;
    use std::fs;
    use std::os::unix::fs::MetadataExt;
    use tempfile::TempDir;

    /// Receipt text shaped exactly like the upstream adapter's (fact K).
    fn receipt(path: &str, size: usize) -> String {
        format!(
            "<system>The tool output is large. To preserve your context window, it was saved to the file system under {path}.\nOutput size: {size} characters\nMax size before saving: 16000 characters</system>\n<truncated-output>\nThe full output is available in the saved file.\n</truncated-output>"
        )
    }

    fn temp_tool_results() -> (TempDir, PathBuf) {
        let temp = TempDir::new().expect("temp dir");
        let dir = temp.path().join("tool-results");
        fs::create_dir_all(&dir).unwrap();
        (temp, dir)
    }

    fn write_saved(dir: &Path, name: &str, content: &str) {
        fs::write(dir.join(name), content).unwrap();
    }

    #[test]
    fn plain_output_text_is_not_a_receipt() {
        let (temp, dir) = temp_tool_results();
        let mut resolver = SavedResultResolver::new(dir);
        assert_eq!(resolver.resolve("[package]").unwrap(), None);

        // The truncation marker alone does not make a receipt.
        let no_sentence = "<truncated-output>\nThe full output is available in the saved file.\n</truncated-output>".to_string();
        assert_eq!(resolver.resolve(&no_sentence).unwrap(), None);

        // The saving sentence without the marker is not a receipt either.
        let no_marker =
            receipt("/x/tool-results/saved.json", 3).replacen("<truncated-output>", "plain", 1);
        assert_eq!(resolver.resolve(&no_marker).unwrap(), None);

        assert!(resolver.into_consumed().is_empty());
        let _ = temp;
    }

    #[test]
    fn receipt_resolves_local_record_and_records_stat() {
        let (temp, dir) = temp_tool_results();
        let content = r#"{"out":"abc"}"#;
        write_saved(&dir, "saved.json", content);

        let mut resolver = SavedResultResolver::new(dir.clone());
        let resolved = resolver
            .resolve(&receipt(
                "/home/host/.vibe/logs/session/unified/x/tool-results/saved.json",
                content.encode_utf16().count(),
            ))
            .unwrap()
            .expect("the local record resolves");
        assert_eq!(resolved, content);

        let consumed = resolver.into_consumed();
        assert_eq!(consumed.len(), 1);
        assert_eq!(consumed[0].0, "saved.json");
        assert_eq!(consumed[0].1.size, content.len() as u64);
        let metadata = fs::symlink_metadata(dir.join("saved.json")).unwrap();
        assert_eq!(
            consumed[0].1.mtime_ns,
            metadata.mtime() * 1_000_000_000 + metadata.mtime_nsec()
        );
        let _ = temp;
    }

    #[test]
    fn sandbox_host_path_resolves_local_basename() {
        // A sandbox receipt names a tmp host directory; only the
        // basename is used, so the store's own record resolves.
        let (temp, dir) = temp_tool_results();
        let content = r#"{"out":"sandbox"}"#;
        write_saved(&dir, "bash-call-7.json", content);
        let mut resolver = SavedResultResolver::new(dir);
        let resolved = resolver
            .resolve(&receipt(
                "/tmp/vibe-sandbox-1a2b/tool-results/bash-call-7.json",
                content.encode_utf16().count(),
            ))
            .unwrap()
            .expect("the sandbox record resolves");
        assert_eq!(resolved, content);
        let _ = temp;
    }

    #[test]
    fn receipt_escaping_tool_results_is_corrupt() {
        let (temp, dir) = temp_tool_results();
        write_saved(&dir, "meta.json", "{}");
        let mut resolver = SavedResultResolver::new(dir);
        let error = resolver
            .resolve(&receipt("/x/tool-results/../meta.json", 2))
            .unwrap_err();
        assert_eq!(error_variant(&error), "Corrupt");
        let _ = temp;
    }

    #[test]
    fn receipt_outside_tool_results_is_corrupt() {
        let (temp, dir) = temp_tool_results();
        let mut resolver = SavedResultResolver::new(dir);
        let error = resolver.resolve(&receipt("/etc/passwd", 1)).unwrap_err();
        assert_eq!(error_variant(&error), "Corrupt");
        let _ = temp;
    }

    #[test]
    fn receipt_with_invalid_name_is_corrupt() {
        let (temp, dir) = temp_tool_results();
        let mut resolver = SavedResultResolver::new(dir);
        // A space is not part of the encoded file-name alphabet.
        let error = resolver
            .resolve(&receipt("/x/tool-results/bad name.json", 1))
            .unwrap_err();
        assert_eq!(error_variant(&error), "Corrupt");
        let _ = temp;
    }

    #[test]
    fn missing_record_is_missing_saved_result() {
        let (temp, dir) = temp_tool_results();
        let mut resolver = SavedResultResolver::new(dir);
        let error = resolver
            .resolve(&receipt("/x/tool-results/saved.json", 3))
            .unwrap_err();
        assert_eq!(error_variant(&error), "MissingSavedResult");
        let _ = temp;
    }

    #[test]
    fn utf16_length_mismatch_is_corrupt() {
        let (temp, dir) = temp_tool_results();
        write_saved(&dir, "saved.json", "abcdef"); // 6 UTF-16 units
        let mut resolver = SavedResultResolver::new(dir);
        let error = resolver
            .resolve(&receipt("/x/tool-results/saved.json", 5))
            .unwrap_err();
        assert_eq!(error_variant(&error), "Corrupt");
        let _ = temp;
    }

    #[test]
    fn utf16_length_counts_code_units_not_chars() {
        let (temp, dir) = temp_tool_results();
        // The emoji is one char but two UTF-16 code units.
        let content = "{\"e\":\"🚀é\"}";
        let units = content.encode_utf16().count();
        let chars = content.chars().count();
        assert_eq!(units, chars + 1, "fixture premise: the emoji doubles");
        write_saved(&dir, "saved.json", content);

        let mut resolver = SavedResultResolver::new(dir.clone());
        let resolved = resolver
            .resolve(&receipt("/x/tool-results/saved.json", units))
            .unwrap()
            .expect("UTF-16 units match");
        assert_eq!(resolved, content);

        let mut resolver = SavedResultResolver::new(dir);
        let error = resolver
            .resolve(&receipt("/x/tool-results/saved.json", chars))
            .unwrap_err();
        assert_eq!(error_variant(&error), "Corrupt");
        let _ = temp;
    }

    #[test]
    fn symlinked_record_is_corrupt() {
        let (temp, dir) = temp_tool_results();
        let target = temp.path().join("target.json");
        fs::write(&target, "{}").unwrap();
        std::os::unix::fs::symlink(&target, dir.join("saved.json")).unwrap();
        let mut resolver = SavedResultResolver::new(dir);
        let error = resolver
            .resolve(&receipt("/x/tool-results/saved.json", 2))
            .unwrap_err();
        assert_eq!(error_variant(&error), "Corrupt");
        let _ = temp;
    }

    #[test]
    fn symlinked_tool_results_dir_is_corrupt() {
        let temp = TempDir::new().expect("temp dir");
        let real = temp.path().join("real-tool-results");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("saved.json"), "abc").unwrap();
        let link = temp.path().join("tool-results");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let mut resolver = SavedResultResolver::new(link);
        let error = resolver
            .resolve(&receipt("/x/tool-results/saved.json", 3))
            .unwrap_err();
        assert_eq!(error_variant(&error), "Corrupt");
    }

    // ------------------------------------------------------------------
    // Review fixes: path shape, percent triplets, JSON validity and the
    // check/use race (swaps happen at the deterministic test seam).
    // ------------------------------------------------------------------

    #[test]
    fn receipt_with_relative_path_is_corrupt() {
        let (temp, dir) = temp_tool_results();
        write_saved(&dir, "saved.json", r#"{"out":"abc"}"#);
        let mut resolver = SavedResultResolver::new(dir);
        // Relative: a `tool-results` parent component alone is not
        // enough; the receipt names an absolute host path (fact K).
        let error = resolver
            .resolve(&receipt("tool-results/saved.json", 13))
            .unwrap_err();
        assert_eq!(error_variant(&error), "Corrupt");
        let _ = temp;
    }

    #[test]
    fn receipt_with_traversal_component_is_corrupt() {
        let (temp, dir) = temp_tool_results();
        write_saved(&dir, "saved.json", r#"{"out":"abc"}"#);
        let mut resolver = SavedResultResolver::new(dir);
        // The review's example: the final parent component is
        // `tool-results`, but a `..` component traverses elsewhere.
        let error = resolver
            .resolve(&receipt(
                "/tmp/tool-results/../elsewhere/tool-results/saved.json",
                13,
            ))
            .unwrap_err();
        assert_eq!(error_variant(&error), "Corrupt");
        let _ = temp;
    }

    #[test]
    fn receipt_with_malformed_percent_escape_is_corrupt() {
        let (temp, dir) = temp_tool_results();
        let mut resolver = SavedResultResolver::new(dir);
        // Only valid `%XX` triplets are encoded names: a bare `%`, a
        // non-hex `%Q1` and an incomplete `%1` are all rejected.
        for name in ["bash-call%.json", "bash-call%Q19.json", "bash-call%1.json"] {
            let error = resolver
                .resolve(&receipt(&format!("/x/tool-results/{name}"), 3))
                .unwrap_err();
            assert_eq!(
                error_variant(&error),
                "Corrupt",
                "malformed percent escape {name:?} must be rejected"
            );
        }
        let _ = temp;
    }

    #[test]
    fn invalid_json_record_is_corrupt() {
        let (temp, dir) = temp_tool_results();
        let content = "not json at all";
        write_saved(&dir, "saved.json", content);
        let mut resolver = SavedResultResolver::new(dir);
        // Fact K: the record holds pretty JSON; a same-length non-JSON
        // file is not a saved result.
        let error = resolver
            .resolve(&receipt(
                "/x/tool-results/saved.json",
                content.encode_utf16().count(),
            ))
            .unwrap_err();
        assert_eq!(error_variant(&error), "Corrupt");
        let _ = temp;
    }

    #[test]
    fn record_swapped_for_symlink_after_check_is_rejected() {
        let (temp, dir) = temp_tool_results();
        let content = r#"{"out":"abc"}"#;
        write_saved(&dir, "saved.json", content);
        // The swap target holds identical, fully valid content — only
        // the identity of the file behind the path changes.
        let target = temp.path().join("elsewhere.json");
        fs::write(&target, content).unwrap();
        let swap_dir = dir.clone();
        test_hooks::on_between_check_and_open(Box::new(move || {
            fs::remove_file(swap_dir.join("saved.json")).unwrap();
            std::os::unix::fs::symlink(&target, swap_dir.join("saved.json")).unwrap();
        }));

        let mut resolver = SavedResultResolver::new(dir);
        let error = resolver
            .resolve(&receipt(
                "/x/tool-results/saved.json",
                content.encode_utf16().count(),
            ))
            .unwrap_err();
        assert_eq!(error_variant(&error), "Corrupt");
        let _ = temp;
    }

    #[test]
    fn record_swapped_for_other_file_after_check_is_rejected() {
        let (temp, dir) = temp_tool_results();
        let content = r#"{"out":"abc"}"#;
        write_saved(&dir, "saved.json", content);
        // A different regular file with identical bytes: same content,
        // different inode — still a swap.
        let swap_dir = dir.clone();
        let swap_content = content.to_string();
        test_hooks::on_between_check_and_open(Box::new(move || {
            fs::remove_file(swap_dir.join("saved.json")).unwrap();
            fs::write(swap_dir.join("saved.json"), swap_content).unwrap();
        }));

        let mut resolver = SavedResultResolver::new(dir);
        let error = resolver
            .resolve(&receipt(
                "/x/tool-results/saved.json",
                content.encode_utf16().count(),
            ))
            .unwrap_err();
        assert_eq!(error_variant(&error), "Corrupt");
        let _ = temp;
    }

    #[test]
    fn tool_results_dir_swapped_for_symlink_after_check_is_rejected() {
        let temp = TempDir::new().expect("temp dir");
        let dir = temp.path().join("tool-results");
        fs::create_dir_all(&dir).unwrap();
        let content = r#"{"out":"abc"}"#;
        fs::write(dir.join("saved.json"), content).unwrap();
        // A twin directory with an identical record: swapping the
        // directory mid-resolution must still be detected.
        let twin = temp.path().join("twin-tool-results");
        fs::create_dir_all(&twin).unwrap();
        fs::write(twin.join("saved.json"), content).unwrap();
        let swap_dir = dir.clone();
        test_hooks::on_between_check_and_open(Box::new(move || {
            fs::remove_dir_all(&swap_dir).unwrap();
            std::os::unix::fs::symlink(&twin, &swap_dir).unwrap();
        }));

        let mut resolver = SavedResultResolver::new(dir);
        let error = resolver
            .resolve(&receipt(
                "/x/tool-results/saved.json",
                content.encode_utf16().count(),
            ))
            .unwrap_err();
        assert_eq!(error_variant(&error), "Corrupt");
    }
}
