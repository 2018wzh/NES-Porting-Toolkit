//! Mapper 抽象与注册机制
//!
//! 本模块定义了 NES/Famicom 卡带芯片（Mapper）的核心接口和类型。
//!
//! # 架构
//!
//! - **MapperChip trait** — 所有 mapper 需实现的核心接口
//! - **MapperContext** — mapper 运行上下文（Rc<RefCell<>> 共享）
//! - **Cartridge** — 卡带容器，封装 mapper + 存储
//! - **AddressMapper** — 地址翻译辅助 trait
//! - **registry** — 显式注册机制
//!
//! # 依赖关系
//!
//! ```text
//! nptk-core::mapper::registry (全局注册表)
//!   ↑
//! nptk-mapper (init() 中注册所有启用的 mapper)
//!   ↑
//! mapper-nrom, mapper-uxrom, mapper-cnrom (提供构造器)
//! ```

pub mod address_mapper;
pub mod audio;
pub mod cartridge;
pub mod context;
pub mod event_sink;
pub mod map_result;
pub mod mapper_chip;
pub mod registry;
pub mod types;

// 重导出关键类型到 mapper 模块根
pub use address_mapper::AddressMapper;
pub use audio::ExpansionAudio;
pub use cartridge::{Cartridge, CartridgeMetadata};
pub use context::MapperContext;
pub use event_sink::{CartridgeEventSink, NullEventSink};
pub use map_result::{CpuMapResult, CpuWriteAction, PpuMapResult, PpuWriteAction};
pub use mapper_chip::MapperChip;
pub use registry::create_mapper;
pub use types::{
    ChrStorage, IrqState, MapperDebugInfo, MapperSaveState, NesRegion, PpuAccessKind, PpuBusEvent,
    PrgRam,
};
