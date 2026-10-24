//! Generation loader for Mistral Vibe Unified session stores.
//!
//! This module reads one store generation end to end — the `CURRENT`
//! pointer, the manifest, the projection document with its pooled history
//! chunks, and the runtime state — and returns the snapshot projection
//! split into envelope plus entries together with a runtime view.
//! Journal replay is layered on top by the journal reader.

use std::fs;
use std::io;
use std::path::Path;

use serde_json::Value;

use super::canonical::{parse_canonical, sha256_hex};
use super::error::UnifiedError;

pub use super::file_stats::FileSample;

/// Pinned store format of the Unified session store (Vibe 3.0.x).
const STORE_FORMAT: &str = "mistral.vibe.unified-session-store/v1";
const STORE_FORMAT_MINOR: u64 = 9;
/// Pinned snapshot format of the public session state.
const PROJECTION_FORMAT: &str = "harness.public-session-state/v1";

/// Pinned fact J: a child session ID is derived from the parent session
/// ID and the spawn action ID, so a child links back to its spawn effect
/// without any stored mapping.
pub fn child_session_id(parent_session_id: &str, action_id: &str) -> String {
    let mut identity = Vec::with_capacity(parent_session_id.len() + action_id.len() + 1);
    identity.extend_from_slice(parent_session_id.as_bytes());
    identity.push(0);
    identity.extend_from_slice(action_id.as_bytes());
    format!("child-{}", &sha256_hex(&identity)[..24])
}

/// A Unified session directory lives directly inside a directory named
/// `unified`.
pub use crate::utils::session_path::is_unified_session_dir;

/// Rejects `path` when it is itself a symlink.  A missing path is fine:
/// the caller's follow-up read reports it as [`UnifiedError::Incomplete`].
/// Symlink rejection applies from the session directory downward; an
/// ancestor symlink (dotfile setups) is allowed.
pub(crate) fn reject_symlink(path: &Path) -> Result<(), UnifiedError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(UnifiedError::Corrupt(format!(
            "{} is a symlink",
            path.display()
        ))),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(UnifiedError::Io(error)),
    }
}

/// Shared pointer boundary for loading, stability and incremental freshness.
pub(crate) fn read_current(session_dir: &Path) -> Result<Vec<u8>, UnifiedError> {
    let path = session_dir.join("CURRENT");
    reject_symlink(&path)?;
    fs::read(path).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => UnifiedError::Incomplete("CURRENT missing".to_string()),
        _ => UnifiedError::Io(error),
    })
}

/// The snapshot projection of one generation, split into the envelope
/// (`snapshot` with `history.entries` emptied) and the history entries.
#[derive(Debug, Clone)]
pub(crate) struct ProjectionState {
    pub envelope: Value,
    pub entries: Vec<Value>,
    pub watermark: u64,
}

impl ProjectionState {
    /// Splits an owned snapshot into envelope plus entries without
    /// deep-copying the history: `history.entries` is moved out and the
    /// envelope keeps an empty entries placeholder, while every other
    /// envelope/history field survives verbatim.
    ///
    /// The snapshot's `format` and history shape are validated here; the
    /// caller checks the snapshot is an object first and owns that error
    /// message (`load_generation` and `apply_advanced_snapshot` word it
    /// differently).
    pub(crate) fn from_snapshot(snapshot: Value, watermark: u64) -> Result<Self, UnifiedError> {
        let format = snapshot
            .get("format")
            .and_then(Value::as_str)
            .ok_or_else(|| UnifiedError::Corrupt("snapshot format is missing".to_string()))?;
        if format != PROJECTION_FORMAT {
            return Err(UnifiedError::Unsupported(format!(
                "snapshot format {format}"
            )));
        }
        let mut envelope = snapshot;
        // The array check and the move are one operation, so the
        // history is validated and extracted without ever cloning it.
        let entries = envelope
            .get_mut("history")
            .and_then(|history| history.get_mut("entries"))
            .map(|entries| std::mem::replace(entries, Value::Array(Vec::new())))
            .and_then(|entries| match entries {
                Value::Array(entries) => Some(entries),
                _ => None,
            })
            .ok_or_else(|| {
                UnifiedError::Corrupt("snapshot history entries are not an array".to_string())
            })?;
        Ok(Self {
            envelope,
            entries,
            watermark,
        })
    }
}

/// The identity a runtime state declares for its session (fact D).
#[derive(Debug, Clone)]
pub(crate) enum Identity {
    Root,
    Fork,
    Subagent { parent: String, spawn_key: String },
}

/// The runtime state fields the reader consumes (fact D).
#[derive(Debug, Clone)]
pub(crate) struct RuntimeView {
    pub session_id: String,
    pub root_session_id: String,
    pub parent_session_id: Option<String>,
    pub ephemeral: bool,
    pub identity: Identity,
    pub cwd: Option<String>,
    pub active_model: Option<String>,
    pub deleting_children: Option<Vec<String>>,
}

/// One store generation, fully validated and sampled for the journal
/// reader: the raw `CURRENT` bytes, the manifest-derived sequences, a
/// `symlink_metadata` samples of the journal and optional metadata taken
/// before consumption, the runtime view and the snapshot projection.
#[derive(Debug, Clone)]
pub(crate) struct LoadedStore {
    pub current_bytes: Vec<u8>,
    pub journal_first_sequence: u64,
    pub journal: FileSample,
    /// Optional metadata sampled before conversion can consume its fallback.
    /// Keep absence too: a later appearance must invalidate this attempt.
    pub meta: Option<super::saved_results::FileStat>,
    pub runtime: RuntimeView,
    pub projection: ProjectionState,
}

