//! Projection conversion: turns a replayed Mistral Vibe Unified snapshot
//! into the shared [`ParsedSession`] domain.
//!
//! The converter walks the projection entries in history order, mapping
//! messages, reasoning and checkpoints per the pinned upstream facts G-I.
//! Effect entries and skill payload blocks are routed through the effect
//! sink ([`super::effects::EffectSink`]), which maps them to tool-call,
//! saved-result and subagent rows (facts H, I, K).

use std::fs;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde_json::Value;
use tracing::{debug, warn};

use crate::models::{
    AiAssistant, Message, ReasoningAttachment, Role, Session, SessionEndingStatus, Subagent,
    TokenUsage, ToolCall, TranscriptItem, TranscriptItemKind,
};
use crate::parsers::model::normalize_model;
use crate::parsers::{ParsedSession, PendingReasoning, extract_first_prompt};

use super::effects;
use super::error::UnifiedError;
use super::saved_results::SavedResultResolver;
use super::store::{Identity, LoadedStore, child_session_id, reject_symlink};

/// Parser for Mistral Vibe Unified session stores
/// (`<vibe-root>/unified/<id>/`, Vibe 3.0.x).
pub struct MistralVibeUnifiedParser;

impl MistralVibeUnifiedParser {
    /// Loads one Unified session store and converts it.
    ///
    /// The read is guarded by the load-twice stability loop
    /// ([`super::load_stable`]); `convert` turns the validated projection
    /// into a [`ParsedSession`], routing effects through a fresh sink per
    /// load attempt.
    pub fn parse(&self, session_dir: &Path) -> Result<UnifiedParse, UnifiedError> {
        let session_id = session_dir
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                UnifiedError::Corrupt(
                    "session directory name is missing or not valid UTF-8".to_string(),
                )
            })?
            .to_string();
        let ((parsed, signature), _store) = super::load_stable(session_dir, |store| {
            // A fresh resolver per attempt: an unstable retry never
            // reuses partial consumed-file state.
            let mut results = SavedResultResolver::new(session_dir.join("tool-results"));
            let converted = {
                let mut effects = effects::EffectSink::new(&session_id, &mut results);
                convert(session_dir, store, &mut effects)
            };
            // Task 7 folds the consumed saved results into the freshness
            // signature; on failure the list is dropped with the parse.
            let consumed = results.into_consumed();
            let parsed = converted?;
            let signature = super::signature::UnifiedSignature::from_parse(store, consumed);
            Ok((parsed, signature))
        })?;
        Ok(UnifiedParse { parsed, signature })
    }
}

/// Result of parsing one Unified session store.
#[derive(Debug)]
pub struct UnifiedParse {
    /// The converted session, ready for indexing.
    pub parsed: ParsedSession,
    pub signature: super::signature::UnifiedSignature,
}

