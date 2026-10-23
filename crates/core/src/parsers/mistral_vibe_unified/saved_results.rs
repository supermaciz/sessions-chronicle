//! Receipt detection and local saved-result resolution (fact K).
//!
//! A large tool result is replaced in the projection by a receipt text
//! pointing at the record the host saved; the host always writes that
//! record into the session's own `tool-results/` directory, so resolution
//! takes only the receipt's percent-encoded basename and stays inside the
//! local store's symlink boundary.

use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

use super::error::UnifiedError;
use super::store::reject_symlink;

/// Receipt sentence (fact K): the saving host names the absolute path and
/// the record's UTF-16 size.
static RECEIPT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"saved to the file system under (?P<path>[^\n]+)\.\nOutput size: (?P<n>[0-9]+) characters",
    )
    .expect("saved-result receipt regex compiles")
});

/// A `symlink_metadata` snapshot of one consumed saved result, used for
/// freshness signatures.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct FileStat {
    pub size: u64,
    /// Modification time in nanoseconds since the Unix epoch.
    pub mtime_ns: i64,
}

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
        // The receipt names the host that saved the record; only its
        // encoded basename is trusted and used, so resolution always
        // stays inside the local tool-results directory.
        let path = Path::new(path);
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
        if !is_encoded_json_name(basename) {
            return Err(UnifiedError::Corrupt(format!(
                "saved-result name {basename:?} is not an encoded .json file name"
            )));
        }
        // The store's symlink boundary applies to the directory and the
        // record itself.
        reject_symlink(&self.tool_results_dir)?;
        let local_path = self.tool_results_dir.join(basename);
        reject_symlink(&local_path)?;
        let metadata = fs::symlink_metadata(&local_path).map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => UnifiedError::MissingSavedResult(format!(
                "saved result {basename} referenced by a receipt is missing"
            )),
            _ => UnifiedError::Io(error),
        })?;
        let raw = fs::read(&local_path).map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => UnifiedError::MissingSavedResult(format!(
                "saved result {basename} referenced by a receipt is missing"
            )),
            _ => UnifiedError::Io(error),
        })?;
        let content = String::from_utf8(raw).map_err(|_| {
            UnifiedError::Corrupt(format!("saved result {basename} is not valid UTF-8"))
        })?;
        let utf16_len = content.encode_utf16().count() as u64;
        if utf16_len != expected_utf16_len {
            return Err(UnifiedError::Corrupt(format!(
                "saved result {basename} holds {utf16_len} UTF-16 characters, the receipt says {expected_utf16_len}"
            )));
        }
        self.consumed.push((
            basename.to_string(),
            FileStat {
                size: metadata.len(),
                mtime_ns: metadata.mtime() * 1_000_000_000 + metadata.mtime_nsec() as i64,
            },
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

/// Encoded saved-result file name (fact K): `encode` keeps
/// `[A-Za-z0-9-_.!~*'()]` and writes other bytes as `%XX`, and the record
/// always ends in `.json`.
fn is_encoded_json_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".json") else {
        return false;
    };
    !stem.is_empty()
        && stem.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' | b'%'
                )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn write_saved(dir: &std::path::Path, name: &str, content: &str) {
        fs::write(dir.join(name), content).unwrap();
    }

    fn error_variant(error: &UnifiedError) -> &'static str {
        match error {
            UnifiedError::NoUserMessages => "NoUserMessages",
            UnifiedError::Ephemeral => "Ephemeral",
            UnifiedError::DeletingTree { .. } => "DeletingTree",
            UnifiedError::Unsupported(_) => "Unsupported",
            UnifiedError::Corrupt(_) => "Corrupt",
            UnifiedError::Incomplete(_) => "Incomplete",
            UnifiedError::IdentityConflict(_) => "IdentityConflict",
            UnifiedError::Unstable(_) => "Unstable",
            UnifiedError::MissingSavedResult(_) => "MissingSavedResult",
            UnifiedError::Io(_) => "Io",
        }
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
        write_saved(&dir, "saved.json", "abc");

        let mut resolver = SavedResultResolver::new(dir.clone());
        let resolved = resolver
            .resolve(&receipt(
                "/home/host/.vibe/logs/session/unified/x/tool-results/saved.json",
                3,
            ))
            .unwrap()
            .expect("the local record resolves");
        assert_eq!(resolved, "abc");

        let consumed = resolver.into_consumed();
        assert_eq!(consumed.len(), 1);
        assert_eq!(consumed[0].0, "saved.json");
        assert_eq!(consumed[0].1.size, 3);
        let metadata = fs::symlink_metadata(dir.join("saved.json")).unwrap();
        assert_eq!(
            consumed[0].1.mtime_ns,
            metadata.mtime() * 1_000_000_000 + metadata.mtime_nsec() as i64
        );
        let _ = temp;
    }

    #[test]
    fn sandbox_host_path_resolves_local_basename() {
        // A sandbox receipt names a tmp host directory; only the
        // basename is used, so the store's own record resolves.
        let (temp, dir) = temp_tool_results();
        write_saved(&dir, "bash-call-7.json", "sandbox output");
        let mut resolver = SavedResultResolver::new(dir);
        let resolved = resolver
            .resolve(&receipt(
                "/tmp/vibe-sandbox-1a2b/tool-results/bash-call-7.json",
                14, // "sandbox output" in UTF-16 code units
            ))
            .unwrap()
            .expect("the sandbox record resolves");
        assert_eq!(resolved, "sandbox output");
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
        write_saved(&dir, "saved.json", "🚀é"); // 3 UTF-16 units, 2 chars

        let mut resolver = SavedResultResolver::new(dir.clone());
        let resolved = resolver
            .resolve(&receipt("/x/tool-results/saved.json", 3))
            .unwrap()
            .expect("UTF-16 units match");
        assert_eq!(resolved, "🚀é");

        let mut resolver = SavedResultResolver::new(dir);
        let error = resolver
            .resolve(&receipt("/x/tool-results/saved.json", 2))
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
}