/// Loads the generation that `CURRENT` points at, following the pinned
/// upstream facts A-D: canonical pointer and manifest, digest-checked
/// documents, pooled history chunks refilled into the projection, and
/// the runtime identity rules.  Missing referenced files are
/// [`UnifiedError::Incomplete`]; other IO failures are
/// [`UnifiedError::Io`].
pub(crate) fn load_generation(session_dir: &Path) -> Result<LoadedStore, UnifiedError> {
    // Symlink rejection starts at the session directory: ancestors may be
    // symlinks, nothing inside the session directory may be.
    reject_symlink(session_dir)?;

    let current_bytes = read_current(session_dir)?;
    if current_bytes.last() != Some(&b'\n') {
        return Err(UnifiedError::Corrupt(
            "CURRENT is missing its final newline".to_string(),
        ));
    }
    let pointer = parse_canonical(&current_bytes[..current_bytes.len() - 1])?;

    // Fact A: the store format gates every other field.
    let store_format = field_str(&pointer, "CURRENT", "store_format")?;
    if store_format != STORE_FORMAT {
        return Err(UnifiedError::Unsupported(format!(
            "store_format {store_format}"
        )));
    }
    let store_format_minor = field_u64(&pointer, "CURRENT", "store_format_minor")?;
    if store_format_minor != STORE_FORMAT_MINOR {
        return Err(UnifiedError::Unsupported(format!(
            "store_format_minor {store_format_minor}"
        )));
    }
    let generation = field_str(&pointer, "CURRENT", "generation")?;
    if !is_sixteen_digits(generation) {
        return Err(UnifiedError::Corrupt(format!(
            "CURRENT generation {generation:?} is not 16 digits"
        )));
    }
    let session_id = field_str(&pointer, "CURRENT", "session_id")?;
    let snapshot_sequence = field_u64(&pointer, "CURRENT", "snapshot_sequence")?;
    let manifest_sha256 = field_str(&pointer, "CURRENT", "manifest_sha256")?;
    if !is_sha256_hex(manifest_sha256) {
        return Err(UnifiedError::Corrupt(
            "CURRENT manifest_sha256 is not a SHA-256 digest".to_string(),
        ));
    }
    let directory_name = session_dir
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            UnifiedError::Corrupt(
                "session directory name is missing or not valid UTF-8".to_string(),
            )
        })?;
    if directory_name != session_id {
        return Err(UnifiedError::IdentityConflict(format!(
            "directory name {directory_name} does not match session_id {session_id}"
        )));
    }

    let generations_dir = session_dir.join("generations");
    reject_symlink(&generations_dir)?;
    let generation_dir = generations_dir.join(generation);
    reject_symlink(&generation_dir)?;

    // Manifest (fact B).
    let manifest_body = read_body(&generation_dir.join("manifest.json"), "manifest.json")?;
    if sha256_hex(&manifest_body) != manifest_sha256 {
        return Err(UnifiedError::Corrupt(
            "manifest does not match CURRENT manifest_sha256".to_string(),
        ));
    }
    let manifest = parse_canonical(&manifest_body)?;
    let manifest_version = field_u64(&manifest, "manifest", "manifest_version")?;
    if manifest_version != 1 {
        return Err(UnifiedError::Unsupported(format!(
            "manifest_version {manifest_version}"
        )));
    }
    if field_str(&manifest, "manifest", "session_id")? != session_id {
        return Err(UnifiedError::IdentityConflict(
            "manifest session_id does not match CURRENT".to_string(),
        ));
    }
    if field_str(&manifest, "manifest", "generation")? != generation {
        return Err(UnifiedError::IdentityConflict(
            "manifest generation does not match CURRENT".to_string(),
        ));
    }
    if field_u64(&manifest, "manifest", "snapshot_sequence")? != snapshot_sequence {
        return Err(UnifiedError::IdentityConflict(
            "manifest snapshot_sequence does not match CURRENT".to_string(),
        ));
    }
    let projection_descriptor = stored_file(&manifest, "projection_state")?;
    let runtime_descriptor = stored_file(&manifest, "runtime_state")?;
    let (journal_relative, journal_first_sequence) =
        recovery_segment(&manifest, snapshot_sequence)?;

    // Projection document (fact C): digest over the raw body, no
    // re-canonicalization.
    let projection_body = read_body(
        &generation_dir.join(&projection_descriptor.path),
        "projection-state.json",
    )?;
    if sha256_hex(&projection_body) != projection_descriptor.sha256 {
        return Err(UnifiedError::Corrupt(
            "projection-state.json does not match the manifest digest".to_string(),
        ));
    }
    let mut projection_document = parse_document(&projection_body, "projection-state.json")?;
    let projection_state_version = field_u64(
        &projection_document,
        "projection-state.json",
        "projection_state_version",
    )?;
    if projection_state_version != 1 {
        return Err(UnifiedError::Unsupported(format!(
            "projection_state_version {projection_state_version}"
        )));
    }
    // Fact C: the projection document carries its own identity and
    // sequence; both must match the pointer.  Malformed or missing fields
    // are corrupt, mismatches are identity conflicts.
    if field_str(&projection_document, "projection-state.json", "session_id")? != session_id {
        return Err(UnifiedError::IdentityConflict(
            "projection-state.json session_id does not match CURRENT".to_string(),
        ));
    }
    if field_u64(
        &projection_document,
        "projection-state.json",
        "snapshot_sequence",
    )? != snapshot_sequence
    {
        return Err(UnifiedError::IdentityConflict(
            "projection-state.json snapshot_sequence does not match CURRENT".to_string(),
        ));
    }
    let watermark = field_u64(&projection_document, "projection-state.json", "watermark")?;
    // The snapshot is consumed by value: its history entries move into
    // the projection state instead of being deep-copied for validation.
    let snapshot = projection_document
        .get_mut("snapshot")
        .map(Value::take)
        .unwrap_or(Value::Null);
    if !snapshot.is_object() {
        return Err(UnifiedError::Corrupt(
            "projection snapshot is not an object".to_string(),
        ));
    }
    let mut projection = ProjectionState::from_snapshot(snapshot, watermark)?;

    // Runtime document (fact C): read whole and digest-checked; its pooled
    // core_capabilities chunk is not consumed and not read.
    let runtime_body = read_body(
        &generation_dir.join(&runtime_descriptor.path),
        "runtime-state.json",
    )?;
    if sha256_hex(&runtime_body) != runtime_descriptor.sha256 {
        return Err(UnifiedError::Corrupt(
            "runtime-state.json does not match the manifest digest".to_string(),
        ));
    }
    let runtime_document = parse_document(&runtime_body, "runtime-state.json")?;
    let runtime = runtime_view(&runtime_document, directory_name)?;
    validate_envelope(&projection.envelope, &runtime)?;

    // Pooled history chunks (fact C): a chunked snapshot stores its
    // entries empty and refills them by concatenating the chunk arrays.
    if let Some(digests) = &projection_descriptor.chunks {
        if !projection.entries.is_empty() {
            return Err(UnifiedError::Corrupt(
                "chunked projection snapshot must store empty history entries".to_string(),
            ));
        }
        projection.entries = read_chunks(session_dir, digests)?;
    }

    // Journal segment sample, taken with symlink_metadata right before
    // returning so the journal reader can bound its later read.
    let journal_dir = session_dir.join("journal");
    reject_symlink(&journal_dir)?;
    let journal_path = session_dir.join(&journal_relative);
    reject_symlink(&journal_path)?;
    let journal_metadata =
        fs::symlink_metadata(&journal_path).map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => {
                UnifiedError::Incomplete(format!("journal segment {journal_relative} is missing"))
            }
            _ => UnifiedError::Io(error),
        })?;
    let journal = FileSample::from_metadata(journal_relative, &journal_metadata);

    Ok(LoadedStore {
        current_bytes,
        journal_first_sequence,
        journal,
        meta: super::saved_results::FileStat::for_path(&session_dir.join("meta.json")).ok(),
        runtime,
        projection,
    })
}

/// Reads one stored document: rejects symlinks, reads the raw bytes
/// (missing file is `Incomplete`, other IO failures pass through) and
/// strips exactly one required final newline.
fn read_body(path: &Path, what: &str) -> Result<Vec<u8>, UnifiedError> {
    reject_symlink(path)?;
    let raw = fs::read(path).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => UnifiedError::Incomplete(format!("{what} is missing")),
        _ => UnifiedError::Io(error),
    })?;
    if raw.last() != Some(&b'\n') {
        return Err(UnifiedError::Corrupt(format!(
            "{what} is missing its final newline"
        )));
    }
    Ok(raw[..raw.len() - 1].to_vec())
}

/// Parses a document body without re-canonicalizing it (fact C).
fn parse_document(body: &[u8], what: &str) -> Result<Value, UnifiedError> {
    serde_json::from_slice(body)
        .map_err(|error| UnifiedError::Corrupt(format!("{what} is not valid JSON: {error}")))
}

/// A manifest `StoredFileV1` descriptor (fact B).
struct StoredFile {
    path: String,
    sha256: String,
    chunks: Option<Vec<String>>,
}

/// Validates a manifest `StoredFileV1` descriptor: a single file name
/// `path`, a SHA-256 `sha256`, and `chunks` that is null or a list of
/// SHA-256 digests.
fn stored_file(manifest: &Value, field: &str) -> Result<StoredFile, UnifiedError> {
    let descriptor = manifest
        .get(field)
        .and_then(Value::as_object)
        .ok_or_else(|| UnifiedError::Corrupt(format!("manifest {field} is not an object")))?;
    let path = descriptor
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| UnifiedError::Corrupt(format!("manifest {field} path is missing")))?;
    if !is_single_file_name(path) {
        return Err(UnifiedError::Corrupt(format!(
            "manifest {field} path {path:?} is not a single file name"
        )));
    }
    let sha256 = descriptor
        .get("sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| UnifiedError::Corrupt(format!("manifest {field} sha256 is missing")))?;
    if !is_sha256_hex(sha256) {
        return Err(UnifiedError::Corrupt(format!(
            "manifest {field} sha256 is not a SHA-256 digest"
        )));
    }
    let chunks = match descriptor.get("chunks") {
        None | Some(Value::Null) => None,
        Some(Value::Array(items)) => {
            let mut digests = Vec::with_capacity(items.len());
            for item in items {
                let digest = item.as_str().ok_or_else(|| {
                    UnifiedError::Corrupt(format!("manifest {field} chunks must hold digests"))
                })?;
                if !is_sha256_hex(digest) {
                    return Err(UnifiedError::Corrupt(format!(
                        "manifest {field} chunk digest {digest:?} is not a SHA-256 digest"
                    )));
                }
                digests.push(digest.to_string());
            }
            Some(digests)
        }
        Some(_) => {
            return Err(UnifiedError::Corrupt(format!(
                "manifest {field} chunks must be null or an array"
            )));
        }
    };
    Ok(StoredFile {
        path: path.to_string(),
        sha256: sha256.to_string(),
        chunks,
    })
}

