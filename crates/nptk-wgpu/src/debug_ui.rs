//! Debug UI types — re-exported from `nptk-debug-ui` for backward compatibility.
//!
//! The actual debug UI implementation has moved to the `nptk-debug-ui` crate
//! which uses FLTK for a standalone window.  This module now only re-exports
//! the shared data types so that existing code (e.g. game crates) continues
//! to compile without changes.

pub use nptk_debug_ui::{DebugData, DebugUiState, InputMappings, NesButton};