/// Converts a loaded, journal-replayed Unified store into a
/// [`ParsedSession`].
///
/// `session_dir` names the store (its basename is the session id) and
/// holds the optional `meta.json` fallback for project path and model.
/// Effect entries and skill payload blocks are recorded through
/// `effects` into tool-call, saved-result and subagent rows.
pub(crate) fn convert(
    session_dir: &Path,
    store: &LoadedStore,
    effects: &mut effects::EffectSink<'_>,
) -> Result<ParsedSession, UnifiedError> {
    if store.runtime.ephemeral {
        return Err(UnifiedError::Ephemeral);
    }
    if let Some(children) = &store.runtime.deleting_children {
        return Err(UnifiedError::DeletingTree {
            child_session_ids: children.clone(),
        });
    }

    // The loader validated this identity against CURRENT and the directory.
    let session_id = store.runtime.session_id.clone();

    let envelope = &store.projection.envelope;
    let session_object = envelope
        .get("session")
        .and_then(Value::as_object)
        .ok_or_else(|| UnifiedError::Corrupt("snapshot session is missing".to_string()))?;
    let start_time = envelope_timestamp(session_object.get("createdAt"), "createdAt")?;
    let last_updated = envelope_timestamp(session_object.get("updatedAt"), "updatedAt")?;
    let ending_status = ending_status(envelope);
    let token_usage = token_usage(session_object.get("tokenUsage"));

    // The runtime cwd wins over meta.json; a blank one falls back to a
    // matching meta.json working directory.
    let runtime_cwd = store
        .runtime
        .cwd
        .as_deref()
        .filter(|cwd| !cwd.trim().is_empty());
    let runtime_model_value = store
        .runtime
        .active_model
        .as_ref()
        .map(|model| Value::String(model.clone()));
    let runtime_model = normalize_model(runtime_model_value.as_ref());

    // meta.json is read at most once, and only when a fallback is needed.
    let meta_json = if runtime_cwd.is_none() || runtime_model.is_none() {
        read_meta_json(session_dir, &session_id)
    } else {
        None
    };
    let project_path = match runtime_cwd {
        Some(cwd) => Some(cwd.to_string()),
        None => meta_json
            .as_ref()
            .and_then(|meta| meta.pointer("/environment/working_directory"))
            .and_then(Value::as_str)
            .filter(|path| !path.trim().is_empty())
            .map(str::to_string),
    };
    let session_model = runtime_model.or_else(|| {
        let meta = meta_json.as_ref()?;
        normalize_model(
            meta.get("config")
                .and_then(|config| config.get("active_model")),
        )
    });

    // Only subagent identities are children; forks stay top-level.  Fact
    // J self-verification: a child's session id must derive from its
    // parent id and spawn key, so a forged or stale child directory is an
    // identity conflict.
    let (parent_session_id, is_subagent) = match &store.runtime.identity {
        Identity::Subagent { parent, spawn_key } => {
            let derived = child_session_id(parent, spawn_key);
            if derived != session_id {
                return Err(UnifiedError::IdentityConflict(format!(
                    "subagent session id {session_id} does not match the child id {derived} derived from parent {parent} and spawn key {spawn_key}"
                )));
            }
            (Some(parent.clone()), true)
        }
        Identity::Root | Identity::Fork => (None, false),
    };

    // Model attribution (fact H): once any model_change checkpoint exists,
    // the current model starts None and every change resets it; otherwise
    // assistant messages and reasoning carry the session model.
    let tracks_model_changes = store.projection.entries.iter().any(is_model_change);
    let mut current_model = if tracks_model_changes {
        None
    } else {
        session_model.clone()
    };

    let mut transcript = Transcript::new(&session_id);
    let mut has_user_message = false;

    for entry in &store.projection.entries {
        let entry_type = entry
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| UnifiedError::Corrupt("entry type is missing".to_string()))?;
        // The id is required on every entry (fact H); the effect mapping
        // consumes it.
        if entry.get("id").and_then(Value::as_str).is_none() {
            return Err(UnifiedError::Corrupt("entry id is missing".to_string()));
        }
        let turn_id = entry.get("turnId").and_then(Value::as_str);

        match entry_type {
            "message" => {
                let role = entry.get("role").and_then(Value::as_str).ok_or_else(|| {
                    UnifiedError::Corrupt("message entry role is missing".to_string())
                })?;
                let content = entry
                    .get("content")
                    .and_then(Value::as_array)
                    .ok_or_else(|| {
                        UnifiedError::Corrupt("message entry content is not an array".to_string())
                    })?;
                match role {
                    "user" => {
                        let (text, skill_payloads) = user_display_text(entry, content);
                        if text.trim().is_empty() {
                            continue;
                        }
                        has_user_message = true;
                        transcript.push_message(
                            Role::User,
                            text,
                            entry_timestamp(entry, start_time),
                            None,
                            turn_id,
                        );
                        for (name, payload) in skill_payloads {
                            effects.push_skill(entry, &name, &payload, &mut transcript);
                        }
                    }
                    "assistant" => {
                        if is_uncommitted(entry) {
                            continue;
                        }
                        let text = text_blocks_joined(content);
                        if text.trim().is_empty() {
                            continue;
                        }
                        transcript.push_message(
                            Role::Assistant,
                            text,
                            entry_timestamp(entry, start_time),
                            current_model.clone(),
                            turn_id,
                        );
                    }
                    other => {
                        return Err(UnifiedError::Unsupported(format!("message role {other:?}")));
                    }
                }
            }
            "reasoning" => {
                if is_uncommitted(entry) {
                    continue;
                }
                let mut piece = PendingReasoning::default();
                match entry.get("text").and_then(Value::as_str) {
                    // Upstream redaction sentinel (fact H): the reasoning
                    // text was encrypted away, the entry stays visible.
                    Some("[redacted]") => piece.has_encrypted_content = true,
                    Some(text) if !text.trim().is_empty() => {
                        piece.visible_text = Some(text.to_string());
                    }
                    _ => {}
                }
                if let Some(summary) = entry.get("summary").and_then(Value::as_array) {
                    let parts: Vec<&str> = summary.iter().filter_map(Value::as_str).collect();
                    if !parts.is_empty() {
                        piece.summary_text = Some(parts.join("\n"));
                    }
                }
                // A reasoning entry that yields nothing readable is
                // omitted entirely.
                if piece.is_empty() {
                    continue;
                }
                piece.source_model = current_model.clone();
                piece.source_timestamp = Some(entry_timestamp(entry, start_time));
                transcript.add_reasoning(turn_id, piece);
            }
            "effect" => effects.push_effect(entry, &mut transcript)?,
            "checkpoint" => {
                let kind = entry.get("kind").and_then(Value::as_str).ok_or_else(|| {
                    UnifiedError::Corrupt("checkpoint kind is missing".to_string())
                })?;
                match kind {
                    "model_change" => {
                        current_model = normalize_model(
                            entry
                                .get("details")
                                .and_then(|details| details.get("model")),
                        );
                    }
                    "agent_change" | "compaction" => {}
                    "recovered" => {
                        let discarded = entry
                            .get("details")
                            .and_then(|details| details.get("discardedEntries"));
                        warn!(
                            ?discarded,
                            "Mistral Vibe store recovered; history entries were discarded"
                        );
                    }
                    other => {
                        return Err(UnifiedError::Unsupported(format!(
                            "checkpoint kind {other:?}"
                        )));
                    }
                }
            }
            "notice" => {}
            other => {
                return Err(UnifiedError::Unsupported(format!("entry type {other:?}")));
            }
        }
    }

    transcript.drop_pending_reasoning();

    if !has_user_message {
        return Err(UnifiedError::NoUserMessages);
    }

    let first_prompt = extract_first_prompt(&transcript.messages);
    let message_count = transcript.messages.len();
    let file_path = session_dir.to_str().unwrap_or_default().to_string();
    let Transcript {
        messages,
        tool_calls,
        subagents,
        transcript_items,
        reasoning_attachments,
        ..
    } = transcript;

    Ok(ParsedSession {
        session: Session {
            id: session_id,
            tool: AiAssistant::MistralVibe,
            project_path,
            project_id: None,
            start_time,
            message_count,
            file_path,
            last_updated,
            pinned_at: None,
            first_prompt,
            parent_session_id,
            is_subagent,
            token_usage: None,
            edit_count: 0,
            read_count: 0,
            command_count: 0,
            ending_status,
        },
        messages,
        tool_calls,
        subagents,
        transcript_items,
        reasoning_attachments,
        token_usage,
        agent_path: None,
        context_entries: Vec::new(),
    })
}

/// Conversion accumulator shared by the converter and the effect sink.
///
/// Messages land here through [`Transcript::push_message`]; the effect
/// sink appends tool calls and subagents together with their transcript
/// items through [`Transcript::push_item`].  Pending reasoning attaches
/// to the next same-turn assistant message, tool call or subagent item
/// and is dropped with a debug diagnostic when an item of another turn
/// or the end of the history is reached first.
pub(crate) struct Transcript {
    session_id: String,
    pub(crate) messages: Vec<Message>,
    pub(crate) tool_calls: Vec<ToolCall>,
    pub(crate) subagents: Vec<Subagent>,
    pub(crate) transcript_items: Vec<TranscriptItem>,
    pub(crate) reasoning_attachments: Vec<ReasoningAttachment>,
    pending_reasoning: Option<PendingTurnReasoning>,
}

/// Pending reasoning plus the turn it belongs to: reasoning only attaches
/// to same-turn items.
struct PendingTurnReasoning {
    turn_id: Option<String>,
    reasoning: PendingReasoning,
}

