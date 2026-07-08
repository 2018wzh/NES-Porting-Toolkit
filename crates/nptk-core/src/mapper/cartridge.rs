//! Cartridge — 卡带容器
//!
//! Cartridge 是 Mapper 和卡带存储（PRG-ROM、CHR、PRG-RAM）的容器。
//! 它实现了 CPU/PPU 总线的卡带地址空间访问，并将请求转发给 MapperChip。

use std::cell::RefCell;
use std::rc::Rc;

use super::context::MapperContext;
use super::event_sink::{CartridgeEventSink, NullEventSink};
use super::mapper_chip::MapperChip;
use super::types::{
    ChrStorage, IrqState, PpuBusEvent,
};
use crate::rom::Mirroring;

/// 卡带元数据
#[derive(Debug, Clone)]
pub struct CartridgeMetadata {
    pub mapper_id: u16,
    pub submapper_id: u8,
    pub prg_rom_size: usize,
    pub chr_rom_size: usize,
    pub has_sram: bool,
    pub has_trainer: bool,
    pub battery_backed: bool,
}

/// 卡带容器
///
/// 包含 Mapper 芯片、卡带存储和共享上下文。
/// NesBusImpl 通过 Cartridge 访问所有卡带相关功能。
pub struct Cartridge {
    pub metadata: CartridgeMetadata,
    pub mapper: Box<dyn MapperChip>,
    pub ctx: Rc<RefCell<MapperContext>>,
}

impl Cartridge {
    /// 创建新的 Cartridge
    pub fn new(
        metadata: CartridgeMetadata,
        prg_rom: Vec<u8>,
        chr: ChrStorage,
        mapper: Box<dyn MapperChip>,
        event_sink: Box<dyn CartridgeEventSink>,
    ) -> Self {
        let ctx = MapperContext::new(prg_rom, chr, event_sink).into_rc();
        Cartridge {
            metadata,
            mapper,
            ctx,
        }
    }

    /// 使用 NullEventSink 创建 Cartridge
    pub fn new_simple(
        metadata: CartridgeMetadata,
        prg_rom: Vec<u8>,
        chr: ChrStorage,
        mapper: Box<dyn MapperChip>,
    ) -> Self {
        Self::new(metadata, prg_rom, chr, mapper, Box::new(NullEventSink))
    }

    // ── CPU 总线 ──

    /// CPU 读取卡带地址空间
    pub fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        self.mapper.cpu_read(&self.ctx, addr)
    }

    /// CPU 写入卡带地址空间
    pub fn cpu_write(&mut self, addr: u16, value: u8) -> bool {
        self.mapper.cpu_write(&self.ctx, addr, value)
    }

    // ── PPU 总线 ──

    /// PPU 读取卡带地址空间
    pub fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        self.mapper.ppu_read(&self.ctx, addr)
    }

    /// PPU 写入卡带地址空间
    pub fn ppu_write(&mut self, addr: u16, value: u8) -> bool {
        self.mapper.ppu_write(&self.ctx, addr, value)
    }

    // ── 时钟推进 ──

    /// CPU 时钟推进
    pub fn cpu_tick(&mut self, cycles: u32) {
        self.mapper.cpu_tick(&self.ctx, cycles);
    }

    /// PPU 时钟推进
    pub fn ppu_tick(&mut self, event: PpuBusEvent) {
        self.mapper.ppu_tick(&self.ctx, event);
    }

    // ── IRQ ──

    /// 返回当前 IRQ 状态
    pub fn irq_state(&self) -> IrqState {
        self.mapper.irq_state()
    }

    /// 清除 IRQ
    pub fn clear_irq(&mut self) {
        self.mapper.clear_irq();
    }

    // ── 镜像 ──

    /// 返回当前 nametable 镜像模式
    pub fn mirroring(&self) -> Mirroring {
        self.mapper.mirroring()
    }

    // ── 状态持久化 ──

    /// 保存卡带状态
    pub fn save_state(&self) -> super::types::MapperSaveState {
        self.mapper.save_state()
    }

    /// 加载卡带状态
    pub fn load_state(&mut self, state: &super::types::MapperSaveState) {
        self.mapper.load_state(state);
    }

    // ── 调试 ──

    /// 返回调试信息
    pub fn debug_info(&self) -> super::types::MapperDebugInfo {
        self.mapper.debug_info()
    }
}