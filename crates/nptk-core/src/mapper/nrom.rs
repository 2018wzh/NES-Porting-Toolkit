//! Mapper 0: NROM
//!
//! 最简单的 NES mapper，没有 bank switching。
//! - 16KB PRG: $8000-$BFFF = first 16KB, $C000-$FFFF = same (mirrored)
//! - 32KB PRG: $8000-$FFFF 直接映射
//! - CHR: 直接映射 (CHR-ROM 或 CHR-RAM)

use super::Mapper;
use crate::rom::{Mirroring, NesRom};

pub struct Mapper0Nrom {
    prg: Vec<u8>,
    chr: Vec<u8>,
    has_chr_ram: bool,
    mirroring: Mirroring,
    is_16k_prg: bool,
}

impl Mapper0Nrom {
    pub fn new(rom: &NesRom) -> Self {
        let prg = rom.prg_rom.clone();
        let is_16k = prg.len() <= 16_384;
        let chr = rom.chr_rom.clone().unwrap_or_else(|| vec![0u8; 8_192]);
        Mapper0Nrom {
            prg,
            chr,
            has_chr_ram: rom.has_chr_ram,
            mirroring: rom.header.mirroring,
            is_16k_prg: is_16k,
        }
    }

    fn read_prg(&self, addr: u16) -> u8 {
        let addr = addr as usize;
        if self.is_16k_prg {
            // 16KB: $8000-$BFFF = prg[0..16384], $C000-$FFFF = prg[0..16384] (mirror)
            let offset = addr & 0x3FFF;
            self.prg[offset.min(self.prg.len().saturating_sub(1))]
        } else {
            // 32KB: $8000-$FFFF = prg[0..32768]
            let offset = addr & 0x7FFF;
            self.prg[offset.min(self.prg.len().saturating_sub(1))]
        }
    }

    fn write_prg(&mut self, addr: u16, value: u8) -> bool {
        let _ = (addr, value, self);
        // NROM 没有 PRG 写入（可能用于部分 PRG-RAM 扩展，但 NROM 标准不支持）
        false
    }
}

impl Mapper for Mapper0Nrom {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => Some(self.read_prg(addr)),
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) -> bool {
        self.write_prg(addr, value)
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if self.has_chr_ram {
            return None; // CHR-RAM: 由 bus 处理
        }
        match addr {
            0x0000..=0x1FFF => Some(self.chr[addr as usize % self.chr.len()]),
            _ => None,
        }
    }

    fn ppu_write(&mut self, _addr: u16, _value: u8) -> bool {
        if self.has_chr_ram {
            // CHR-RAM: 允许写入 pattern table
            return true;
        }
        false
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn mapper_id(&self) -> u16 {
        0
    }
}
