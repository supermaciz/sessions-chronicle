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
}
