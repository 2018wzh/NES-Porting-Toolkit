//! AddressMapper trait — 地址翻译辅助接口
//!
//! 将 CPU/PPU 地址翻译为具体的映射目标，与 MapperChip 的副作用逻辑分离。
//! 简单 Mapper 可以直接基于 AddressMapper 实现 MapperChip。

use super::map_result::{CpuMapResult, CpuWriteAction, PpuMapResult, PpuWriteAction};

/// 地址翻译器 — 将 CPU/PPU 地址映射到具体存储或寄存器
pub trait AddressMapper {
    /// 映射 CPU 读取地址
    fn map_cpu_read(&self, addr: u16) -> CpuMapResult;

    /// 映射 CPU 写入地址
    fn map_cpu_write(&self, addr: u16, value: u8) -> CpuWriteAction;

    /// 映射 PPU 读取地址
    fn map_ppu_read(&self, addr: u16) -> PpuMapResult;

    /// 映射 PPU 写入地址
    fn map_ppu_write(&self, addr: u16, value: u8) -> PpuWriteAction;
}
