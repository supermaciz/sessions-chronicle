//! Reader for Mistral Vibe 3.0.x Unified session stores
//! (`<vibe-root>/unified/<id>/`, `mistral.vibe.unified-session-store/v1`).

mod canonical;
mod error;

pub use error::UnifiedError;
