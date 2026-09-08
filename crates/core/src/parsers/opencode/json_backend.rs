use anyhow::{Context, Result};
use std::cell::RefCell;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::models::Role;
use crate::parsers::model::normalize_model;

use super::{
    MessageMetadata, OpenCodeBackend, OpenCodeReadDiagnostics, PartData, SessionEntry,
    SessionEnumeration, SessionMetadata, SessionSource, read_json, timestamp_from_millis,
};

pub struct JsonBackend {
    storage_root: PathBuf,
    read_diagnostics: RefCell<OpenCodeReadDiagnostics>,
}

impl JsonBackend {
    pub fn new(storage_root: &Path) -> Self {
        Self {
            storage_root: storage_root.to_path_buf(),
            read_diagnostics: RefCell::default(),
        }
    }

    pub(crate) fn storage_root_path(&self) -> &Path {
        &self.storage_root
    }

    fn diagnostic(&self, path: Option<PathBuf>, error: impl Into<String>) {
        let mut diagnostics = self.read_diagnostics.borrow_mut();
        diagnostics.incomplete = true;
        diagnostics.errors.push((path, error.into()));
    }

    pub(crate) fn parse_session_metadata_from_file(
        &self,
        session_path: &Path,
    ) -> Result<SessionMetadata> {
        let value = read_json(session_path).context("Failed to read session metadata")?;
        let id = value
            .get("id")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| {
                session_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(str::to_string)
            })
            .context("Session id missing")?;

        let directory = value
            .get("directory")
            .and_then(|v| v.as_str())
            .map(str::to_string);

        let parent_id = value
            .get("parentID")
            .or_else(|| value.get("parentId"))
            .and_then(|v| v.as_str())
            .map(str::to_string);

        let created_ms = value
            .get("time")
            .and_then(|v| v.get("created"))
            .and_then(|v| v.as_i64())
            .context("Session created time missing")?;

        let updated_ms = value
            .get("time")
            .and_then(|v| v.get("updated"))
            .and_then(|v| v.as_i64())
            .unwrap_or(created_ms);

        Ok(SessionMetadata {
            id,
            directory,
            title: None,
            time_created: timestamp_from_millis(created_ms)?,
            time_updated: timestamp_from_millis(updated_ms)?,
            parent_id,
        })
    }
}

impl OpenCodeBackend for JsonBackend {
    fn list_sessions(&self) -> Result<Vec<SessionEntry>> {
        let enumeration = self.enumerate_sessions()?;
        anyhow::ensure!(enumeration.complete, "OpenCode JSON enumeration incomplete");
        Ok(enumeration.entries)
    }

