//! Internal implementation of the `SessionDetail` transcript view.
//!
//! These modules are coupled to `SessionDetail` (they depend on
//! `SessionDetailMsg`) and are not meant as a general-purpose UI API, so they
//! live here rather than at the top level of `ui`.

pub(crate) mod display;
pub(crate) mod item_data;
pub(crate) mod item_init;
pub(crate) mod row_rendering;
pub(crate) mod tool_call_row;
pub(crate) mod tool_preview;
pub(crate) mod typed_row;
