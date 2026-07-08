//! 地址映射结果枚举
//!
//! 用于 `AddressMapper` trait 的返回值，描述 CPU/PPU 地址经过 mapper
//! 翻译后的具体映射目标。

/// CPU 读取地址映射结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuMapResult {
    /// 映射到 PRG-ROM 的指定偏移
    PrgRom { offset: usize },
    /// 映射到 PRG-RAM 的指定偏移
    PrgRam { offset: usize },
    /// 映射到 Mapper 内部寄存器（返回寄存器值）
    MapperRegister { value: u8 },
    /// Open bus（无有效映射）
    OpenBus,
    /// 未映射到此 mapper 的地址范围
    NotMapped,
}

/// CPU 写入地址映射结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuWriteAction {
    /// 写入 PRG-RAM 的指定偏移
    WritePrgRam { offset: usize, value: u8 },
    /// 更新 Mapper 内部寄存器
    UpdateRegister,
    /// 忽略写入
    Ignore,
    /// 未映射到此 mapper 的地址范围
    NotMapped,
}

/// PPU 读取地址映射结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PpuMapResult {
    /// 映射到 CHR-ROM 的指定偏移
    ChrRom { offset: usize },
    /// 映射到 CHR-RAM 的指定偏移
    ChrRam { offset: usize },
    /// 映射到 VRAM（nametable）
    Vram { nametable: usize, offset: usize },
    /// 映射到调色板
    Palette,
    /// 未映射到此 mapper 的地址范围
    NotMapped,
}

/// PPU 写入地址映射结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PpuWriteAction {
    /// 写入 CHR-RAM 的指定偏移
    WriteChrRam { offset: usize, value: u8 },
    /// 写入 VRAM（nametable）
    WriteVram {
        nametable: usize,
        offset: usize,
        value: u8,
    },
    /// 忽略写入
    Ignore,
    /// 未映射到此 mapper 的地址范围
    NotMapped,
}
