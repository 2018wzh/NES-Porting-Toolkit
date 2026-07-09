//! # nptk — NES Porting Toolkit
//!
//! 统一 facade crate，暴露游戏移植所需的最小 API 集合。
//!
//! ## 运行时 API
//!
//! 所有游戏移植需要的类型和函数都通过本 crate 的根模块或 `prelude` 导出。
//!
//! ## 构建时 API
//!
//! `nptk::build` 模块提供 AOT 编译工具（Cranelift），用于 build.rs 中。
//!
//! ## 用法
//!
//! ```ignore
//! // Cargo.toml
//! // [dependencies]
//! // nptk = { path = "../../crates/nptk" }
//! //
//! // [build-dependencies]
//! // nptk = { path = "../../crates/nptk" }
//!
//! // build.rs
//! use nptk::build::{CraneliftAot, IrBuilder};
//!
//! // src/main.rs
//! use nptk::prelude::*;
//! use nptk::{NesApp, GameHandlers, FrameContext};
//! ```

// ============================================================================
// 应用框架
// ============================================================================

pub use nptk_app::{ExecMode, FrameContext, GameHandlers, NesApp};

// ============================================================================
// NES 核心
// ============================================================================

/// ROM 解析
pub mod rom {
    pub use nptk_core::rom::{Mirroring, NesRom, RomError, RomHeader, parse_rom};
}

/// NES 系统
pub mod system {
    pub use nptk_core::system::{CPU_CYCLES_PER_FRAME, NesSystem};
}

/// CPU 引用（mos6502 封装）
pub mod cpu_ref {
    pub use nptk_core::cpu_ref::{Cpu6502, CpuFlags, MosStatus, MosRicoh2a03};
}

/// 总线
pub mod bus {
    pub use nptk_core::bus::NesBus;
    pub use nptk_core::bus::NesBusImpl;
}

/// 控制器
pub mod controller {
    pub use nptk_core::controller::NesControllerState;
}

/// Mapper
pub mod mapper {
    pub use nptk_core::mapper::{self, Cartridge, CartridgeMetadata, ChrStorage, create_mapper};
    /// Mapper 注册表
    pub mod registry {
        pub use nptk_core::mapper::registry::*;
    }

    /// 初始化 mapper 注册表
    ///
    /// 在首次调用 `create_mapper()` 之前调用。
    /// 委托给 `nptk_mapper::init()`。
    pub fn init() {
        nptk_mapper::init();
    }
}

// ============================================================================
// 运行时 ABI
// ============================================================================

pub mod runtime {
    pub use nptk_native_runtime::runtime::{
        AudioEventSink, CAbiBlockFn, NativeCpuState, PpuEventSink, RecompiledRuntime,
    };
}

// ============================================================================
// 调试 / 渲染
// ============================================================================

pub mod debug {
    pub use nptk_wgpu::debug_ui::DebugData;
}

pub mod render {
    pub use nptk_wgpu::renderer::RenderMode;
}

// ============================================================================
// 音频
// ============================================================================

pub mod audio {
    pub use nptk_audio::apu_mixer::ApuMixer;
}

// ============================================================================
// Profile / Hook
// ============================================================================

pub mod profile {
    pub use nptk_profile::hooks::{CodeHook, HookConfig, HookType};
}

// ============================================================================
// 构建时 API（用于 build.rs）
// ============================================================================

pub mod build {
    //! AOT 编译工具 — 用于 build.rs
    pub use nptk_recompiler::codegen::{CompiledBlock, CraneliftAot};
    pub use nptk_recompiler::ir_builder::IrBuilder;
}

// ============================================================================
// Prelude — 一键导入最常用的类型
// ============================================================================

pub mod prelude {
    //! 游戏移植最常用的类型

    // 应用框架
    pub use crate::{ExecMode, FrameContext, GameHandlers, NesApp};

    // NES 核心
    pub use crate::bus::NesBusImpl;
    pub use crate::controller::NesControllerState;
    pub use crate::mapper::{Cartridge, CartridgeMetadata, ChrStorage, create_mapper};
    pub use crate::rom::{NesRom, parse_rom};
    pub use crate::system::NesSystem;

    // 运行时
    pub use crate::runtime::{
        AudioEventSink, CAbiBlockFn, NativeCpuState, PpuEventSink, RecompiledRuntime,
    };

    // 调试 / 渲染
    pub use crate::debug::DebugData;
    pub use crate::render::RenderMode;

    // 音频
    pub use crate::audio::ApuMixer;
}
