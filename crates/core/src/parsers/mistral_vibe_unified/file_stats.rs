//! Shared file-stat snapshots for freshness signatures and stability.
//!
//! Both [`FileSample`] (store documents) and [`FileStat`] (consumed
//! saved results) pin the same metadata fields and the same
//! nanosecond mtime, so the reader's freshness and stability checks
//! compare like with like.  Constructors accept `&std::fs::Metadata`
//! so a caller can pass either a `symlink_metadata` stat or the fstat
//! of a handle it actually consumed; the syscall choice stays at the
//! call site and is never hidden here.

use std::fs::{self, Metadata};
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Modification time in nanoseconds since the Unix epoch.
fn mtime_ns(metadata: &Metadata) -> i64 {
    metadata.mtime() * 1_000_000_000 + metadata.mtime_nsec()
}

/// A `symlink_metadata` snapshot of one store file, used for freshness
/// signatures and read-stability checks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileSample {
    /// Path relative to the session directory, as listed by the manifest.
    pub relative_path: String,
    pub size: u64,
    /// Modification time in nanoseconds since the Unix epoch.
    pub mtime_ns: i64,
    /// Device and inode identify the same file across pointer flips.
    pub dev: u64,
    pub ino: u64,
}

impl FileSample {
    /// Builds a sample from metadata the caller already holds — a
    /// `symlink_metadata` stat or the fstat of an open handle.
    pub fn from_metadata(relative_path: String, metadata: &Metadata) -> Self {
        Self {
            relative_path,
            size: metadata.len(),
            mtime_ns: mtime_ns(metadata),
            dev: metadata.dev(),
            ino: metadata.ino(),
        }
    }

    /// Samples the path with `symlink_metadata`: a symlink is sampled
    /// as itself, never its target.
    pub fn for_path(relative_path: String, path: &Path) -> io::Result<Self> {
        Ok(Self::from_metadata(
            relative_path,
            &fs::symlink_metadata(path)?,
        ))
    }

    /// Exact freshness comparison against metadata the caller sampled
    /// (`symlink_metadata` at the freshness call sites): size, mtime,
    /// device and inode must all be identical.  This is deliberately
    /// stricter than the growth-tolerant read-stability check, which
    /// accepts a larger size on the same dev/ino; that policy stays at
    /// its own call site and is never routed through here.
    pub fn matches_metadata(&self, metadata: &Metadata) -> bool {
        metadata.len() == self.size
            && mtime_ns(metadata) == self.mtime_ns
            && metadata.dev() == self.dev
            && metadata.ino() == self.ino
    }
}

/// A `symlink_metadata` snapshot of one consumed saved result, used for
/// freshness signatures.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileStat {
    pub size: u64,
    /// Modification time in nanoseconds since the Unix epoch.
    pub mtime_ns: i64,
}

impl FileStat {
    /// Builds a stat from metadata the caller already holds — a
    /// `symlink_metadata` stat or the fstat of the handle that produced
    /// the consumed bytes.
    pub fn from_metadata(metadata: &Metadata) -> Self {
        Self {
            size: metadata.len(),
            mtime_ns: mtime_ns(metadata),
        }
    }

