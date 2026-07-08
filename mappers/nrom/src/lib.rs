//! mapper-nrom — NROM (Mapper 0) 实现
//!
//! NROM 是最简单的 NES mapper，没有 bank switching。
//! - 16KB PRG: $8000-$BFFF = first 16KB, $C000-$FFFF = same (mirrored)
//! - 32KB PRG: $8000-$FFFF 直接映射
//! - CHR: 直接映射 (CHR-ROM 或 CHR-RAM)

use std::cell::RefCell;
use std::rc::Rc;

use nptk_core::mapper::types::{
    ChrStorage, IrqState, MapperDebugInfo, MapperSaveState, PpuBusEvent,
};
use nptk_core::mapper::{MapperChip, MapperContext};
use nptk_core::rom::Mirroring;
use nptk_core::rom::NesRom;

/// NROM (Mapper 0) 实现
pub struct Mapper000Nrom {
    mirroring: Mirroring,
    is_16k_prg: bool,
    irq_state: IrqState,
}

impl Mapper000Nrom {
    pub fn new(rom: &NesRom) -> Self {
        let prg_len = rom.prg_rom.len();
        Mapper000Nrom {
            mirroring: rom.header.mirroring,
            is_16k_prg: prg_len <= 16_384,
            irq_state: IrqState::Inactive,
        }
    }
}

impl MapperChip for Mapper000Nrom {
    fn mapper_id(&self) -> u16 {
        0
    }

    fn name(&self) -> &'static str {
        "NROM"
    }

    fn cpu_read(&mut self, ctx: &Rc<RefCell<MapperContext>>, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => {
                let ctx = ctx.borrow();
                let prg = &ctx.prg_rom;
                if prg.is_empty() {
                    return Some(0);
                }
                let offset = if self.is_16k_prg {
                    (addr as usize - 0x8000) & 0x3FFF
                } else {
                    (addr as usize - 0x8000) & 0x7FFF
                };
                Some(prg[offset.min(prg.len() - 1)])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, _ctx: &Rc<RefCell<MapperContext>>, _addr: u16, _value: u8) -> bool {
        // NROM 没有 PRG 写入
        false
    }

    fn ppu_read(&mut self, ctx: &Rc<RefCell<MapperContext>>, addr: u16) -> Option<u8> {
        match addr {
            0x0000..=0x1FFF => {
                let ctx = ctx.borrow();
                match &ctx.chr {
                    ChrStorage::Rom(chr) => {
                        if chr.is_empty() {
                            Some(0)
                        } else {
                            Some(chr[addr as usize % chr.len()])
                        }
                    }
                    ChrStorage::Ram(chr) => {
                        if chr.is_empty() {
                            Some(0)
                        } else {
                            Some(chr[addr as usize % chr.len()])
                        }
                    }
                }
            }
            _ => None,
        }
    }

    fn ppu_write(&mut self, ctx: &Rc<RefCell<MapperContext>>, addr: u16, value: u8) -> bool {
        match addr {
            0x0000..=0x1FFF => {
                let mut ctx = ctx.borrow_mut();
                ctx.chr.write(addr, value)
            }
            _ => false,
        }
    }

    fn cpu_tick(&mut self, _ctx: &Rc<RefCell<MapperContext>>, _cycles: u32) {}

    fn ppu_tick(&mut self, _ctx: &Rc<RefCell<MapperContext>>, _event: PpuBusEvent) {}

    fn irq_state(&self) -> IrqState {
        self.irq_state
    }

    fn clear_irq(&mut self) {
        self.irq_state = IrqState::Inactive;
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> MapperSaveState {
        MapperSaveState::new(0)
    }

    fn load_state(&mut self, _state: &MapperSaveState) {}

    fn debug_info(&self) -> MapperDebugInfo {
        MapperDebugInfo::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nptk_core::mapper::context::MapperContext;
    use nptk_core::mapper::event_sink::NullEventSink;
    use nptk_core::mapper::types::ChrStorage;
    use nptk_core::rom::parse_rom;

    fn make_rom() -> NesRom {
        let mut data = vec![0u8; 16 + 16384 + 8192];
        data[0..4].copy_from_slice(b"NES\x1a");
        data[4] = 1;
        data[5] = 1;
        // 写入一些测试数据到 PRG-ROM
        data[0x10] = 0xA9; // LDA #$42
        data[0x11] = 0x42;
        data[0x10 + 0x3FFC] = 0x00;
        data[0x10 + 0x3FFD] = 0x80;
        parse_rom(&data).unwrap()
    }

    #[test]
    fn test_mapper_id() {
        let rom = make_rom();
        let mapper = Mapper000Nrom::new(&rom);
        assert_eq!(mapper.mapper_id(), 0);
        assert_eq!(mapper.name(), "NROM");
    }

    #[test]
    fn test_cpu_read_prg() {
        let rom = make_rom();
        let mut mapper = Mapper000Nrom::new(&rom);
        let ctx = MapperContext::new(
            rom.prg_rom.clone(),
            ChrStorage::Rom(rom.chr_rom.clone().unwrap_or_default()),
            Box::new(NullEventSink),
        )
        .into_rc();

        // 读取 PRG-ROM 区域
        let val = mapper.cpu_read(&ctx, 0x8000);
        assert_eq!(val, Some(0xA9));
    }

    #[test]
    fn test_cpu_read_unmapped() {
        let rom = make_rom();
        let mut mapper = Mapper000Nrom::new(&rom);
        let ctx = MapperContext::new(
            rom.prg_rom.clone(),
            ChrStorage::Rom(rom.chr_rom.clone().unwrap_or_default()),
            Box::new(NullEventSink),
        )
        .into_rc();

        // $6000 不应被 NROM 映射
        assert_eq!(mapper.cpu_read(&ctx, 0x6000), None);
    }

    #[test]
    fn test_ppu_read_chr() {
        let rom = make_rom();
        let mut mapper = Mapper000Nrom::new(&rom);
        let chr_data = vec![0x01, 0x02, 0x03, 0x04];
        let ctx = MapperContext::new(
            rom.prg_rom.clone(),
            ChrStorage::Rom(chr_data),
            Box::new(NullEventSink),
        )
        .into_rc();

        assert_eq!(mapper.ppu_read(&ctx, 0x0000), Some(0x01));
        assert_eq!(mapper.ppu_read(&ctx, 0x0001), Some(0x02));
    }
}
