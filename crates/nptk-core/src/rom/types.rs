//! ROM 类型定义
//! 定义 iNES / NES 2.0 header 结构和镜像模式等

/// 镜像模式
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mirroring {
    Horizontal,
    Vertical,
    FourScreen,
    /// 单屏幕 A（mapper 控制）
    ScreenAOnly,
    /// 单屏幕 B（mapper 控制）
    ScreenBOnly,
    /// Mapper 动态控制镜像模式
    MapperControlled,
}

/// ROM 格式
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RomFormat {
    Ines,
    Nes20,
}

/// iNES / NES 2.0 header
#[derive(Debug, Clone)]
pub struct RomHeader {
    pub format: RomFormat,
    pub prg_rom_size: usize, // 16KB units
    pub chr_rom_size: usize, // 8KB units
    pub mapper_id: u16,
    pub submapper_id: u8,
    pub mirroring: Mirroring,
    pub has_sram: bool,
    pub has_trainer: bool,
    pub vs_unisystem: bool,
    pub console_type: u8,
    pub input_type: u8,
}

/// 解析后的 NES ROM
#[derive(Debug, Clone)]
pub struct NesRom {
    pub header: RomHeader,
    pub trainer: Option<[u8; 512]>,
    pub prg_rom: Vec<u8>,
    pub chr_rom: Option<Vec<u8>>,
    pub has_chr_ram: bool,
    pub raw: Vec<u8>,
}

/// ROM 解析错误
#[derive(Debug)]
pub enum RomError {
    TooSmall(usize),
    InvalidSignature,
    InvalidFormat(String),
    TrainerOnlyInInes,
    MapperNotSupported(u16),
}

impl core::fmt::Display for RomError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            RomError::TooSmall(s) => write!(f, "ROM too small: {} bytes", s),
            RomError::InvalidSignature => write!(f, "invalid iNES/NES2.0 signature"),
            RomError::InvalidFormat(msg) => write!(f, "invalid ROM format: {}", msg),
            RomError::TrainerOnlyInInes => write!(f, "trainer only valid in iNES format"),
            RomError::MapperNotSupported(m) => write!(f, "mapper {} not supported", m),
        }
    }
}

impl std::error::Error for RomError {}