impl Transcript {
    pub(crate) fn new(session_id: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            messages: Vec::new(),
            tool_calls: Vec::new(),
            subagents: Vec::new(),
            transcript_items: Vec::new(),
            reasoning_attachments: Vec::new(),
            pending_reasoning: None,
        }
    }

    /// Merges a reasoning piece into the pending reasoning of the same
    /// turn; a piece of another turn drops whatever is still pending.
    pub(crate) fn add_reasoning(&mut self, turn_id: Option<&str>, piece: PendingReasoning) {
        let same_turn = self
            .pending_reasoning
            .as_ref()
            .is_some_and(|pending| pending.turn_id.as_deref() == turn_id);
        if !same_turn {
            self.drop_pending_reasoning();
        }
        match &mut self.pending_reasoning {
            Some(pending) => pending.reasoning.merge(piece),
            None => {
                self.pending_reasoning = Some(PendingTurnReasoning {
                    turn_id: turn_id.map(str::to_string),
                    reasoning: piece,
                })
            }
        }
    }

    /// Appends a message with its transcript item.  Assistant messages
    /// attach same-turn pending reasoning; user messages never consume it.
    pub(crate) fn push_message(
        &mut self,
        role: Role,
        content: String,
        timestamp: DateTime<Utc>,
        model: Option<String>,
        turn_id: Option<&str>,
    ) {
        let message_index = self.messages.len();
        self.messages.push(Message {
            session_id: self.session_id.clone(),
            index: message_index,
            role,
            content,
            timestamp,
            model,
        });
        let item_index = self.transcript_items.len() as i64;
        self.transcript_items.push(TranscriptItem {
            session_id: self.session_id.clone(),
            item_index,
            kind: TranscriptItemKind::Message,
            message_index: Some(message_index as i64),
            tool_call_id: None,
            subagent_id: None,
            context_id: None,
        });
        if role == Role::Assistant {
            self.attach_pending_reasoning(item_index, turn_id);
        }
    }

    /// Appends an effect-derived transcript item (tool call or subagent),
    /// attaching same-turn pending reasoning.  The item index is
    /// `transcript_items.len()` before the caller builds the item.
    pub(crate) fn push_item(&mut self, item: TranscriptItem, turn_id: Option<&str>) {
        let item_index = item.item_index;
        self.transcript_items.push(item);
        self.attach_pending_reasoning(item_index, turn_id);
    }

    /// Drops any reasoning still pending after the history walk.
    pub(crate) fn drop_pending_reasoning(&mut self) {
        if let Some(pending) = self.pending_reasoning.take() {
            debug!(
                turn = ?pending.turn_id,
                "dropping pending reasoning that found no same-turn transcript item"
            );
        }
    }

    /// Attaches the pending reasoning to `item_index` when its turn
    /// matches; an item of another turn drops it.
    fn attach_pending_reasoning(&mut self, item_index: i64, turn_id: Option<&str>) {
        if let Some(pending) = self.pending_reasoning.take() {
            if pending.turn_id.as_deref() == turn_id {
                self.reasoning_attachments.push(
                    pending
                        .reasoning
                        .into_attachment(&self.session_id, item_index),
                );
            } else {
                debug!(
                    pending_turn = ?pending.turn_id,
                    item_turn = ?turn_id,
                    "dropping pending reasoning at an item of another turn"
                );
            }
        }
    }
}

/// A committed entry: provisional generations
/// (`generationStatus == "in_progress"`) and discarded outcomes are
/// omitted from the visible transcript (fact H).
fn is_uncommitted(entry: &Value) -> bool {
    if entry.get("generationStatus").and_then(Value::as_str) == Some("in_progress") {
        return true;
    }
    entry
        .get("outcome")
        .and_then(|outcome| outcome.get("type"))
        .and_then(Value::as_str)
        == Some("discarded")
}

fn is_model_change(entry: &Value) -> bool {
    entry.get("type").and_then(Value::as_str) == Some("checkpoint")
        && entry.get("kind").and_then(Value::as_str) == Some("model_change")
}

