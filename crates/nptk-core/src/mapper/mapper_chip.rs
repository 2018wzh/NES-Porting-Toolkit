//! MapperChip trait — 所有 Mapper 需实现的核心接口
//!
//! 每个 NES 卡带芯片（Mapper）都需要实现此 trait。Mapper 通过
//! `Rc<RefCell<MapperContext>>` 访问卡带存储和事件接口。

use std::cell::RefCell;
use std::rc::Rc;

use super::audio::ExpansionAudio;
use super::context::MapperContext;
use super::types::{IrqState, MapperDebugInfo, MapperSaveState, PpuBusEvent};
use crate::rom::Mirroring;

/// Mapper 芯片接口
///
/// 所有 NES/Famicom 卡带芯片（Mapper）必须实现此 trait。
///
/// # 方法分类
/// - **标识**: `mapper_id()`, `name()`
/// - **CPU 总线**: `cpu_read()`, `cpu_write()`
/// - **PPU 总线**: `ppu_read()`, `ppu_write()`
/// - **时钟推进**: `cpu_tick()`, `ppu_tick()`
/// - **IRQ**: `irq_state()`, `clear_irq()`
/// - **镜像**: `mirroring()`
/// - **扩展音频**: `expansion_audio()`（可选）
/// - **状态持久化**: `save_state()`, `load_state()`
/// - **调试**: `debug_info()`（可选）
pub trait MapperChip {
    /// 返回 Mapper ID（iNES 编号）
    fn mapper_id(&self) -> u16;

    /// 返回 Mapper 名称（如 "NROM", "UxROM", "MMC1"）
    fn name(&self) -> &'static str;

    // ── CPU 总线 ──

    /// CPU 读取卡带地址空间
    ///
    /// 返回 `Some(value)` 表示此地址由 Mapper 处理，
    /// 返回 `None` 表示未映射（由 Bus 处理 open bus）。
    fn cpu_read(&mut self, ctx: &Rc<RefCell<MapperContext>>, addr: u16) -> Option<u8>;

    /// CPU 写入卡带地址空间
    ///
    /// 返回 `true` 表示写入已被处理，
    /// 返回 `false` 表示未映射（写入被忽略）。
    fn cpu_write(&mut self, ctx: &Rc<RefCell<MapperContext>>, addr: u16, value: u8) -> bool;

    // ── PPU 总线 ──

    /// PPU 读取卡带地址空间（图案表 $0000-$1FFF）
    fn ppu_read(&mut self, ctx: &Rc<RefCell<MapperContext>>, addr: u16) -> Option<u8>;

    /// PPU 写入卡带地址空间（图案表 $0000-$1FFF）
    fn ppu_write(&mut self, ctx: &Rc<RefCell<MapperContext>>, addr: u16, value: u8) -> bool;

    // ── 时钟推进 ──

    /// CPU 时钟推进（用于 MMC3 等需要 CPU 计数的 Mapper）
    fn cpu_tick(&mut self, _ctx: &Rc<RefCell<MapperContext>>, _cycles: u32) {}

    /// PPU 时钟推进（用于 MMC3 等需要观察 PPU 地址线的 Mapper）
    fn ppu_tick(&mut self, _ctx: &Rc<RefCell<MapperContext>>, _event: PpuBusEvent) {}

    // ── IRQ ──

    /// 返回当前 IRQ 状态
    fn irq_state(&self) -> IrqState {
        IrqState::Inactive
    }

    /// 清除 IRQ
    fn clear_irq(&mut self) {}

    // ── 镜像 ──

    /// 返回当前 nametable 镜像模式
    fn mirroring(&self) -> Mirroring;

    // ── 扩展音频 ──

    /// 返回扩展音频接口引用（可选）
    fn expansion_audio(&mut self) -> Option<&mut dyn ExpansionAudio> {
        None
    }

    // ── 状态持久化 ──

    /// 保存 Mapper 状态
    fn save_state(&self) -> MapperSaveState;

    /// 加载 Mapper 状态
    fn load_state(&mut self, state: &MapperSaveState);

    // ── 调试 ──

    /// 返回调试信息（可选）
    fn debug_info(&self) -> MapperDebugInfo {
        MapperDebugInfo::default()
    }
}