    /// Samples the path with `symlink_metadata`: a symlink is sampled
    /// as itself, never its target.
    pub fn for_path(path: &Path) -> io::Result<Self> {
        Ok(Self::from_metadata(&fs::symlink_metadata(path)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn serialized_shapes_stay_pinned() {
        let sample = FileSample {
            relative_path: "journal/0000000000000002.jsonl".to_string(),
            size: 42,
            mtime_ns: 1_792_700_000_000_000_000,
            dev: 7,
            ino: 9,
        };
        assert_eq!(
            serde_json::to_value(&sample).unwrap(),
            json!({
                "relative_path": "journal/0000000000000002.jsonl",
                "size": 42,
                "mtime_ns": 1_792_700_000_000_000_000i64,
                "dev": 7,
                "ino": 9,
            })
        );
        let stat = FileStat {
            size: 42,
            mtime_ns: -1,
        };
        assert_eq!(
            serde_json::to_value(&stat).unwrap(),
            json!({"size": 42, "mtime_ns": -1})
        );
    }

    #[test]
    fn constructors_agree_on_regular_files() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("meta.json");
        std::fs::write(&path, "{}").unwrap();
        let path_metadata = std::fs::symlink_metadata(&path).unwrap();
        let handle = std::fs::File::open(&path).unwrap();

        let by_path = FileStat::for_path(&path).unwrap();
        let from_path_metadata = FileStat::from_metadata(&path_metadata);
        let from_handle_metadata = FileStat::from_metadata(&handle.metadata().unwrap());
        assert_eq!(by_path, from_path_metadata);
        assert_eq!(from_path_metadata, from_handle_metadata);
        assert_eq!(by_path.size, 2);

        let sample = FileSample::for_path("meta.json".to_string(), &path).unwrap();
        assert_eq!(sample.relative_path, "meta.json");
        assert_eq!(sample.size, 2);
        assert_eq!(
            sample,
            FileSample::from_metadata("meta.json".to_string(), &path_metadata)
        );
    }

    #[test]
    fn for_path_samples_a_symlink_itself_and_the_handle_follows_it() {
        // The deliberate symlink-vs-handle distinction: freshness
        // sampling must never follow a symlink, while a handle opened
        // through one reports the target it actually reads.
        let temp = TempDir::new().unwrap();
        let target = temp.path().join("target.json");
        std::fs::write(&target, "abc").unwrap();
        let link = temp.path().join("link.json");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let target_stat = FileStat::for_path(&target).unwrap();
        let link_stat = FileStat::for_path(&link).unwrap();
        assert_ne!(link_stat, target_stat, "for_path followed the symlink");

        let handle = std::fs::File::open(&link).unwrap();
        assert_eq!(
            FileStat::from_metadata(&handle.metadata().unwrap()),
            target_stat,
            "the handle reports the followed target"
        );
    }

    #[test]
    fn matches_metadata_is_exact_and_rejects_growth_and_replacement() {
        // Exact freshness comparison on FileSample: every pinned field
        // must be identical.  Growth (a larger size on the same inode)
        // and replacement (a different inode with identical bytes) are
        // both changes — the growth-tolerant read-stability policy is
        // a separate, deliberately different comparison that stays at
        // its own call site.
        let temp = TempDir::new().unwrap();
        let relative = "journal/0000000000000002.jsonl";
        let path = temp.path().join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "record\n").unwrap();
        let metadata = std::fs::symlink_metadata(&path).unwrap();
        let sample = FileSample::for_path(relative.to_string(), &path).unwrap();

        // Exact equality: the very stat that produced the sample matches,
        // and so does an unchanged re-read of the same file.
        assert!(sample.matches_metadata(&metadata));
        assert!(sample.matches_metadata(&std::fs::symlink_metadata(&path).unwrap()));

        // Growth after the sample is a change for freshness.
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(b"more\n").unwrap();
        drop(file);
        let grown = std::fs::symlink_metadata(&path).unwrap();
        assert_eq!(grown.ino(), metadata.ino(), "fixture premise: same inode");
        assert!(
            grown.len() > metadata.len(),
            "fixture premise: the file grew"
        );
        assert!(
            !sample.matches_metadata(&grown),
            "exact freshness must reject journal growth"
        );

        // Replacement: identical bytes — same size — under a different
        // inode is still a different file.
        let twin = temp.path().join("twin.jsonl");
        std::fs::write(&twin, "record\n").unwrap();
        let twin_metadata = std::fs::symlink_metadata(&twin).unwrap();
        assert_eq!(twin_metadata.len(), metadata.len());
        assert_ne!(twin_metadata.ino(), metadata.ino());
        assert!(
            !sample.matches_metadata(&twin_metadata),
            "a same-size replacement must not match"
        );
    }
}