/// Validates the manifest `recovery_journal_segment` (fact B): the path
/// matches `journal/<16 digits>.jsonl` and `first_sequence` follows the
/// snapshot sequence.
fn recovery_segment(
    manifest: &Value,
    snapshot_sequence: u64,
) -> Result<(String, u64), UnifiedError> {
    let segment = manifest
        .get("recovery_journal_segment")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            UnifiedError::Corrupt("manifest recovery_journal_segment is not an object".to_string())
        })?;
    let path = segment.get("path").and_then(Value::as_str).ok_or_else(|| {
        UnifiedError::Corrupt("manifest recovery_journal_segment path is missing".to_string())
    })?;
    if !is_journal_segment_path(path) {
        return Err(UnifiedError::Corrupt(format!(
            "recovery journal path {path:?} is not a segment path"
        )));
    }
    let first_sequence = segment
        .get("first_sequence")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            UnifiedError::Corrupt(
                "manifest recovery_journal_segment first_sequence is not an integer".to_string(),
            )
        })?;
    // Untrusted sequence data: guard the next-sequence arithmetic even
    // though canonical pointer documents already bound integers to the
    // canonical safe range.
    let next_sequence = snapshot_sequence.checked_add(1).ok_or_else(|| {
        UnifiedError::Corrupt(format!(
            "snapshot_sequence {snapshot_sequence} has no representable next sequence"
        ))
    })?;
    if first_sequence != next_sequence {
        return Err(UnifiedError::Corrupt(format!(
            "recovery journal first_sequence {first_sequence} does not follow snapshot_sequence {snapshot_sequence}"
        )));
    }
    Ok((path.to_string(), first_sequence))
}

/// Reads the pooled history chunks in order, digest-checking each body
/// after removing its final newline (fact C).
fn read_chunks(session_dir: &Path, digests: &[String]) -> Result<Vec<Value>, UnifiedError> {
    let chunks_dir = session_dir.join("chunks");
    reject_symlink(&chunks_dir)?;
    let mut entries = Vec::new();
    for digest in digests {
        if !is_sha256_hex(digest) {
            return Err(UnifiedError::Corrupt(format!(
                "chunk digest {digest:?} is not a SHA-256 digest"
            )));
        }
        let chunk_path = chunks_dir.join(format!("{digest}.json"));
        let body = read_body(&chunk_path, &format!("chunk {digest}"))?;
        if sha256_hex(&body) != digest.as_str() {
            return Err(UnifiedError::Corrupt(format!(
                "chunk {digest} does not match its digest"
            )));
        }
        let chunk = parse_document(&body, &format!("chunk {digest}"))?;
        match chunk {
            Value::Array(chunk_entries) => entries.extend(chunk_entries),
            _ => {
                return Err(UnifiedError::Corrupt(format!(
                    "chunk {digest} is not a JSON array"
                )));
            }
        }
    }
    Ok(entries)
}

/// Builds the runtime view (fact D): the version gates everything else,
/// then the session's own IDs must match the directory name and the
/// identity rules must hold.
fn runtime_view(document: &Value, directory_name: &str) -> Result<RuntimeView, UnifiedError> {
    let version = field_u64(document, "runtime state", "runtime_state_version")?;
    if version != 3 {
        return Err(UnifiedError::Unsupported(format!(
            "runtime_state_version {version}"
        )));
    }
    let session_id = field_str(document, "runtime state", "session_id")?;
    if session_id != directory_name {
        return Err(UnifiedError::IdentityConflict(format!(
            "runtime session_id {session_id} does not match directory {directory_name}"
        )));
    }
    let metadata = document
        .get("session_metadata")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            UnifiedError::Corrupt("runtime session_metadata is not an object".to_string())
        })?;
    let identity = runtime_identity(document, metadata, directory_name)?;
    // Validate provenance without retaining lineage: migrated stores are
    // independent sessions, not aliases for their legacy source.
    imported_legacy_session(document)?;
    Ok(RuntimeView {
        session_id: session_id.to_string(),
        root_session_id: field_str(&document["identity"], "identity", "root_session_id")?
            .to_string(),
        parent_session_id: nullable_identity(&document["identity"], "parent_session_id")?
            .map(str::to_string),
        ephemeral: ephemeral_lifetime(document)?,
        identity,
        cwd: metadata
            .get("cwd")
            .and_then(Value::as_str)
            .map(str::to_string),
        active_model: metadata
            .get("active_model")
            .and_then(Value::as_str)
            .map(str::to_string),
        deleting_children: lifecycle_children(document)?,
    })
}

fn nullable_identity<'a>(object: &'a Value, field: &str) -> Result<Option<&'a str>, UnifiedError> {
    match object.get(field) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        _ => Err(UnifiedError::Corrupt(format!(
            "{field} is missing or malformed"
        ))),
    }
}

/// Every selected/replaced snapshot must describe the loaded runtime identity.
/// Fork lineage is compared verbatim, not forced to the fork's own ID.
pub(crate) fn validate_envelope(
    envelope: &Value,
    runtime: &RuntimeView,
) -> Result<(), UnifiedError> {
    let format = field_str(envelope, "snapshot", "format")?;
    if format != PROJECTION_FORMAT {
        return Err(UnifiedError::Unsupported(format!(
            "snapshot format {format}"
        )));
    }
    let session = envelope
        .get("session")
        .ok_or_else(|| UnifiedError::Corrupt("snapshot session missing".into()))?;
    let id = field_str(session, "snapshot session", "id")?;
    let root = field_str(session, "snapshot session", "rootSessionId")?;
    let parent = nullable_identity(session, "parentSessionId")?;
    if id != runtime.session_id
        || root != runtime.root_session_id
        || parent != runtime.parent_session_id.as_deref()
    {
        return Err(UnifiedError::IdentityConflict(
            "snapshot session identity/lineage does not match runtime".into(),
        ));
    }
    Ok(())
}