/// Text blocks joined with `\n`; non-text blocks are omitted (fact H).
pub(crate) fn text_blocks_joined(blocks: &[Value]) -> String {
    blocks
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

/// User display text (fact I): `userDisplayContent` text blocks win when
/// the object carries non-blank `version`/`host`, a content array and
/// non-blank text; otherwise the entry `content` is used with skill
/// payload blocks stripped, each reported for the effect sink.
fn user_display_text(entry: &Value, content: &[Value]) -> (String, Vec<(String, String)>) {
    if let Some(display) = entry.get("userDisplayContent").and_then(Value::as_object) {
        let version = display.get("version").and_then(Value::as_str);
        let host = display.get("host").and_then(Value::as_str);
        let credentials_present = version.is_some_and(|v| !v.trim().is_empty())
            && host.is_some_and(|h| !h.trim().is_empty());
        if credentials_present
            && let Some(blocks) = display.get("content").and_then(Value::as_array)
        {
            let text = text_blocks_joined(blocks);
            if !text.trim().is_empty() {
                return (text, Vec::new());
            }
        }
    }

    let mut parts: Vec<&str> = Vec::new();
    let mut skill_payloads: Vec<(String, String)> = Vec::new();
    for block in content {
        if block.get("type").and_then(Value::as_str) != Some("text") {
            continue;
        }
        let Some(text) = block.get("text").and_then(Value::as_str) else {
            continue;
        };
        match skill_payload_name(text) {
            Some(name) => skill_payloads.push((name.to_string(), text.to_string())),
            None => parts.push(text),
        }
    }
    (parts.join("\n"), skill_payloads)
}

/// Fact I: identifies a skill payload text block — one starting with
/// `<skill_content name="{name}">` or exactly equal to the upstream
/// already-loaded sentence — and extracts the invoked skill name.  The
/// already-loaded form must match the whole sentence: anything before or
/// after it is real user content and stays visible.
fn skill_payload_name(text: &str) -> Option<&str> {
    const OPEN: &str = "<skill_content name=\"";
    if let Some(rest) = text.strip_prefix(OPEN) {
        let end = rest.find("\">")?;
        let name = &rest[..end];
        return (!name.is_empty()).then_some(name);
    }
    const LOADED_TAIL: &str =
        "' is already loaded earlier in this conversation. Reuse those instructions.";
    let rest = text.strip_prefix("Skill '")?;
    if !rest.ends_with(LOADED_TAIL) {
        return None;
    }
    let name = &rest[..rest.len() - LOADED_TAIL.len()];
    (!name.is_empty()).then_some(name)
}

/// Session timestamps (fact G) are epoch milliseconds; a missing or
/// out-of-range value is corrupt.
fn envelope_timestamp(value: Option<&Value>, field: &str) -> Result<DateTime<Utc>, UnifiedError> {
    let millis = value
        .and_then(Value::as_i64)
        .ok_or_else(|| UnifiedError::Corrupt(format!("session {field} is missing")))?;
    DateTime::<Utc>::from_timestamp_millis(millis)
        .ok_or_else(|| UnifiedError::Corrupt(format!("session {field} {millis} is out of range")))
}

/// Entry timestamps fall back to the session `createdAt` (pinned rule).
fn entry_timestamp(entry: &Value, fallback: DateTime<Utc>) -> DateTime<Utc> {
    entry
        .get("createdAt")
        .and_then(Value::as_i64)
        .and_then(DateTime::<Utc>::from_timestamp_millis)
        .unwrap_or(fallback)
}

/// Ending status precedence (pinned rule): failed session or latest turn
/// → Error; running/blocked session or in-progress/interrupted latest
/// turn → Abrupt; completed latest turn → Clean; otherwise Unknown.
fn ending_status(envelope: &Value) -> SessionEndingStatus {
    let session_status = envelope
        .pointer("/session/status/type")
        .and_then(Value::as_str);
    let latest_turn = envelope
        .pointer("/latestTurn/status")
        .and_then(Value::as_str);
    if session_status == Some("failed") || latest_turn == Some("failed") {
        return SessionEndingStatus::Error;
    }
    if matches!(session_status, Some("running") | Some("blocked"))
        || matches!(latest_turn, Some("in_progress") | Some("interrupted"))
    {
        return SessionEndingStatus::Abrupt;
    }
    if latest_turn == Some("completed") {
        return SessionEndingStatus::Clean;
    }
    SessionEndingStatus::Unknown
}

/// Session token usage comes from `session.tokenUsage`, never
/// `contextUsage` (pinned rule).  A malformed usage object degrades to
/// `None` rather than failing the session.
fn token_usage(value: Option<&Value>) -> Option<TokenUsage> {
    let usage = value?.as_object()?;
    let input_tokens = usage.get("inputTokens").and_then(Value::as_i64)?;
    let output_tokens = usage.get("outputTokens").and_then(Value::as_i64)?;
    if usage
        .get("cachedInputTokens")
        .is_some_and(|cached| cached.as_i64().is_none())
    {
        debug!("ignoring tokenUsage with a non-integer cachedInputTokens");
        return None;
    }
    Some(TokenUsage {
        input_tokens,
        output_tokens,
        cache_read_tokens: usage.get("cachedInputTokens").and_then(Value::as_i64),
        cache_write_tokens: None,
        reasoning_tokens: None,
    })
}

/// Reads the store's optional meta.json, honoring the store's symlink
/// boundary and ignoring the file with a warning when it is malformed or
/// belongs to another session.
fn read_meta_json(session_dir: &Path, session_id: &str) -> Option<Value> {
    let path = session_dir.join("meta.json");
    if let Err(error) = reject_symlink(&path) {
        warn!(?error, "ignoring meta.json: the file is not a regular file");
        return None;
    }
    let raw = match fs::read(&path) {
        Ok(raw) => raw,
        // A missing meta.json is normal: Unified stores never require it.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            warn!(?error, "ignoring unreadable meta.json");
            return None;
        }
    };
    let meta: Value = match serde_json::from_slice(&raw) {
        Ok(meta) => meta,
        Err(error) => {
            warn!(?error, "ignoring malformed meta.json");
            return None;
        }
    };
    if meta.get("session_id").and_then(Value::as_str) != Some(session_id) {
        warn!("ignoring meta.json: session_id does not match the store");
        return None;
    }
    Some(meta)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ToolCallStatus;
    use crate::parsers::mistral_vibe_unified::store::{
        FileSample, ProjectionState, RuntimeView,
        test_support::{copy_fixture, rewrite_document},
    };
    use crate::parsers::mistral_vibe_unified::{child_session_id, effects::EffectSink};
    use serde_json::json;
    use std::path::PathBuf;
    use tempfile::TempDir;

    // Fixture IDs (Task 2 contract; the child ID matches the generated
    // fixture directory).
    const R302: &str = "aaaaaaaa-0000-4000-8000-000000000302";
    const R300: &str = "aaaaaaaa-0000-4000-8000-000000000300";
    const F301: &str = "aaaaaaaa-0000-4000-8000-000000000301";
    const CHILD_A: &str = "child-ea9b00b3ad4061f4ca36fd81";
    const T0_MS: i64 = 1_792_700_000_000;

    // Variant store IDs (Task 2 generator contract).
    const V_EPHEMERAL: &str = "cccccccc-0000-4000-8000-000000000001";
    const V_DELETING: &str = "cccccccc-0000-4000-8000-000000000002";
    const V_UNKNOWN_ENTRY: &str = "cccccccc-0000-4000-8000-000000000007";
    const V_UNKNOWN_CHECKPOINT: &str = "cccccccc-0000-4000-8000-000000000008";
    const V_EMPTY: &str = "cccccccc-0000-4000-8000-00000000000b";
    const V_RECOVERED: &str = "cccccccc-0000-4000-8000-00000000000c";

    fn unified_rel(id: &str) -> String {
        format!("vibe_sessions/unified/{id}")
    }

    fn variant_rel(name: &str, id: &str) -> String {
        format!("vibe_unified_variants/{name}/unified/{id}")
    }

    fn parse_fixture(rel: &str) -> Result<UnifiedParse, UnifiedError> {
        MistralVibeUnifiedParser.parse(&crate::fixture_path(rel))
    }

    /// Copies a Unified fixture store into a temp directory, keeping the
    /// session directory name the store's identity requires.
    fn copy_unified(id: &str) -> (TempDir, PathBuf) {
        let temp = copy_fixture(&unified_rel(id));
        let session = temp.path().join(id);
        (temp, session)
    }

    fn metadata_change_after_consumption(initial: Option<&str>) {
        let (_temp, dir) = copy_unified(R300);
        rewrite_document(&dir, "runtime-state.json", |doc| {
            doc["session_metadata"]["cwd"] = Value::Null;
            doc["session_metadata"]["active_model"] = Value::Null;
        });
        let meta_path = dir.join("meta.json");
        if meta_path.exists() {
            fs::remove_file(&meta_path).unwrap();
        }
        let metadata = |cwd: &str| {
            json!({
                "session_id": R300,
                "environment": {"working_directory": cwd},
                "config": {"active_model": "mistral-medium-3"}
            })
            .to_string()
        };
        if let Some(cwd) = initial {
            fs::write(&meta_path, metadata(cwd)).unwrap();
        }
        // Use the production stable loader, conversion, and signature builder.
        // Mutate exactly after consumption and before signature assembly; no
        // timing, sleeping, or writer thread is involved.
        let ((parsed, signature), _) = super::super::load_stable(&dir, |store| {
            let mut results = SavedResultResolver::new(dir.join("tool-results"));
            let parsed = {
                let mut effects = EffectSink::new(R300, &mut results);
                convert(&dir, store, &mut effects)?
            };
            assert_eq!(parsed.session.project_path.as_deref(), initial);
            fs::write(&meta_path, metadata("/project/new-metadata")).unwrap();
            let signature = super::super::signature::UnifiedSignature::from_parse(
                store,
                results.into_consumed(),
            );
            Ok((parsed, signature))
        })
        .unwrap();
        assert_eq!(parsed.session.project_path.as_deref(), initial);
        assert!(
            !signature.is_current(&dir),
            "signature acknowledged metadata the conversion never consumed"
        );
        // The next incremental freshness check must request a new parse, which
        // imports the new fallback and produces a genuinely current signature.
        let next = MistralVibeUnifiedParser.parse(&dir).unwrap();
        assert_eq!(
            next.parsed.session.project_path.as_deref(),
            Some("/project/new-metadata")
        );
        assert!(next.signature.is_current(&dir));
    }

    #[test]
    fn metadata_mutation_after_consumption_invalidates_signature() {
        metadata_change_after_consumption(Some("/project/old"));
    }

    #[test]
    fn metadata_appearance_after_consumption_invalidates_signature() {
        metadata_change_after_consumption(None);
    }

    fn expect_variant(result: Result<UnifiedParse, UnifiedError>, expected: &str) {
        let error = result.expect_err("expected the parse to fail");
        let actual = match &error {
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
        };
        assert_eq!(
            actual, expected,
            "parse failed with the wrong variant: {error}"
        );
    }

    // ------------------------------------------------------------------
    // In-memory conversion harness: entries built as JSON values, a
    // synthetic runtime whose cwd/model satisfy the metadata rules, and a
    // session directory path that is never touched on disk.
    // ------------------------------------------------------------------

    const IN_MEMORY_SESSION: &str = "in-memory";

    fn in_memory_session_dir() -> PathBuf {
        PathBuf::from("/home/fixture/.vibe/logs/session/unified").join(IN_MEMORY_SESSION)
    }

    fn in_memory_store(entries: Vec<Value>) -> LoadedStore {
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
                session_id: IN_MEMORY_SESSION.to_string(),
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
                        "id": IN_MEMORY_SESSION,
                        "rootSessionId": IN_MEMORY_SESSION,
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

    fn convert_entries(entries: Vec<Value>) -> Result<ParsedSession, UnifiedError> {
        let store = in_memory_store(entries);
        let session_dir = in_memory_session_dir();
        let mut results = SavedResultResolver::new(session_dir.join("tool-results"));
        let mut effects = EffectSink::new(IN_MEMORY_SESSION, &mut results);
        convert(&session_dir, &store, &mut effects)
    }

    fn user_message(id: &str, blocks: Value) -> Value {
        json!({
            "type": "message", "id": id, "turnId": "t1",
            "createdAt": T0_MS, "updatedAt": T0_MS, "generationStatus": "completed",
            "role": "user", "source": "turn_start", "content": blocks,
        })
    }

    // ------------------------------------------------------------------
    // R302: messages, metadata, reasoning (brief steps 1a-1c)
    // ------------------------------------------------------------------

    #[test]
    fn r302_messages_in_order() {
        let parsed = parse_fixture(&unified_rel(R302)).unwrap().parsed;

        let contents: Vec<&str> = parsed.messages.iter().map(|m| m.content.as_str()).collect();
        assert_eq!(
            contents,
            [
                "/vibe-skill Résume le projet 🚀",
                "Voici le résumé.",
                "Corrige le test",
                "Test corrigé.",
                "Ajoute la doc",
                "Et le README",
                "Doc ajoutée.",
            ]
        );
        let roles: Vec<Role> = parsed.messages.iter().map(|m| m.role).collect();
        assert_eq!(
            roles,
            [
                Role::User,
                Role::Assistant,
                Role::User,
                Role::Assistant,
                Role::User,
                Role::User,
                Role::Assistant,
            ]
        );
        let models: Vec<Option<&str>> =
            parsed.messages.iter().map(|m| m.model.as_deref()).collect();
        assert_eq!(
            models,
            [
                None,
                None,
                None,
                Some("mistral-large-4"),
                None,
                None,
                Some("mistral-large-4"),
            ]
        );
        for message in &parsed.messages {
            assert!(
                !message.content.contains("WRONG ANSWER"),
                "discarded assistant text leaked: {}",
                message.content
            );
            assert!(
                !message.content.contains("partial"),
                "provisional assistant text leaked: {}",
                message.content
            );
            assert!(
                !message.content.contains("<skill_content"),
                "skill payload block leaked: {}",
                message.content
            );
        }
    }

    #[test]
    fn r302_session_metadata() {
        let parsed = parse_fixture(&unified_rel(R302)).unwrap().parsed;
        let session = parsed.session;

        assert_eq!(session.id, R302);
        assert_eq!(
            session.project_path.as_deref(),
            Some("/home/fixture/projects/moved")
        );
        assert_eq!(
            session.start_time,
            DateTime::<Utc>::from_timestamp_millis(T0_MS).unwrap()
        );
        assert_eq!(
            session.last_updated,
            DateTime::<Utc>::from_timestamp_millis(T0_MS + 60_000).unwrap()
        );
        assert_eq!(session.ending_status, SessionEndingStatus::Clean);
        assert_eq!(
            session.first_prompt.as_deref(),
            Some("/vibe-skill Résume le projet 🚀")
        );
        assert_eq!(session.parent_session_id, None);
        assert!(!session.is_subagent);
        assert_eq!(
            parsed.token_usage,
            Some(TokenUsage {
                input_tokens: 1200,
                output_tokens: 300,
                cache_read_tokens: Some(800),
                cache_write_tokens: None,
                reasoning_tokens: None,
            })
        );
    }

    /// Finds the transcript item of the tool call with the given
    /// parser_call_id (action id).
    fn tool_call_item<'a>(parsed: &'a ParsedSession, action_id: &str) -> &'a TranscriptItem {
        let call = parsed
            .tool_calls
            .iter()
            .find(|call| call.parser_call_id.as_deref() == Some(action_id))
            .unwrap_or_else(|| panic!("tool call {action_id}"));
        parsed
            .transcript_items
            .iter()
            .find(|item| item.tool_call_id.as_deref() == Some(call.id.as_str()))
            .unwrap_or_else(|| panic!("transcript item of tool call {action_id}"))
    }

    /// The deferred Task 5 assertions, now over the real effect sink:
    /// reasoning attaches through `Transcript::push_item` to the first
    /// same-turn effect item — e2 on `act-read`, e7 on `act-patch` and
    /// e20b on `act-write`.
    #[test]
    fn r302_reasoning_attachments() {
        let parsed = parse_fixture(&unified_rel(R302)).unwrap().parsed;

        let attachments = &parsed.reasoning_attachments;
        assert_eq!(
            attachments.len(),
            3,
            "e2, e7 and e20b attach to their turn's effect items"
        );

        // e2 (turn t1) attaches to the act-read tool call item.
        let read_item = tool_call_item(&parsed, "act-read");
        assert_eq!(attachments[0].transcript_item_index, read_item.item_index);
        assert_eq!(
            attachments[0].visible_text.as_deref(),
            Some("Je regarde le dépôt")
        );
        assert_eq!(attachments[0].summary_text, None);
        assert!(!attachments[0].has_encrypted_content);
        assert_eq!(
            attachments[0].source_model, None,
            "e2 precedes the model_change checkpoint"
        );
        assert_eq!(
            parsed.transcript_items[read_item.item_index as usize]
                .kind
                .to_storage(),
            "tool_call"
        );

        // e7 (turn t2, redacted) attaches to the act-patch tool call item.
        let patch_item = tool_call_item(&parsed, "act-patch");
        assert_eq!(attachments[1].transcript_item_index, patch_item.item_index);
        assert_eq!(attachments[1].visible_text, None);
        assert_eq!(
            attachments[1].summary_text.as_deref(),
            Some("Résumé visible")
        );
        assert!(attachments[1].has_encrypted_content);
        assert_eq!(
            attachments[1].source_model.as_deref(),
            Some("mistral-large-4")
        );

        // e20b (turn t3) attaches to the act-write tool call item.
        let write_item = tool_call_item(&parsed, "act-write");
        assert_eq!(attachments[2].transcript_item_index, write_item.item_index);
        assert_eq!(
            attachments[2].visible_text.as_deref(),
            Some("Plan: write docs")
        );
    }

    /// Sessions without effect items still attach reasoning to the next
    /// same-turn assistant message.
    #[test]
    fn reasoning_attaches_to_next_same_turn_assistant_message() {
        let entries = vec![
            user_message("u1", json!([{"type": "text", "text": "Question"}])),
            json!({
                "type": "reasoning", "id": "r1", "turnId": "t1",
                "createdAt": T0_MS, "updatedAt": T0_MS, "generationStatus": "completed",
                "text": "Thinking", "summary": [], "outcome": {"type": "committed"},
            }),
            json!({
                "type": "message", "id": "a1", "turnId": "t1",
                "createdAt": T0_MS, "updatedAt": T0_MS, "generationStatus": "completed",
                "role": "assistant", "content": [ {"type": "text", "text": "Answer"} ],
                "outcome": {"type": "committed"},
            }),
        ];

        let parsed = convert_entries(entries).unwrap();
        assert_eq!(parsed.reasoning_attachments.len(), 1);
        let attachment = &parsed.reasoning_attachments[0];
        assert_eq!(attachment.visible_text.as_deref(), Some("Thinking"));
        let item = &parsed.transcript_items[attachment.transcript_item_index as usize];
        assert_eq!(item.kind.to_storage(), "message");
        let message = &parsed.messages[item.message_index.unwrap() as usize];
        assert_eq!(message.content, "Answer");
    }

    #[test]
    fn r300_without_model_change_uses_session_model() {
        let parsed = parse_fixture(&unified_rel(R300)).unwrap().parsed;

        let assistant = parsed
            .messages
            .iter()
            .find(|m| m.role == Role::Assistant)
            .expect("assistant message");
        assert_eq!(assistant.model.as_deref(), Some("mistral-medium-3"));
        assert_eq!(parsed.session.ending_status, SessionEndingStatus::Error);
    }

    #[test]
    fn f301_interrupted_is_abrupt_and_fork_is_top_level() {
        let session = parse_fixture(&unified_rel(F301)).unwrap().parsed.session;

        assert_eq!(session.ending_status, SessionEndingStatus::Abrupt);
        assert_eq!(session.parent_session_id, None);
        assert!(!session.is_subagent);
    }

    #[test]
    fn child_a_is_subagent_without_meta_json() {
        let parsed = parse_fixture(&unified_rel(CHILD_A)).unwrap().parsed;

        let session = &parsed.session;
        assert_eq!(session.parent_session_id.as_deref(), Some(R302));
        assert!(session.is_subagent);
        // CHILD_A ships without meta.json: the runtime cwd alone drives
        // the project path.
        assert_eq!(
            session.project_path.as_deref(),
            Some("/home/fixture/projects/original")
        );
        assert_eq!(
            parsed.token_usage,
            Some(TokenUsage {
                input_tokens: 50,
                output_tokens: 10,
                cache_read_tokens: Some(0),
                cache_write_tokens: None,
                reasoning_tokens: None,
            })
        );
    }

    // ------------------------------------------------------------------
    // Variant stores: lifecycle and error cases (brief step 1e)
    // ------------------------------------------------------------------

    #[test]
    fn ephemeral_variant_is_ephemeral() {
        expect_variant(
            parse_fixture(&variant_rel("ephemeral", V_EPHEMERAL)),
            "Ephemeral",
        );
    }

    #[test]
    fn deleting_tree_variant_lists_children() {
        let child = child_session_id(V_DELETING, "act-spawn-del");
        let error =
            parse_fixture(&variant_rel("deleting/deleting", V_DELETING)).expect_err("deleting");
        let UnifiedError::DeletingTree { child_session_ids } = error else {
            panic!("expected DeletingTree, got: {error}");
        };
        assert_eq!(child_session_ids, vec![child]);
    }

    #[test]
    fn empty_variant_has_no_user_messages() {
        expect_variant(
            parse_fixture(&variant_rel("empty", V_EMPTY)),
            "NoUserMessages",
        );
    }

    #[test]
    fn unknown_entry_type_is_unsupported() {
        expect_variant(
            parse_fixture(&variant_rel("unknown-entry", V_UNKNOWN_ENTRY)),
            "Unsupported",
        );
    }

    #[test]
    fn unknown_checkpoint_kind_is_unsupported() {
        expect_variant(
            parse_fixture(&variant_rel("unknown-checkpoint", V_UNKNOWN_CHECKPOINT)),
            "Unsupported",
        );
    }

    #[test]
    fn recovered_variant_parses() {
        let parsed = parse_fixture(&variant_rel("recovered", V_RECOVERED))
            .unwrap()
            .parsed;

        let contents: Vec<&str> = parsed.messages.iter().map(|m| m.content.as_str()).collect();
        assert_eq!(contents, ["Recovered question", "Recovered answer"]);
    }

    // ------------------------------------------------------------------
    // Display content and skill payloads (brief step 1f, fact I)
    // ------------------------------------------------------------------

    #[test]
    fn display_content_text_wins() {
        let entry = json!({
            "type": "message", "id": "u1", "turnId": "t1",
            "createdAt": T0_MS, "updatedAt": T0_MS, "generationStatus": "completed",
            "role": "user", "source": "turn_start",
            "content": [ {"type": "text", "text": "Injected"} ],
            "userDisplayContent": {
                "version": "1", "host": "vibe",
                "content": [ {"type": "text", "text": "Shown"} ],
            },
        });

        let parsed = convert_entries(vec![entry.clone()]).unwrap();
        assert_eq!(parsed.messages.len(), 1);
        assert_eq!(parsed.messages[0].content, "Shown");

        // A blank version disqualifies userDisplayContent: the entry
        // content wins.
        let mut blank_version = entry;
        blank_version["userDisplayContent"]["version"] = json!(" ");
        let parsed = convert_entries(vec![blank_version]).unwrap();
        assert_eq!(parsed.messages[0].content, "Injected");
    }

    #[test]
    fn skill_payload_blocks_are_stripped_from_user_content() {
        let entry = user_message(
            "u1",
            json!([
                {"type": "text", "text": "Do the thing"},
                {"type": "text", "text": "<skill_content name=\"vibe-skill\">\nSkill body\n</skill_content>"},
                {"type": "text", "text": "Skill 'other-skill' is already loaded earlier in this conversation. Reuse those instructions."},
                {"type": "image", "attachment": {"mimeType": "image/png", "path": "/home/fixture/screenshot.png"}},
            ]),
        );

        let parsed = convert_entries(vec![entry]).unwrap();
        assert_eq!(parsed.messages.len(), 1);
        assert_eq!(parsed.messages[0].content, "Do the thing");

        // Each payload becomes a settled `skill` tool call right after
        // the message item.
        assert_eq!(parsed.tool_calls.len(), 2);
        let names: Vec<&str> = parsed
            .tool_calls
            .iter()
            .map(|call| call.tool_name.as_str())
            .collect();
        assert_eq!(names, ["skill", "skill"]);
        assert_eq!(
            parsed.tool_calls[0].id,
            format!("{IN_MEMORY_SESSION}-u1:invoked-skill:vibe-skill")
        );
        assert_eq!(
            parsed.tool_calls[1].id,
            format!("{IN_MEMORY_SESSION}-u1:invoked-skill:other-skill")
        );
        assert_eq!(
            parsed.tool_calls[0].input_json.as_deref(),
            Some("{\"name\":\"vibe-skill\"}")
        );
        assert_eq!(parsed.tool_calls[0].status, ToolCallStatus::Completed);
        let output = parsed.tool_calls[0]
            .output_text
            .as_deref()
            .expect("skill payload output");
        assert!(output.starts_with("<skill_content name=\"vibe-skill\">"));
        let loaded = parsed.tool_calls[1]
            .output_text
            .as_deref()
            .expect("already-loaded payload output");
        assert_eq!(
            loaded,
            "Skill 'other-skill' is already loaded earlier in this conversation. Reuse those instructions."
        );

        // The skill items sit right after the message item.
        let kinds: Vec<&str> = parsed
            .transcript_items
            .iter()
            .map(|item| item.kind.to_storage())
            .collect();
        assert_eq!(kinds, ["message", "tool_call", "tool_call"]);
        assert_eq!(
            parsed.transcript_items[1].tool_call_id.as_deref(),
            Some(parsed.tool_calls[0].id.as_str())
        );
    }

    /// Review finding (regraded Important): the already-loaded sentence
    /// matcher must be exact.  Text after the sentence is real user
    /// content and stays visible; only the exact upstream sentence is a
    /// skill payload block.
    #[test]
    fn near_match_already_loaded_sentence_stays_visible() {
        let entry = user_message(
            "u1",
            json!([
                {"type": "text", "text": "Do the thing"},
                {"type": "text", "text": "Skill 'vibe-skill' is already loaded earlier in this conversation. Reuse those instructions. But also read the docs"},
            ]),
        );

        let parsed = convert_entries(vec![entry]).unwrap();
        assert_eq!(parsed.messages.len(), 1);
        assert_eq!(
            parsed.messages[0].content,
            "Do the thing\nSkill 'vibe-skill' is already loaded earlier in this conversation. Reuse those instructions. But also read the docs"
        );
        // No skill payload was extracted for the near match.
        assert!(parsed.tool_calls.is_empty());
    }

    #[test]
    fn reasoning_of_another_turn_is_dropped_at_the_next_item() {
        let entries = vec![
            json!({
                "type": "reasoning", "id": "r1", "turnId": "t1",
                "createdAt": T0_MS, "updatedAt": T0_MS, "generationStatus": "completed",
                "text": "Old turn thinking", "summary": [], "outcome": {"type": "committed"},
            }),
            user_message("u2", json!([{"type": "text", "text": "Next turn"}])),
            json!({
                "type": "message", "id": "a2", "turnId": "t2",
                "createdAt": T0_MS, "updatedAt": T0_MS, "generationStatus": "completed",
                "role": "assistant", "content": [ {"type": "text", "text": "Answer"} ],
                "outcome": {"type": "committed"},
            }),
        ];

        let parsed = convert_entries(entries).unwrap();
        assert_eq!(parsed.messages.len(), 2);
        assert!(
            parsed.reasoning_attachments.is_empty(),
            "reasoning of another turn must not attach"
        );
    }

    #[test]
    fn blank_user_and_assistant_text_is_omitted() {
        let entries = vec![
            user_message("u1", json!([{"type": "text", "text": "   "}])),
            json!({
                "type": "message", "id": "a1", "turnId": "t1",
                "createdAt": T0_MS, "updatedAt": T0_MS, "generationStatus": "completed",
                "role": "assistant", "content": [ {"type": "text", "text": ""} ],
                "outcome": {"type": "committed"},
            }),
        ];

        // The blank user message does not count, so the session has no
        // user message at all.
        let error = convert_entries(entries).expect_err("no user message");
        assert!(
            matches!(error, UnifiedError::NoUserMessages),
            "got: {error}"
        );
    }

    // ------------------------------------------------------------------
    // Structural error cases (pinned: missing type/id, non-array
    // content, unsupported role)
    // ------------------------------------------------------------------

    #[test]
    fn missing_entry_type_or_id_is_corrupt() {
        let missing_type = json!({"id": "u1", "turnId": "t1", "role": "user", "content": []});
        let error = convert_entries(vec![missing_type]).expect_err("missing type");
        assert!(matches!(error, UnifiedError::Corrupt(_)), "got: {error}");

        let missing_id = json!({"type": "message", "turnId": "t1", "role": "user", "content": []});
        let error = convert_entries(vec![missing_id]).expect_err("missing id");
        assert!(matches!(error, UnifiedError::Corrupt(_)), "got: {error}");
    }

    #[test]
    fn non_array_message_content_is_corrupt() {
        let entry = json!({
            "type": "message", "id": "u1", "turnId": "t1",
            "createdAt": T0_MS, "updatedAt": T0_MS, "generationStatus": "completed",
            "role": "user", "content": "plain text",
        });
        let error = convert_entries(vec![entry]).expect_err("non-array content");
        assert!(matches!(error, UnifiedError::Corrupt(_)), "got: {error}");
    }

    #[test]
    fn unsupported_message_role_is_unsupported() {
        let entry = json!({
            "type": "message", "id": "s1", "turnId": "t1",
            "createdAt": T0_MS, "updatedAt": T0_MS, "generationStatus": "completed",
            "role": "system", "content": [],
        });
        let error = convert_entries(vec![entry]).expect_err("unsupported role");
        assert!(
            matches!(error, UnifiedError::Unsupported(_)),
            "got: {error}"
        );
    }

    // ------------------------------------------------------------------
    // meta.json fallbacks: project path and session model (pinned rule)
    // ------------------------------------------------------------------

    #[test]
    fn empty_runtime_cwd_falls_back_to_meta_json() {
        let (temp, session) = copy_unified(R300);
        rewrite_document(&session, "runtime-state.json", |document| {
            document["session_metadata"]["cwd"] = json!("");
        });

        let parsed = MistralVibeUnifiedParser.parse(&session).unwrap().parsed;
        assert_eq!(
            parsed.session.project_path.as_deref(),
            Some("/home/fixture/projects/original")
        );
        let _ = temp;
    }

    #[test]
    fn mismatched_or_malformed_meta_json_is_ignored() {
        // Mismatched session_id: warned and ignored.
        let (temp, session) = copy_unified(R300);
        rewrite_document(&session, "runtime-state.json", |document| {
            document["session_metadata"]["cwd"] = json!("");
        });
        let meta = json!({
            "session_id": "aaaaaaaa-0000-4000-8000-000000000ffd",
            "environment": {"working_directory": "/home/fixture/projects/elsewhere"},
        });
        fs::write(session.join("meta.json"), meta.to_string()).unwrap();
        let parsed = MistralVibeUnifiedParser.parse(&session).unwrap().parsed;
        assert_eq!(parsed.session.project_path, None);
        let _ = temp;

        // Malformed JSON: warned and ignored.
        let (temp, session) = copy_unified(R300);
        rewrite_document(&session, "runtime-state.json", |document| {
            document["session_metadata"]["cwd"] = json!("");
        });
        fs::write(session.join("meta.json"), b"{not json").unwrap();
        let parsed = MistralVibeUnifiedParser.parse(&session).unwrap().parsed;
        assert_eq!(parsed.session.project_path, None);
        let _ = temp;
    }

    #[test]
    fn missing_runtime_model_falls_back_to_meta_json() {
        let (temp, session) = copy_unified(R300);
        rewrite_document(&session, "runtime-state.json", |document| {
            document["session_metadata"]["active_model"] = Value::Null;
        });
        let meta = json!({
            "session_id": R300,
            "environment": {"working_directory": "/home/fixture/projects/original"},
            "config": {"active_model": "mistral-small-1"},
        });
        fs::write(session.join("meta.json"), meta.to_string()).unwrap();

        let parsed = MistralVibeUnifiedParser.parse(&session).unwrap().parsed;
        let assistant = parsed
            .messages
            .iter()
            .find(|m| m.role == Role::Assistant)
            .expect("assistant message");
        assert_eq!(assistant.model.as_deref(), Some("mistral-small-1"));
        let _ = temp;
    }

    #[test]
    fn symlinked_meta_json_is_ignored() {
        let (temp, session) = copy_unified(R300);
        rewrite_document(&session, "runtime-state.json", |document| {
            document["session_metadata"]["cwd"] = json!("");
        });
        let target = temp.path().join("meta-target.json");
        fs::write(
            &target,
            br#"{"session_id":"aaaaaaaa-0000-4000-8000-000000000300","environment":{"working_directory":"/home/fixture/projects/elsewhere"}}"#,
        )
        .unwrap();
        fs::remove_file(session.join("meta.json")).unwrap();
        std::os::unix::fs::symlink(&target, session.join("meta.json")).unwrap();

        // The symlinked meta.json is rejected at the store's symlink
        // boundary and ignored: no project path.
        let parsed = MistralVibeUnifiedParser.parse(&session).unwrap().parsed;
        assert_eq!(parsed.session.project_path, None);
    }
}
