//! nptk-wgpu: WGPU 渲染器
//! 支持兼容模式 framebuffer 上传和原生 tilemap/sprite 渲染

pub mod debug_ui;
pub mod palette;
pub mod renderer;
pub mod sprite;
pub mod tilemap;

// Re-export common debug types at the crate root for convenience.
pub use debug_ui::{DebugData, DebugUiState, InputMappings, NesButton};