/// Validates the runtime identity rules (fact D) and returns the parsed
/// [`Identity`].  The session's own session IDs must equal the directory
/// name; structural violations (unknown kind, wrong depth, missing
/// parent or spawn key, metadata disagreeing with the identity) are
/// `Corrupt`.
fn runtime_identity(
    document: &Value,
    metadata: &serde_json::Map<String, Value>,
    directory_name: &str,
) -> Result<Identity, UnifiedError> {
    let identity = document
        .get("identity")
        .and_then(Value::as_object)
        .ok_or_else(|| UnifiedError::Corrupt("runtime identity is not an object".to_string()))?;
    let kind = identity
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| UnifiedError::Corrupt("identity kind is missing".to_string()))?;
    let identity_session_id = identity
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| UnifiedError::Corrupt("identity session_id is missing".to_string()))?;
    if identity_session_id != directory_name {
        return Err(UnifiedError::IdentityConflict(format!(
            "identity session_id {identity_session_id} does not match directory {directory_name}"
        )));
    }
    let root_session_id = identity
        .get("root_session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| UnifiedError::Corrupt("identity root_session_id is missing".to_string()))?;
    let parent = identity.get("parent_session_id");
    let parent_id = parent.and_then(Value::as_str);
    let depth = identity
        .get("depth")
        .and_then(Value::as_u64)
        .ok_or_else(|| UnifiedError::Corrupt("identity depth is not an integer".to_string()))?;

    // session_metadata root/parent must agree with the identity (fact D).
    let metadata_root = metadata
        .get("root_session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            UnifiedError::Corrupt("session_metadata root_session_id is missing".to_string())
        })?;
    if metadata_root != root_session_id {
        return Err(UnifiedError::IdentityConflict(
            "session_metadata root_session_id does not match identity".to_string(),
        ));
    }
    let metadata_parent = match metadata.get("parent_session_id") {
        Some(Value::Null) => None,
        Some(Value::String(parent)) => Some(parent.as_str()),
        _ => {
            return Err(UnifiedError::Corrupt(
                "session_metadata parent_session_id is missing or malformed".into(),
            ));
        }
    };
    if metadata_parent != parent_id {
        return Err(UnifiedError::IdentityConflict(
            "session_metadata parent_session_id does not match identity".to_string(),
        ));
    }

    match kind {
        "root" => {
            if root_session_id != directory_name {
                return Err(UnifiedError::IdentityConflict(format!(
                    "root identity root_session_id {root_session_id} does not match directory {directory_name}"
                )));
            }
            if !matches!(parent, None | Some(Value::Null)) {
                return Err(UnifiedError::Corrupt(
                    "root identity must have a null parent_session_id".to_string(),
                ));
            }
            Ok(Identity::Root)
        }
        "fork" => {
            let parent_id = parent_id.ok_or_else(|| {
                UnifiedError::Corrupt("fork identity requires a parent_session_id".to_string())
            })?;
            if parent_id == identity_session_id {
                return Err(UnifiedError::Corrupt(
                    "fork identity parent_session_id must differ from session_id".to_string(),
                ));
            }
            Ok(Identity::Fork)
        }
        "subagent" => {
            if depth != 1 {
                return Err(UnifiedError::Corrupt(format!(
                    "subagent identity depth must be 1, found {depth}"
                )));
            }
            let parent_id = parent_id.ok_or_else(|| {
                UnifiedError::Corrupt("subagent identity requires a parent_session_id".to_string())
            })?;
            if parent_id != root_session_id {
                return Err(UnifiedError::Corrupt(
                    "subagent parent_session_id must equal root_session_id".to_string(),
                ));
            }
            if identity_session_id == root_session_id || identity_session_id == parent_id {
                return Err(UnifiedError::Corrupt(
                    "subagent session_id must differ from its root and parent".to_string(),
                ));
            }
            let spawn_key = metadata
                .get("subagent_spawn_key")
                .and_then(Value::as_str)
                .filter(|key| !key.is_empty())
                .ok_or_else(|| {
                    UnifiedError::Corrupt(
                        "subagent identity requires a subagent_spawn_key".to_string(),
                    )
                })?;
            Ok(Identity::Subagent {
                parent: parent_id.to_string(),
                spawn_key: spawn_key.to_string(),
            })
        }
        other => Err(UnifiedError::Corrupt(format!(
            "unknown identity kind {other:?}"
        ))),
    }
}

/// `storage_lifetime`: `persistent` by default, `ephemeral` opts the
/// session out of indexing (fact D).
fn ephemeral_lifetime(document: &Value) -> Result<bool, UnifiedError> {
    match document.get("storage_lifetime") {
        None | Some(Value::Null) => Ok(false),
        Some(Value::String(lifetime)) if lifetime == "persistent" => Ok(false),
        Some(Value::String(lifetime)) if lifetime == "ephemeral" => Ok(true),
        Some(_) => Err(UnifiedError::Corrupt(
            "storage_lifetime must be persistent or ephemeral".to_string(),
        )),
    }
}

/// `lifecycle` (fact D): `active` by default, `deleting_tree` carries the
/// ordered child session IDs being deleted.
fn lifecycle_children(document: &Value) -> Result<Option<Vec<String>>, UnifiedError> {
    let lifecycle = match document.get("lifecycle") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Object(lifecycle)) => lifecycle,
        Some(_) => {
            return Err(UnifiedError::Corrupt(
                "lifecycle is not an object".to_string(),
            ));
        }
    };
    match lifecycle.get("type").and_then(Value::as_str) {
        Some("active") => Ok(None),
        Some("deleting_tree") => {
            let ids = lifecycle
                .get("ordered_child_session_ids")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    UnifiedError::Corrupt(
                        "deleting_tree lifecycle requires ordered_child_session_ids".to_string(),
                    )
                })?;
            let children = ids
                .iter()
                .map(|child| {
                    child.as_str().map(str::to_string).ok_or_else(|| {
                        UnifiedError::Corrupt(
                            "ordered_child_session_ids must be strings".to_string(),
                        )
                    })
                })
                .collect::<Result<Vec<String>, UnifiedError>>()?;
            Ok(Some(children))
        }
        Some(lifecycle_type) => Err(UnifiedError::Corrupt(format!(
            "unknown lifecycle type {lifecycle_type:?}"
        ))),
        None => Err(UnifiedError::Corrupt(
            "lifecycle type is missing".to_string(),
        )),
    }
}

/// `import_provenance.source.backend` (fact D): `unified` (or none) is
/// native, `legacy` points at the imported legacy session.
fn imported_legacy_session(document: &Value) -> Result<Option<String>, UnifiedError> {
    let provenance = match document.get("import_provenance") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Object(provenance)) => provenance,
        Some(_) => {
            return Err(UnifiedError::Corrupt(
                "import_provenance is not an object".to_string(),
            ));
        }
    };
    let source = provenance
        .get("source")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            UnifiedError::Corrupt("import_provenance source is not an object".to_string())
        })?;
    match source.get("backend").and_then(Value::as_str) {
        Some("unified") => Ok(None),
        Some("legacy") => {
            let legacy_session_id = source
                .get("session_id")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    UnifiedError::Corrupt(
                        "legacy import_provenance requires source.session_id".to_string(),
                    )
                })?;
            Ok(Some(legacy_session_id.to_string()))
        }
        Some(backend) => Err(UnifiedError::Corrupt(format!(
            "unknown import_provenance backend {backend:?}"
        ))),
        None => Err(UnifiedError::Corrupt(
            "import_provenance source backend is missing".to_string(),
        )),
    }
}

fn field_str<'a>(document: &'a Value, what: &str, field: &str) -> Result<&'a str, UnifiedError> {
    document
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| UnifiedError::Corrupt(format!("{what} {field} is not a string")))
}

fn field_u64(document: &Value, what: &str, field: &str) -> Result<u64, UnifiedError> {
    document
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| UnifiedError::Corrupt(format!("{what} {field} is not an integer")))
}