    fn enumerate_sessions(&self) -> Result<SessionEnumeration> {
        let sessions_dir = self.storage_root.join("session");
        if !sessions_dir.exists() {
            return Ok(SessionEnumeration {
                entries: Vec::new(),
                complete: true,
                errors: Vec::new(),
                protected_locators: HashSet::new(),
            });
        }

        let mut entries = Vec::new();
        let mut errors = Vec::new();
        let mut protected_locators = HashSet::new();
        let mut complete = true;
        for entry in walkdir::WalkDir::new(&sessions_dir).max_depth(5) {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    complete = false;
                    let path = err.path().map(Path::to_path_buf);
                    if let Some(path) = &path {
                        protected_locators.insert(path.clone());
                    }
                    errors.push((path, err.to_string()));
                    continue;
                }
            };
            let path = entry.path();
            if entry.file_type().is_file() && path.extension().is_some_and(|ext| ext == "json") {
                let id = match read_json(path) {
                    Ok(value) => value
                        .get("id")
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                        .or_else(|| {
                            path.file_stem()
                                .and_then(|s| s.to_str())
                                .map(str::to_string)
                        }),
                    Err(err) => {
                        complete = false;
                        protected_locators.insert(path.to_path_buf());
                        errors.push((Some(path.to_path_buf()), err.to_string()));
                        continue;
                    }
                };

                if let Some(id) = id {
                    entries.push(SessionEntry {
                        id,
                        source: SessionSource::JsonFile(path.to_path_buf()),
                    });
                } else {
                    complete = false;
                    protected_locators.insert(path.to_path_buf());
                    errors.push((Some(path.to_path_buf()), "Session id missing".to_string()));
                }
            }
        }

        Ok(SessionEnumeration {
            entries,
            complete,
            errors,
            protected_locators,
        })
    }

    fn take_read_diagnostics(&self) -> OpenCodeReadDiagnostics {
        std::mem::take(&mut *self.read_diagnostics.borrow_mut())
    }

    fn load_session_metadata(&self, entry: &SessionEntry) -> Result<SessionMetadata> {
        match &entry.source {
            SessionSource::JsonFile(path) => self.parse_session_metadata_from_file(path),
            _ => anyhow::bail!("JsonBackend received non-JSON session entry"),
        }
    }

    fn load_messages(&self, session_id: &str) -> Result<Vec<MessageMetadata>> {
        let messages_dir = self.storage_root.join("message").join(session_id);
        let entries = match fs::read_dir(&messages_dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                self.diagnostic(Some(messages_dir), "Message directory is missing");
                return Ok(Vec::new());
            }
            Err(err) => return Err(err).context("Failed to read messages directory"),
        };

        let mut messages = Vec::new();
        for entry in entries {
            let entry = entry.context("Failed to read message entry")?;
            if !entry
                .file_type()
                .context("Failed to read message type")?
                .is_file()
            {
                continue;
            }

            let value = match read_json(&entry.path()) {
                Ok(value) => value,
                Err(err) => {
                    self.diagnostic(Some(entry.path()), err.to_string());
                    tracing::warn!(
                        "Failed to parse message {}: {}",
                        entry.path().display(),
                        err
                    );
                    continue;
                }
            };

            let id = match value.get("id").and_then(|v| v.as_str()).map(str::to_string) {
                Some(id) => id,
                None => {
                    self.diagnostic(Some(entry.path()), "Message id missing");
                    tracing::warn!("Message id missing in {}", entry.path().display());
                    continue;
                }
            };

            let role = value.get("role").and_then(|v| v.as_str()).and_then(|role| {
                match role.to_lowercase().as_str() {
                    "user" => Some(Role::User),
                    "assistant" => Some(Role::Assistant),
                    _ => None,
                }
            });

            let created_ms = match value
                .get("time")
                .and_then(|v| v.get("created"))
                .and_then(|v| v.as_i64())
            {
                Some(ms) => ms,
                None => {
                    self.diagnostic(Some(entry.path()), "Message created time missing");
                    tracing::warn!("Message created time missing in {}", entry.path().display());
                    continue;
                }
            };

            let time_created = match timestamp_from_millis(created_ms) {
                Ok(ts) => ts,
                Err(err) => {
                    self.diagnostic(Some(entry.path()), err.to_string());
                    tracing::warn!(
                        "Invalid message timestamp in {}: {}",
                        entry.path().display(),
                        err
                    );
                    continue;
                }
            };

            let model = normalize_model(value.get("modelID"))
                .or_else(|| normalize_model(value.get("model").and_then(|m| m.get("modelID"))));

            messages.push(MessageMetadata {
                id,
                role,
                time_created,
                model,
            });
        }

        Ok(messages)
    }

    fn load_parts(&self, message_id: &str) -> Result<Vec<PartData>> {
        let parts_dir = self.storage_root.join("part").join(message_id);
        let entries = match fs::read_dir(&parts_dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                self.diagnostic(Some(parts_dir), "Part directory is missing");
                tracing::warn!("Missing parts for message {}", message_id);
                return Ok(Vec::new());
            }
            Err(err) => return Err(err).context("Failed to read parts directory"),
        };

        let mut parts = Vec::new();
        for entry in entries {
            let entry = entry.context("Failed to read part entry")?;
            if !entry
                .file_type()
                .context("Failed to read part type")?
                .is_file()
            {
                continue;
            }

            let value = match read_json(&entry.path()) {
                Ok(value) => value,
                Err(err) => {
                    self.diagnostic(Some(entry.path()), err.to_string());
                    tracing::warn!("Failed to parse part {}: {}", entry.path().display(), err);
                    continue;
                }
            };

            let id = match value.get("id").and_then(|v| v.as_str()).map(str::to_string) {
                Some(id) => id,
                None => {
                    self.diagnostic(Some(entry.path()), "Part id missing");
                    tracing::warn!("Part id missing in {}", entry.path().display());
                    continue;
                }
            };

            let kind = match value
                .get("type")
                .and_then(|v| v.as_str())
                .map(str::to_string)
            {
                Some(kind) => kind,
                None => {
                    self.diagnostic(Some(entry.path()), "Part type missing");
                    tracing::warn!("Part type missing in {}", entry.path().display());
                    continue;
                }
            };

            let order = value.get("order").and_then(|v| v.as_i64());

            parts.push(PartData {
                id,
                kind,
                order,
                raw: value,
            });
        }

        Ok(parts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_sessions_finds_json_fixtures() {
        let storage_root = crate::fixture_path("opencode_storage");
        let backend = JsonBackend::new(&storage_root);
        let sessions = backend.list_sessions().unwrap();

        assert_eq!(sessions.len(), 5);

        let ids: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&"session-001"));
        assert!(ids.contains(&"session-003"));
    }
}
