//! Errors reported while reading Mistral Vibe Unified session stores.

/// Failure modes of the Unified store reader.
///
/// Variants that carry a diagnostic string embed it after a
/// `… Mistral Vibe store: ` prefix so log lines identify the store format.
#[derive(Debug, thiserror::Error)]
pub enum UnifiedError {
    #[error("Session contains no user messages")]
    NoUserMessages,
    #[error("Ephemeral Mistral Vibe store: session storage_lifetime is ephemeral")]
    Ephemeral,
    #[error(
        "Deleting Mistral Vibe store tree: {}",
        child_session_ids.join(", ")
    )]
    DeletingTree { child_session_ids: Vec<String> },
    #[error("Unsupported Mistral Vibe store: {0}")]
    Unsupported(String),
    #[error("Corrupt Mistral Vibe store: {0}")]
    Corrupt(String),
    #[error("Incomplete Mistral Vibe store: {0}")]
    Incomplete(String),
    #[error("Identity conflict in Mistral Vibe store: {0}")]
    IdentityConflict(String),
    #[error("Unstable Mistral Vibe store: {0}")]
    Unstable(String),
    #[error("Missing saved result in Mistral Vibe store: {0}")]
    MissingSavedResult(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