fn is_sixteen_digits(value: &str) -> bool {
    value.len() == 16 && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_single_file_name(path: &str) -> bool {
    !path.is_empty() && path != "." && path != ".." && !path.contains('/') && !path.contains('\\')
}

fn is_journal_segment_path(path: &str) -> bool {
    let prefix = "journal/";
    let suffix = ".jsonl";
    path.len() == prefix.len() + 16 + suffix.len()
        && path.starts_with(prefix)
        && path.ends_with(suffix)
        && path[prefix.len()..path.len() - suffix.len()]
            .bytes()
            .all(|byte| byte.is_ascii_digit())
}

/// Test-only helpers shared by the Unified reader test modules: fixture
/// copies into temp directories and canonical rewrites of generation
/// documents and journal records for negative tests.  Valid fixtures are
/// always produced by the generator; these helpers only mutate copies.
#[cfg(test)]
pub(crate) mod test_support {
    use std::fs;
    use std::path::{Path, PathBuf};

    use serde_json::{Value, json};
    use tempfile::TempDir;

    use super::{FileSample, Identity, LoadedStore, ProjectionState, RuntimeView};
    use crate::parsers::ParsedSession;
    use crate::parsers::mistral_vibe_unified::canonical::{canonical_json, sha256_hex};
    use crate::parsers::mistral_vibe_unified::convert::convert;
    use crate::parsers::mistral_vibe_unified::effects::EffectSink;
    use crate::parsers::mistral_vibe_unified::saved_results::SavedResultResolver;
    use crate::parsers::mistral_vibe_unified::{
        MistralVibeUnifiedParser, UnifiedError, UnifiedParse,
    };

    /// Recursively copies a directory tree (like the Kimi indexer tests'
    /// `copy_dir`).
    pub(crate) fn copy_tree(source: &Path, destination: &Path) {
        fs::create_dir_all(destination).unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let target = destination.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_tree(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), target).unwrap();
            }
        }
    }

    /// Copies `tests/fixtures/<rel>` into a fresh temp directory, keeping
    /// the fixture's own directory name (`temp/<basename>`), so a session
    /// store copy keeps the directory name its session ID requires.
    pub(crate) fn copy_fixture(rel: &str) -> TempDir {
        let temp = TempDir::new().expect("create temp dir");
        let source = crate::fixture_path(rel);
        assert!(source.is_dir(), "fixture {rel} is missing");
        let destination = temp.path().join(
            source
                .file_name()
                .unwrap_or_else(|| panic!("fixture path {rel} has no file name")),
        );
        copy_tree(&source, &destination);
        temp
    }

    /// Rewrites a generation document (`projection-state.json`,
    /// `runtime-state.json`, `checkpoint.json`) with `edit`, re-encodes it
    /// canonically and updates the manifest descriptor digest plus
    /// `CURRENT.manifest_sha256`.  Chunked projection documents see their
    /// history entries refilled from the pooled chunks first and pool them
    /// back afterwards, so metadata edits never lose the chunked entries.
    pub(crate) fn rewrite_document(
        session_dir: &Path,
        file_name: &str,
        edit: impl FnOnce(&mut Value),
    ) {
        let descriptor_key = match file_name {
            "projection-state.json" => "projection_state",
            "runtime-state.json" => "runtime_state",
            "checkpoint.json" => "checkpoint",
            other => panic!("rewrite_document: unsupported generation document {other}"),
        };
        let mut manifest = read_document_at(session_dir, "manifest.json");
        let mut document = read_document_at(session_dir, file_name);

        let chunk_digests: Option<Vec<String>> =
            manifest[descriptor_key]["chunks"].as_array().map(|items| {
                items
                    .iter()
                    .map(|item| item.as_str().expect("chunk digest").to_string())
                    .collect()
            });
        let mut original_entries: Option<Vec<Value>> = None;
        if chunk_digests.is_some() && file_name == "projection-state.json" {
            let mut entries: Vec<Value> = Vec::new();
            for digest in chunk_digests.as_ref().expect("chunk digests") {
                let path = session_dir.join("chunks").join(format!("{digest}.json"));
                let chunk: Value =
                    serde_json::from_slice(&read_stored(&path)).expect("chunk body is valid JSON");
                entries.extend(
                    chunk
                        .as_array()
                        .expect("chunk body is a JSON array")
                        .iter()
                        .cloned(),
                );
            }
            document["snapshot"]["history"]["entries"] = Value::Array(entries.clone());
            original_entries = Some(entries);
        }

        edit(&mut document);

        let mut pooled_digests = chunk_digests.clone();
        if let Some(original) = original_entries {
            let edited = document["snapshot"]["history"]["entries"]
                .as_array()
                .expect("history entries")
                .clone();
            if edited != original {
                // Edited entries are pooled as one new chunk so no entry is
                // lost; unreferenced old chunk files are simply ignored.
                let body = canonical_json(&Value::Array(edited)).expect("canonicalize entries");
                let digest = sha256_hex(&body);
                let path = session_dir.join("chunks").join(format!("{digest}.json"));
                let mut stored = body;
                stored.push(b'\n');
                fs::write(&path, stored).expect("write chunk");
                pooled_digests = Some(vec![digest]);
            }
            document["snapshot"]["history"]["entries"] = Value::Array(Vec::new());
        }

        let body = canonical_json(&document).expect("canonicalize document");
        let mut stored = body.clone();
        stored.push(b'\n');
        fs::write(generation_dir(session_dir).join(file_name), stored).expect("write document");

        manifest[descriptor_key]["sha256"] = Value::String(sha256_hex(&body));
        manifest[descriptor_key]["chunks"] = match &pooled_digests {
            Some(digests) => Value::Array(
                digests
                    .iter()
                    .map(|digest| Value::String(digest.clone()))
                    .collect(),
            ),
            None => Value::Null,
        };
        refresh_manifest_and_pointer(session_dir, &manifest);
    }

    /// Rewrites the journal record with the given `sequence`: applies
    /// `edit`, re-encodes the record canonically and recomputes its
    /// `record_sha256` per the upstream digest rule.  Successor records
    /// keep chaining to the previous digest; fixing the chain is up to the
    /// test.
    pub(crate) fn rewrite_journal_record(
        session_dir: &Path,
        sequence: u64,
        edit: impl FnOnce(&mut Value),
    ) {
        let manifest = read_document_at(session_dir, "manifest.json");
        let journal_relative = manifest["recovery_journal_segment"]["path"]
            .as_str()
            .expect("manifest recovery_journal_segment.path")
            .to_string();
        let journal_path = session_dir.join(&journal_relative);
        let data = fs::read(&journal_path)
            .unwrap_or_else(|error| panic!("read {journal_relative}: {error}"));

        let mut output: Vec<u8> = Vec::with_capacity(data.len());
        let mut rest = data.as_slice();
        let mut edit = Some(edit);
        let mut found = false;
        while let Some(position) = rest.iter().position(|&byte| byte == b'\n') {
            let (line, tail) = rest.split_at(position);
            rest = &tail[1..];
            let mut record: Value =
                serde_json::from_slice(line).expect("journal record is valid JSON");
            if record["sequence"].as_u64() == Some(sequence) {
                let edit = edit.take().expect("journal record sequence matches twice");
                edit(&mut record);
                output.extend_from_slice(&encode_record_line(&record));
                found = true;
            } else {
                output.extend_from_slice(line);
            }
            output.push(b'\n');
        }
        output.extend_from_slice(rest); // Unterminated tail fragment is preserved.
        assert!(
            found,
            "journal record {sequence} not found in {journal_relative}"
        );
        fs::write(&journal_path, &output).expect("write journal");
    }

    /// Re-encodes one journal record per the upstream rule: the digest is
    /// computed before inserting the `record_sha256` field.
    fn encode_record_line(record: &Value) -> Vec<u8> {
        let mut record = record.clone();
        record.as_object_mut().unwrap().remove("record_sha256");
        let line = canonical_json(&record).expect("canonicalize record");
        record["record_sha256"] = Value::String(sha256_hex(&line));
        canonical_json(&record).expect("canonicalize record with digest")
    }

    fn read_document_at(session_dir: &Path, file_name: &str) -> Value {
        let path = generation_dir(session_dir).join(file_name);
        let raw =
            fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        let body = strip_final_newline(&raw, file_name);
        serde_json::from_slice(body).unwrap_or_else(|error| panic!("parse {file_name}: {error}"))
    }

    fn refresh_manifest_and_pointer(session_dir: &Path, manifest: &Value) {
        let manifest_body = canonical_json(manifest).expect("canonicalize manifest");
        let mut stored = manifest_body.clone();
        stored.push(b'\n');
        fs::write(generation_dir(session_dir).join("manifest.json"), stored)
            .expect("write manifest");

        let mut pointer = pointer_value(session_dir);
        pointer["manifest_sha256"] = Value::String(sha256_hex(&manifest_body));
        write_pointer(session_dir, &pointer);
    }

    fn write_pointer(session_dir: &Path, pointer: &Value) {
        let mut body = canonical_json(pointer).expect("canonicalize CURRENT");
        body.push(b'\n');
        fs::write(session_dir.join("CURRENT"), body).expect("write CURRENT");
    }

    fn pointer_value(session_dir: &Path) -> Value {
        let raw = fs::read(session_dir.join("CURRENT")).expect("read CURRENT");
        serde_json::from_slice(strip_final_newline(&raw, "CURRENT")).expect("parse CURRENT")
    }

    fn generation_dir(session_dir: &Path) -> PathBuf {
        let generation = pointer_value(session_dir)["generation"]
            .as_str()
            .expect("CURRENT.generation")
            .to_string();
        session_dir.join("generations").join(generation)
    }

    fn read_stored(path: &Path) -> Vec<u8> {
        let raw = fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        strip_final_newline(&raw, &path.file_name().unwrap().to_string_lossy()).to_vec()
    }

    fn strip_final_newline<'a>(raw: &'a [u8], what: &str) -> &'a [u8] {
        assert_eq!(raw.last(), Some(&b'\n'), "{what} must end with a newline");
        &raw[..raw.len() - 1]
    }

    /// Stable variant name, so tests can assert on the failure class
    /// without matching on payloads.
    pub(crate) fn error_variant(error: &UnifiedError) -> &'static str {
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

    pub(crate) fn unified_rel(id: &str) -> String {
        format!("vibe_sessions/unified/{id}")
    }

    pub(crate) fn variant_rel(name: &str, id: &str) -> String {
        format!("vibe_unified_variants/{name}/unified/{id}")
    }

    pub(crate) fn parse_fixture(rel: &str) -> Result<UnifiedParse, UnifiedError> {
        MistralVibeUnifiedParser.parse(&crate::fixture_path(rel))
    }

    /// Copies a Unified fixture store into a temp directory, keeping the
    /// session directory name the store's identity requires.
    pub(crate) fn copy_unified(id: &str) -> (TempDir, PathBuf) {
        let temp = copy_fixture(&unified_rel(id));
        let session = temp.path().join(id);
        (temp, session)
    }

    // ------------------------------------------------------------------
    // In-memory conversion harness: entries built as JSON values, a
    // synthetic runtime whose cwd/model satisfy the metadata rules, and a
    // session directory path that is never touched on disk.
    // ------------------------------------------------------------------

    pub(crate) const T0_MS: i64 = 1_792_700_000_000;
    pub(crate) const IN_MEMORY: &str = "in-memory";

    pub(crate) fn in_memory_session_dir() -> PathBuf {
        PathBuf::from("/home/fixture/.vibe/logs/session/unified").join(IN_MEMORY)
    }

    pub(crate) fn in_memory_store(entries: Vec<Value>) -> LoadedStore {
        LoadedStore {
            current_bytes: b"{}\n".to_vec(),
            journal_first_sequence: 2,
            meta: None,
            journal: FileSample {
                relative_path: "journal/0000000000000002.jsonl".to_string(),
                size: 0,
                mtime_ns: 0,
                dev: 0,
                ino: 0,
            },
            runtime: RuntimeView {
                session_id: IN_MEMORY.to_string(),
                root_session_id: IN_MEMORY.to_string(),
                parent_session_id: None,
                ephemeral: false,
                identity: Identity::Root,
                cwd: Some("/home/fixture/projects/original".to_string()),
                active_model: Some("mistral-large-4".to_string()),
                deleting_children: None,
            },
            projection: ProjectionState {
                envelope: json!({
                    "format": "harness.public-session-state/v1",
                    "session": {
                        "id": IN_MEMORY,
                        "rootSessionId": IN_MEMORY,
                        "parentSessionId": null,
                        "title": "In memory",
                        "status": {"type": "idle"},
                        "createdAt": T0_MS,
                        "updatedAt": T0_MS,
                        "tokenUsage": {
                            "inputTokens": 1,
                            "outputTokens": 2,
                            "totalTokens": 3,
                            "cachedInputTokens": 0
                        },
                        "contextUsage": null,
                    },
                    "latestTurn": null,
                    "history": {"entries": []},
                }),
                entries,
                watermark: 1,
            },
        }
    }

    pub(crate) fn convert_entries(entries: Vec<Value>) -> Result<ParsedSession, UnifiedError> {
        let store = in_memory_store(entries);
        let session_dir = in_memory_session_dir();
        let mut results = SavedResultResolver::new(session_dir.join("tool-results"));
        let mut effects = EffectSink::new(IN_MEMORY, &mut results);
        convert(&session_dir, &store, &mut effects)
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{
        copy_fixture, copy_tree, error_variant, rewrite_document, rewrite_journal_record,
    };
    use super::*;
    use crate::parsers::mistral_vibe_unified::canonical::canonical_json;
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    // Fixture IDs (Task 2 contract; the child ID is re-derived with
    // `child_session_id` below).
    const R302: &str = "aaaaaaaa-0000-4000-8000-000000000302";
    const R300: &str = "aaaaaaaa-0000-4000-8000-000000000300";
    const F301: &str = "aaaaaaaa-0000-4000-8000-000000000301";
    const CHILD_A: &str = "child-ea9b00b3ad4061f4ca36fd81";

    fn session_rel(id: &str) -> String {
        format!("vibe_sessions/unified/{id}")
    }

    fn copy_variant(rel: &str) -> TempDir {
        copy_fixture(rel)
    }

    /// Copies a session fixture into a temp directory, keeping the temp
    /// directory alive and returning the session directory copy.
    fn copy_store(id: &str) -> (TempDir, PathBuf) {
        let temp = copy_variant(&session_rel(id));
        let session = temp.path().join(id);
        (temp, session)
    }

    fn expect_variant(result: Result<LoadedStore, UnifiedError>, expected: &'static str) {
        let error = result.expect_err("expected the load to fail");
        assert_eq!(
            error_variant(&error),
            expected,
            "load failed with the wrong variant: {error}"
        );
    }

    fn read_pointer(session_dir: &Path) -> Value {
        let raw = fs::read(session_dir.join("CURRENT")).unwrap();
        serde_json::from_slice(&raw[..raw.len() - 1]).unwrap()
    }

    fn write_pointer(session_dir: &Path, pointer: &Value) {
        let mut body = canonical_json(pointer).unwrap();
        body.push(b'\n');
        fs::write(session_dir.join("CURRENT"), body).unwrap();
    }

    fn generation_dir(session_dir: &Path) -> PathBuf {
        session_dir
            .join("generations")
            .join(read_pointer(session_dir)["generation"].as_str().unwrap())
    }

    fn read_manifest(session_dir: &Path) -> Value {
        let raw = fs::read(generation_dir(session_dir).join("manifest.json")).unwrap();
        serde_json::from_slice(&raw[..raw.len() - 1]).unwrap()
    }

    fn replace_current(session_dir: &Path, from: &str, to: &str) {
        let path = session_dir.join("CURRENT");
        let text = fs::read_to_string(&path).unwrap();
        let updated = text.replacen(from, to, 1);
        assert_ne!(&updated, &text, "CURRENT did not contain {from}");
        fs::write(&path, updated).unwrap();
    }

    #[test]
    fn loads_chunked_r302_generation() {
        let loaded = load_generation(&crate::fixture_path(&session_rel(R302))).unwrap();

        assert_eq!(loaded.journal_first_sequence, 5);
        assert_eq!(
            loaded.journal.relative_path,
            "journal/0000000000000005.jsonl"
        );
        assert!(loaded.journal.size > 0);
        assert_eq!(loaded.projection.watermark, 4);

        let ids: Vec<&str> = loaded
            .projection
            .entries
            .iter()
            .map(|entry| entry["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            [
                "e1",
                "e2",
                "effect-act-read",
                "e4",
                "e5",
                "e6",
                "e7",
                "effect-act-patch",
                "effect-act-bash",
                "effect-act-spawn-explicit",
                "effect-act-spawn-derived",
                "effect-act-spawn-missing",
                "e13",
                "e14",
                "e15",
                "e16",
                "e17",
            ]
        );
        // The pooled chunks are split back out of the stored envelope.
        assert_eq!(
            loaded.projection.envelope["history"]["entries"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            loaded.projection.envelope["session"]["id"]
                .as_str()
                .unwrap(),
            R302
        );

        assert_eq!(loaded.runtime.session_id, R302);
        assert_eq!(
            loaded.runtime.cwd.as_deref(),
            Some("/home/fixture/projects/original")
        );
        assert_eq!(
            loaded.runtime.active_model.as_deref(),
            Some("mistral-large-4")
        );
        assert!(matches!(loaded.runtime.identity, Identity::Root));
        assert!(!loaded.runtime.ephemeral);
        assert!(loaded.runtime.deleting_children.is_none());
        // Raw pointer bytes keep their trailing newline for stability checks.
        assert_eq!(loaded.current_bytes.last(), Some(&b'\n'));
    }

    #[test]
    fn loads_inline_r300_generation() {
        let loaded = load_generation(&crate::fixture_path(&session_rel(R300))).unwrap();

        assert_eq!(loaded.journal_first_sequence, 2);
        assert_eq!(
            loaded.journal.relative_path,
            "journal/0000000000000002.jsonl"
        );
        assert_eq!(loaded.projection.watermark, 1);

        let ids: Vec<&str> = loaded
            .projection
            .entries
            .iter()
            .map(|entry| entry["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["u0"]);
        assert_eq!(
            loaded.projection.envelope["history"]["entries"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            loaded.projection.entries[0]["content"][0]["text"]
                .as_str()
                .unwrap(),
            "Ancienne amorce"
        );

        assert_eq!(
            loaded.runtime.active_model.as_deref(),
            Some("mistral-medium-3")
        );
        assert!(matches!(loaded.runtime.identity, Identity::Root));
    }

    #[test]
    fn child_identity_is_subagent_with_spawn_key() {
        let loaded = load_generation(&crate::fixture_path(&session_rel(CHILD_A))).unwrap();

        assert_eq!(loaded.runtime.session_id, CHILD_A);
        assert!(matches!(
            &loaded.runtime.identity,
            Identity::Subagent { parent, spawn_key }
                if parent == R302 && spawn_key == "act-spawn-explicit"
        ));
    }

    #[test]
    fn fork_identity_is_not_a_subagent() {
        let loaded = load_generation(&crate::fixture_path(&session_rel(F301))).unwrap();

        assert!(matches!(&loaded.runtime.identity, Identity::Fork));
        assert!(!matches!(
            loaded.runtime.identity,
            Identity::Subagent { .. }
        ));
    }

    #[test]
    fn child_session_id_matches_rule_j() {
        let id = child_session_id(R302, "act-spawn-explicit");

        assert_eq!(id, CHILD_A);
        assert!(id.starts_with("child-"));
        assert_eq!(id.len(), 30);
        // The derived ID names the fixture directory Task 2 generated.
        assert!(crate::fixture_path(&session_rel(CHILD_A)).is_dir());
    }

    #[test]
    fn rejects_newer_store_format_minor() {
        let (_temp, session) = copy_store(R302);
        replace_current(
            &session,
            "\"store_format_minor\":9",
            "\"store_format_minor\":10",
        );
        expect_variant(load_generation(&session), "Unsupported");
    }

    #[test]
    fn rejects_older_store_format_minor() {
        let (_temp, session) = copy_store(R302);
        replace_current(
            &session,
            "\"store_format_minor\":9",
            "\"store_format_minor\":8",
        );
        expect_variant(load_generation(&session), "Unsupported");
    }

    #[test]
    fn rejects_current_with_inserted_space() {
        let (_temp, session) = copy_store(R302);
        replace_current(&session, "{\"", "{ \"");
        expect_variant(load_generation(&session), "Corrupt");
    }

    #[test]
    fn rejects_current_without_final_newline() {
        let (_temp, session) = copy_store(R302);
        let path = session.join("CURRENT");
        let mut bytes = fs::read(&path).unwrap();
        assert_eq!(bytes.pop(), Some(b'\n'));
        fs::write(&path, bytes).unwrap();
        expect_variant(load_generation(&session), "Corrupt");
    }

    #[test]
    fn rejects_flipped_byte_in_chunk() {
        let (_temp, session) = copy_store(R302);
        let manifest = read_manifest(&session);
        let digest = manifest["projection_state"]["chunks"][0].as_str().unwrap();
        let chunk_path = session.join("chunks").join(format!("{digest}.json"));
        let mut bytes = fs::read(&chunk_path).unwrap();
        let middle = bytes.len() / 2;
        bytes[middle] ^= 0x20;
        fs::write(&chunk_path, bytes).unwrap();
        expect_variant(load_generation(&session), "Corrupt");
    }

    #[test]
    fn rejects_deleted_chunk_file() {
        let (_temp, session) = copy_store(R302);
        let manifest = read_manifest(&session);
        let digest = manifest["projection_state"]["chunks"][0].as_str().unwrap();
        fs::remove_file(session.join("chunks").join(format!("{digest}.json"))).unwrap();
        expect_variant(load_generation(&session), "Incomplete");
    }

    #[test]
    fn rejects_edited_manifest_generation() {
        let (_temp, session) = copy_store(R302);
        let mut manifest = read_manifest(&session);
        manifest["generation"] = json!("0000000000000005");
        let body = canonical_json(&manifest).unwrap();
        let mut stored = body.clone();
        stored.push(b'\n');
        fs::write(generation_dir(&session).join("manifest.json"), stored).unwrap();
        // Keep the pointer digest consistent: only the identity conflicts.
        let mut pointer = read_pointer(&session);
        pointer["manifest_sha256"] = json!(sha256_hex(&body));
        write_pointer(&session, &pointer);
        expect_variant(load_generation(&session), "IdentityConflict");
    }

    #[test]
    fn rejects_renamed_session_directory() {
        let holder = TempDir::new().unwrap();
        let renamed = holder.path().join("aaaaaaaa-0000-4000-8000-000000000099");
        copy_tree(&crate::fixture_path(&session_rel(R302)), &renamed);
        expect_variant(load_generation(&renamed), "IdentityConflict");
    }

    #[test]
    fn rejects_unsupported_runtime_state_version() {
        let (_temp, session) = copy_store(R302);
        rewrite_document(&session, "runtime-state.json", |document| {
            document["runtime_state_version"] = json!(2);
        });
        expect_variant(load_generation(&session), "Unsupported");
    }

    #[test]
    fn rejects_symlinked_projection_document() {
        let (_temp, session) = copy_store(R302);
        let generation = generation_dir(&session);
        let document = generation.join("projection-state.json");
        fs::remove_file(&document).unwrap();
        std::os::unix::fs::symlink(generation.join("runtime-state.json"), &document).unwrap();
        expect_variant(load_generation(&session), "Corrupt");
    }

    #[test]
    fn rejects_unknown_lifecycle_type() {
        let (_temp, session) = copy_store(R302);
        rewrite_document(&session, "runtime-state.json", |document| {
            document["lifecycle"] = json!({"type": "frozen"});
        });
        expect_variant(load_generation(&session), "Corrupt");
    }

    #[test]
    fn rejects_subagent_depth_two() {
        let (_temp, session) = copy_store(CHILD_A);
        rewrite_document(&session, "runtime-state.json", |document| {
            document["identity"]["depth"] = json!(2);
        });
        expect_variant(load_generation(&session), "Corrupt");
    }

    #[test]
    fn rejects_projection_session_id_mismatch() {
        let (_temp, session) = copy_store(R302);
        rewrite_document(&session, "projection-state.json", |document| {
            document["session_id"] = json!("aaaaaaaa-0000-4000-8000-000000000300");
        });
        expect_variant(load_generation(&session), "IdentityConflict");
    }

    #[test]
    fn rejects_projection_snapshot_sequence_mismatch() {
        let (_temp, session) = copy_store(R302);
        rewrite_document(&session, "projection-state.json", |document| {
            document["snapshot_sequence"] = json!(5);
        });
        expect_variant(load_generation(&session), "IdentityConflict");
    }

    #[test]
    fn rejects_projection_missing_session_id() {
        let (_temp, session) = copy_store(R302);
        rewrite_document(&session, "projection-state.json", |document| {
            document
                .as_object_mut()
                .expect("projection document is an object")
                .remove("session_id");
        });
        expect_variant(load_generation(&session), "Corrupt");
    }

    #[test]
    fn rejects_projection_malformed_snapshot_sequence() {
        let (_temp, session) = copy_store(R302);
        rewrite_document(&session, "projection-state.json", |document| {
            document["snapshot_sequence"] = json!("four");
        });
        expect_variant(load_generation(&session), "Corrupt");
    }

    #[test]
    fn recovery_segment_rejects_unrepresentable_next_sequence() {
        let manifest = json!({
            "recovery_journal_segment": {
                "first_sequence": 0,
                "path": "journal/0000000000000000.jsonl",
            }
        });
        // Canonical CURRENT documents cannot carry u64::MAX (integers are
        // bounded to the canonical safe range), so the guard is exercised
        // at the helper level only, never through an on-disk store.
        let error =
            recovery_segment(&manifest, u64::MAX).expect_err("expected the segment check to fail");
        assert!(
            matches!(error, UnifiedError::Corrupt(_)),
            "unexpected variant: {error}"
        );
    }

    #[test]
    fn symlinked_source_root_is_indexed() {
        let holder = TempDir::new().unwrap();
        let real = holder.path().join("real");
        copy_tree(
            &crate::fixture_path(&session_rel(R302)),
            &real.join("unified").join(R302),
        );
        let link = holder.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        // Symlink rejection starts at the session directory; an ancestor
        // symlink (dotfile setups) is fine (Review Focus 2).
        let through_link = link.join("unified").join(R302);
        assert!(is_unified_session_dir(&through_link));
        assert!(!is_unified_session_dir(&real));

        let loaded = load_generation(&through_link).unwrap();
        assert_eq!(loaded.projection.entries.len(), 17);
        assert!(matches!(loaded.runtime.identity, Identity::Root));
    }

    // ------------------------------------------------------------------
    // ProjectionState::from_snapshot: ownership split and history shape
    // (review follow-up Task 2).
    // ------------------------------------------------------------------

    #[test]
    fn from_snapshot_moves_history_entries_without_cloning() {
        // A nonempty `String` is heap-backed; its buffer address survives
        // a move but not a clone. Recording the address before the
        // snapshot moves and asserting it on the resulting entry proves
        // the history was extracted, not deep-copied (no unsafe, no
        // allocation counting).
        let text = "a heap-backed history entry text that must move".to_string();
        let buffer = text.as_ptr();
        let mut entry = json!({"id": "e1"});
        entry["text"] = Value::String(text);

        let mut snapshot = json!({
            "format": "harness.public-session-state/v1",
            "session": {
                "id": "s",
                "rootSessionId": "s",
                "parentSessionId": null,
                "title": "Kept title",
                "status": {"type": "idle"},
            },
            "latestTurn": {"id": "t1", "status": "completed"},
            "history": {"cursor": 12, "entries": []},
        });
        snapshot["history"]["entries"]
            .as_array_mut()
            .expect("entries placeholder")
            .push(entry);

        let state = ProjectionState::from_snapshot(snapshot, 7).unwrap();

        assert_eq!(state.entries.len(), 1);
        let moved = state.entries[0]["text"].as_str().unwrap();
        assert_eq!(
            moved.as_ptr(),
            buffer,
            "history entries were cloned instead of moved"
        );
        assert_eq!(moved, "a heap-backed history entry text that must move");

        // The envelope keeps an empty entries placeholder, and every
        // other envelope/history field survives the split untouched.
        assert_eq!(
            state.envelope["history"]["entries"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert_eq!(state.envelope["history"]["cursor"].as_u64(), Some(12));
        assert_eq!(state.envelope["format"].as_str(), Some(PROJECTION_FORMAT));
        assert_eq!(state.envelope["session"]["id"].as_str(), Some("s"));
        assert_eq!(
            state.envelope["session"]["title"].as_str(),
            Some("Kept title")
        );
        assert_eq!(state.envelope["latestTurn"]["id"].as_str(), Some("t1"));
        assert_eq!(state.watermark, 7);
    }

    #[test]
    fn from_snapshot_rejects_missing_or_malformed_history_shape() {
        let session = json!({"id": "s", "rootSessionId": "s", "parentSessionId": null});
        let cases = [
            (
                "missing history",
                json!({"format": PROJECTION_FORMAT, "session": session}),
                "snapshot history entries are not an array",
            ),
            (
                "history is not an object",
                json!({"format": PROJECTION_FORMAT, "session": session, "history": "flat"}),
                "snapshot history entries are not an array",
            ),
            (
                "missing entries",
                json!({"format": PROJECTION_FORMAT, "session": session, "history": {}}),
                "snapshot history entries are not an array",
            ),
            (
                "entries is not an array",
                json!({
                    "format": PROJECTION_FORMAT,
                    "session": session,
                    "history": {"entries": {"id": "e1"}},
                }),
                "snapshot history entries are not an array",
            ),
        ];
        for (label, snapshot, expected) in cases {
            let error = ProjectionState::from_snapshot(snapshot, 0).expect_err(label);
            assert!(
                matches!(error, UnifiedError::Corrupt(ref message) if message == expected),
                "{label}: unexpected error {error:?}"
            );
        }
    }

    #[test]
    fn from_snapshot_rejects_missing_or_unknown_format() {
        let session = json!({"id": "s", "rootSessionId": "s", "parentSessionId": null});
        let missing = json!({"session": session, "history": {"entries": []}});
        let error = ProjectionState::from_snapshot(missing, 0).unwrap_err();
        assert!(
            matches!(error, UnifiedError::Corrupt(ref message) if message == "snapshot format is missing"),
            "unexpected error {error:?}"
        );

        let unknown = json!({
            "format": "harness.other-state/v9",
            "session": session,
            "history": {"entries": []},
        });
        let error = ProjectionState::from_snapshot(unknown, 0).unwrap_err();
        assert!(
            matches!(error, UnifiedError::Unsupported(ref message) if message == "snapshot format harness.other-state/v9"),
            "unexpected error {error:?}"
        );
    }

    #[test]
    fn rejects_chunked_snapshot_with_inline_entries() {
        // Characterization of the chunk empty-inline constraint through
        // the ownership-based split: when the manifest declares pooled
        // chunks, non-empty inline entries stay corrupt even with
        // fully consistent document digests.
        let (_temp, session) = copy_store(R302);
        let mut projection: Value = {
            let raw = fs::read(generation_dir(&session).join("projection-state.json")).unwrap();
            serde_json::from_slice(&raw[..raw.len() - 1]).unwrap()
        };
        projection["snapshot"]["history"]["entries"] = json!([{"id": "inline-e1"}]);
        let body = canonical_json(&projection).unwrap();
        let mut stored = body.clone();
        stored.push(b'\n');
        fs::write(
            generation_dir(&session).join("projection-state.json"),
            &stored,
        )
        .unwrap();

        let mut manifest = read_manifest(&session);
        manifest["projection_state"]["sha256"] = json!(sha256_hex(&body));
        let manifest_body = canonical_json(&manifest).unwrap();
        let mut manifest_stored = manifest_body.clone();
        manifest_stored.push(b'\n');
        fs::write(
            generation_dir(&session).join("manifest.json"),
            &manifest_stored,
        )
        .unwrap();

        let mut pointer = read_pointer(&session);
        pointer["manifest_sha256"] = json!(sha256_hex(&manifest_body));
        write_pointer(&session, &pointer);

        let error = load_generation(&session).expect_err("inline entries must be rejected");
        assert!(
            matches!(error, UnifiedError::Corrupt(ref message)
                if message == "chunked projection snapshot must store empty history entries"),
            "unexpected error {error:?}"
        );
    }

    #[test]
    fn rewrite_journal_record_recomputes_digest_and_leaves_chain() {
        let (_temp, session) = copy_store(R302);
        rewrite_journal_record(&session, 5, |record| {
            record["payload"]["watermark"] = json!(6);
        });

        let manifest = read_manifest(&session);
        let journal_path = session.join(
            manifest["recovery_journal_segment"]["path"]
                .as_str()
                .unwrap(),
        );
        let data = fs::read(&journal_path).unwrap();
        let mut lines = data.split(|byte| *byte == b'\n');
        let first = lines.next().unwrap();
        let second = lines.next().unwrap();

        // Upstream reader rule (fact E): omit the entire digest field.
        let marker: &[u8] = &[
            b',', b'"', b'r', b'e', b'c', b'o', b'r', b'd', b'_', b's', b'h', b'a', b'2', b'5',
            b'6', b'"', b':', b'"',
        ];
        let marker_start = first
            .windows(marker.len())
            .rposition(|window| window == marker)
            .unwrap();
        let start = marker_start + marker.len();
        let digest = sha256_hex(&[&first[..marker_start], &first[start + 65..]].concat());
        let record: Value = serde_json::from_slice(first).unwrap();
        assert_eq!(record["payload"]["watermark"].as_u64(), Some(6));
        assert_eq!(record["record_sha256"].as_str().unwrap(), digest);

        // The successor still chains to the previous digest: the helper
        // leaves the chain to the test.
        let previous: Value = serde_json::from_slice(second).unwrap();
        assert_ne!(previous["previous_record_sha256"], record["record_sha256"]);
    }
}
