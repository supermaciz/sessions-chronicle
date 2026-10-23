use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub use super::saved_results::FileStat;
use super::store::FileSample;

// Version 2 invalidates signatures published before the journal/state/identity
// fixes: unchanged stores must be revalidated, not silently skipped forever.
pub const UNIFIED_READER_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UnifiedSignature {
    pub reader_version: u32,
    pub current_sha256: String,
    pub journal: FileSample,
    pub meta: Option<FileStat>,
    pub saved_results: Vec<(String, FileStat)>,
}

impl UnifiedSignature {
    pub(crate) fn from_parse(
        store: &super::store::LoadedStore,
        saved_results: Vec<(String, FileStat)>,
    ) -> Self {
        Self {
            reader_version: UNIFIED_READER_VERSION,
            current_sha256: super::canonical::sha256_hex(&store.current_bytes),
            journal: store.journal.clone(),
            // This sample belongs to the load attempt, before fallback reads.
            // Sampling here could acknowledge a version conversion never saw.
            meta: store.meta.clone(),
            saved_results,
        }
    }

    pub fn is_current(&self, session_dir: &Path) -> bool {
        let Ok(current) = super::store::read_current(session_dir) else {
            return false;
        };
        let digest = super::canonical::sha256_hex(&current);
        if self.reader_version != UNIFIED_READER_VERSION || digest != self.current_sha256 {
            return false;
        }
        let journal_path = session_dir.join(&self.journal.relative_path);
        if !sample_matches(&self.journal, &journal_path) {
            return false;
        }
        let meta_path = session_dir.join("meta.json");
        let meta_matches = match (&self.meta, FileStat::for_path(&meta_path)) {
            (Some(expected), Ok(actual)) => *expected == actual,
            (None, Err(error)) => error.kind() == std::io::ErrorKind::NotFound,
            _ => false,
        };
        if !meta_matches {
            return false;
        }
        self.saved_results.iter().all(|(name, expected)| {
            stat_matches(expected, &session_dir.join("tool-results").join(name))
        })
    }

    pub fn to_storage(&self) -> String {
        serde_json::to_string(self).expect("unified signature serializes")
    }

    pub fn from_storage(value: &str) -> Option<Self> {
        serde_json::from_str(value).ok()
    }
}

fn stat_matches(expected: &FileStat, path: &Path) -> bool {
    FileStat::for_path(path).is_ok_and(|actual| actual == *expected)
}

fn sample_matches(expected: &FileSample, path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    metadata.len() == expected.size
        && metadata.mtime() * 1_000_000_000 + metadata.mtime_nsec() == expected.mtime_ns
        && metadata.dev() == expected.dev
        && metadata.ino() == expected.ino
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn pre_fix_reader_signature_requires_revalidation() {
        let temp = super::super::store::test_support::copy_fixture(
            "vibe_sessions/unified/child-600cf6121578955c8405618d",
        );
        let session = temp.path().join("child-600cf6121578955c8405618d");
        let store = super::super::store::load_generation(&session).unwrap();
        let mut signature = UnifiedSignature::from_parse(&store, vec![]);
        assert!(signature.is_current(&session));
        signature.reader_version = 1;
        assert!(
            !signature.is_current(&session),
            "old accepted content must be validated under the corrected contracts"
        );
    }

    #[test]
    fn storage_round_trip_and_freshness_rejects_changed_current() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("CURRENT"), b"one").unwrap();
        fs::create_dir(dir.path().join("journal")).unwrap();
        fs::write(dir.path().join("journal/1"), b"journal").unwrap();
        let sample = FileStat::for_path(&dir.path().join("journal/1")).unwrap();
        let metadata = fs::symlink_metadata(dir.path().join("journal/1")).unwrap();
        let sig = UnifiedSignature {
            reader_version: UNIFIED_READER_VERSION,
            current_sha256: super::super::canonical::sha256_hex(b"one"),
            journal: FileSample {
                relative_path: "journal/1".into(),
                size: sample.size,
                mtime_ns: sample.mtime_ns,
                dev: metadata.dev(),
                ino: metadata.ino(),
            },
            meta: None,
            saved_results: vec![],
        };
        assert_eq!(
            UnifiedSignature::from_storage(&sig.to_storage()),
            Some(sig.clone())
        );
        assert!(sig.is_current(dir.path()));
        fs::write(dir.path().join("CURRENT"), b"two").unwrap();
        assert!(!sig.is_current(dir.path()));
    }
}
