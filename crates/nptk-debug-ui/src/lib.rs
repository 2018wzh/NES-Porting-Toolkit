//! # nptk-debug-ui
//!
//! Standalone FLTK-based debug UI window for the NES Porting Toolkit.
//!
//! Runs in its own thread and communicates with the main (winit) thread
//! via `mpsc` channels.  The main thread pushes `DebugData` snapshots
//! each frame, and the FLTK thread renders them in a native window.
//!
//! ## Usage
//!
//! ```ignore
//! use nptk_debug_ui::{DebugWindowHandle, DebugCommand, DebugData};
//!
//! // Spawn the FLTK window in a background thread
//! let handle = DebugWindowHandle::spawn();
//!
//! // Each frame, push fresh NES state
//! handle.update(debug_data);
//!
//! // To close the window:
//! handle.tx.send(DebugCommand::Shutdown).ok();
//! ```

pub mod debug_data;
pub mod fltk_ui;

pub use debug_data::{
    DebugCommand, DebugData, DebugEvent, DebugUiState, InputMappings, NesButton,
};
pub use fltk_ui::DebugWindowHandle;