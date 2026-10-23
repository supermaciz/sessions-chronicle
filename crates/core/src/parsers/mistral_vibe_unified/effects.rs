//! Effect mapping for Mistral Vibe Unified projections.
//!
//! Task 5 ships the sink as a no-op placeholder: the converter routes every
//! `effect` history entry and every skill payload block of a user message
//! through it, but no tool call, subagent or skill rows are produced yet.
//! Task 6 replaces this stub with the real mapping (tool calls, statuses,
//! saved results, subagents) on top of the shared [`Transcript`]
//! accumulator.

use serde_json::Value;

use super::convert::Transcript;
use super::error::UnifiedError;

/// Records effect history entries and skill payload calls into the
/// conversion transcript.  No-op until Task 6.
pub(crate) struct EffectSink;

impl EffectSink {
    /// Maps one `effect` history entry (fact H) to tool call or subagent
    /// rows.  Task 5 records nothing; Task 6 implements the mapping.
    pub(crate) fn push_effect(
        &mut self,
        _entry: &Value,
        _out: &mut Transcript,
    ) -> Result<(), UnifiedError> {
        Ok(())
    }

    /// Maps one skill payload block (fact I) of a user message to a settled
    /// `skill` tool call placed right after the message item.  Task 5
    /// records nothing; Task 6 implements the mapping.
    pub(crate) fn push_skill(
        &mut self,
        _message_entry: &Value,
        _name: &str,
        _text: &str,
        _out: &mut Transcript,
    ) {
    }
}
