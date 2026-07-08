//! Mapper 核心类型定义
//!
//! 包含 NES 区域、CHR 存储、PRG-RAM、IRQ 状态、PPU 总线事件等类型。

use serde::{Deserialize, Serialize};

/// NES 区域类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NesRegion {
    Ntsc,
    Pal,
    Dendy,
}

/// CHR 存储类型
#[derive(Debug, Clone)]
pub enum ChrStorage {
    /// CHR-ROM（只读）
    Rom(Vec<u8>),
    /// CHR-RAM（可读写）
    Ram(Vec<u8>),
}

impl ChrStorage {
    /// 读取指定地址的字节
    pub fn read(&self, addr: u16) -> u8 {
        match self {
            ChrStorage::Rom(data) | ChrStorage::Ram(data) => {
                let len = data.len();
                if len == 0 {
                    return 0;
                }
                data[(addr as usize) % len]
            }
        }
    }

    /// 写入指定地址的字节（仅 CHR-RAM 有效）
    pub fn write(&mut self, addr: u16, value: u8) -> bool {
        match self {
            ChrStorage::Ram(data) => {
                let len = data.len();
                if len == 0 {
                    return false;
                }
                data[(addr as usize) % len] = value;
                true
            }
            ChrStorage::Rom(_) => false,
        }
    }

    /// 是否为 RAM
    pub fn is_ram(&self) -> bool {
        matches!(self, ChrStorage::Ram(_))
    }

    /// 获取内部数据引用
    pub fn data(&self) -> &[u8] {
        match self {
            ChrStorage::Rom(data) | ChrStorage::Ram(data) => data,
        }
    }

    /// 获取内部可变数据引用（仅 CHR-RAM）
    pub fn data_mut(&mut self) -> Option<&mut Vec<u8>> {
        match self {
            ChrStorage::Ram(data) => Some(data),
            ChrStorage::Rom(_) => None,
        }
    }
}

/// PRG-RAM / SRAM 存储
#[derive(Debug, Clone)]
pub struct PrgRam {
    pub data: Vec<u8>,
    pub battery_backed: bool,
    pub writable: bool,
}

impl PrgRam {
    pub fn new(size: usize) -> Self {
        PrgRam {
            data: vec![0; size],
            battery_backed: false,
            writable: true,
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        if self.data.is_empty() {
            return 0;
        }
        self.data[(addr as usize) % self.data.len()]
    }

    pub fn write(&mut self, addr: u16, value: u8) -> bool {
        if !self.writable || self.data.is_empty() {
            return false;
        }
        let len = self.data.len();
        self.data[(addr as usize) % len] = value;
        true
    }
}

impl Default for PrgRam {
    fn default() -> Self {
        PrgRam::new(0x2000) // 8KB default
    }
}

/// IRQ 状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrqState {
    Inactive,
    Active,
}

impl IrqState {
    pub fn is_active(&self) -> bool {
        matches!(self, IrqState::Active)
    }
}

/// PPU 访问类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PpuAccessKind {
    Read,
    Write,
    Idle,
}

/// PPU 总线事件 — 用于通知 Mapper 的 PPU 地址线变化
///
/// 一些 Mapper（如 MMC3）需要观察 PPU 的 A12 地址线上升沿来驱动
/// scanline IRQ 计数器。
#[derive(Debug, Clone, Copy)]
pub struct PpuBusEvent {
    pub frame: u64,
    pub scanline: i16,
    pub dot: u16,
    pub addr: u16,
    pub access: PpuAccessKind,
}

/// Mapper 保存状态 — 用于序列化/反序列化
///
/// 初始实现使用 serde_json::Value 作为兜底格式，后续可为每个
/// mapper 定义具体的序列化结构。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapperSaveState {
    pub mapper_id: u16,
    pub data: serde_json::Value,
}

impl MapperSaveState {
    pub fn new(mapper_id: u16) -> Self {
        MapperSaveState {
            mapper_id,
            data: serde_json::Value::Object(Default::default()),
        }
    }
}

/// Mapper 调试信息
#[derive(Debug, Clone, Default)]
pub struct MapperDebugInfo {
    pub registers: Vec<(String, String)>,
    pub banks: Vec<(String, String)>,
    pub extra: Vec<(String, String)>,
}
