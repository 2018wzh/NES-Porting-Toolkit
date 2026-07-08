//! mapper-cnrom — CNROM (Mapper 3) 实现
//!
//! CNROM 提供 8 KiB CHR bank switching。
//! - CPU $8000-$FFFF: 写入值选择 CHR bank（低 2 位有效）
//! - PRG: 固定 32KB PRG-ROM（无 bank switching）
//! - CHR: 通过 bank 选择切换 8KB CHR-ROM bank

use std::cell::RefCell;
use std::rc::Rc;

use nptk_core::mapper::types::{
    ChrStorage, IrqState, MapperDebugInfo, MapperSaveState, PpuBusEvent,
};
use nptk_core::mapper::{MapperChip, MapperContext};
use nptk_core::rom::Mirroring;
use nptk_core::rom::NesRom;

/// CNROM (Mapper 3) 实现
pub struct Mapper003Cnrom {
    selected_chr_bank: usize,
    chr_bank_count: usize,
    mirroring: Mirroring,
    irq_state: IrqState,
}

impl Mapper003Cnrom {
    pub fn new(rom: &NesRom) -> Self {
        let chr_len = rom.chr_rom.as_ref().map(|c| c.len()).unwrap_or(0);
        let chr_bank_count = (chr_len / 0x2000).max(1); // 8KB banks
        Mapper003Cnrom {
            selected_chr_bank: 0,
            chr_bank_count,
            mirroring: rom.header.mirroring,
            irq_state: IrqState::Inactive,
        }
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let bank = self.selected_chr_bank % self.chr_bank_count;
        bank * 0x2000 + (addr as usize)
    }
}

impl MapperChip for Mapper003Cnrom {
    fn mapper_id(&self) -> u16 {
        3
    }

    fn name(&self) -> &'static str {
        "CNROM"
    }

    fn cpu_read(&mut self, ctx: &Rc<RefCell<MapperContext>>, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => {
                let ctx = ctx.borrow();
                let prg = &ctx.prg_rom;
                if prg.is_empty() {
                    return Some(0);
                }
                let offset = (addr as usize - 0x8000) & 0x7FFF;
                Some(prg[offset.min(prg.len() - 1)])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, _ctx: &Rc<RefCell<MapperContext>>, addr: u16, value: u8) -> bool {
        match addr {
            0x8000..=0xFFFF => {
                // 低 2 位选择 CHR bank（部分实现使用更多位）
                self.selected_chr_bank = (value & 0x03) as usize;
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, ctx: &Rc<RefCell<MapperContext>>, addr: u16) -> Option<u8> {
        match addr {
            0x0000..=0x1FFF => {
                let ctx = ctx.borrow();
                match &ctx.chr {
                    ChrStorage::Rom(chr) => {
                        if chr.is_empty() {
                            return Some(0);
                        }
                        let offset = self.chr_offset(addr);
                        Some(chr[offset % chr.len()])
                    }
                    ChrStorage::Ram(chr) => {
                        if chr.is_empty() {
                            return Some(0);
                        }
                        Some(chr[addr as usize % chr.len()])
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
        let data = serde_json::json!({
            "selected_chr_bank": self.selected_chr_bank,
        });
        MapperSaveState { mapper_id: 3, data }
    }

    fn load_state(&mut self, state: &MapperSaveState) {
        if let Some(bank) = state.data.get("selected_chr_bank").and_then(|v| v.as_u64()) {
            self.selected_chr_bank = bank as usize;
        }
    }

    fn debug_info(&self) -> MapperDebugInfo {
        MapperDebugInfo {
            registers: vec![("CHR Bank".into(), format!("{}", self.selected_chr_bank))],
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nptk_core::mapper::context::MapperContext;
    use nptk_core::mapper::event_sink::NullEventSink;
    use nptk_core::mapper::types::ChrStorage;
    use nptk_core::rom::parse_rom;

    /// 创建 16KB CHR-ROM 的测试 ROM（2 个 bank）
    fn make_rom_16k_chr() -> NesRom {
        let mut data = vec![0u8; 16 + 32768 + 16384];
        data[0..4].copy_from_slice(b"NES\x1a");
        data[4] = 2; // 2 × 16KB PRG
        data[5] = 2; // 2 × 8KB CHR
        // Bank 0: fill with 0xAA
        for i in 0..0x2000 {
            data[0x10 + 0x8000 + i] = 0xAA;
        }
        // Bank 1: fill with 0xBB
        for i in 0..0x2000 {
            data[0x10 + 0x8000 + 0x2000 + i] = 0xBB;
        }
        parse_rom(&data).unwrap()
    }

    #[test]
    fn test_mapper_id() {
        let rom = make_rom_16k_chr();
        let mapper = Mapper003Cnrom::new(&rom);
        assert_eq!(mapper.mapper_id(), 3);
        assert_eq!(mapper.name(), "CNROM");
    }

    #[test]
    fn test_default_chr_bank_0() {
        let rom = make_rom_16k_chr();
        let mut mapper = Mapper003Cnrom::new(&rom);
        let chr_data = vec![0xAA; 0x2000]; // bank 0
        let ctx = MapperContext::new(
            rom.prg_rom.clone(),
            ChrStorage::Rom(chr_data),
            Box::new(NullEventSink),
        )
        .into_rc();

        assert_eq!(mapper.ppu_read(&ctx, 0x0000), Some(0xAA));
    }

    #[test]
    fn test_switch_chr_bank() {
        let rom = make_rom_16k_chr();
        let mut mapper = Mapper003Cnrom::new(&rom);
        // 创建 2 个 bank 的 CHR-ROM
        let mut chr_data = vec![0xAA; 0x2000];
        chr_data.extend_from_slice(&[0xBB; 0x2000]);
        let ctx = MapperContext::new(
            rom.prg_rom.clone(),
            ChrStorage::Rom(chr_data),
            Box::new(NullEventSink),
        )
        .into_rc();

        // 默认 bank 0
        assert_eq!(mapper.ppu_read(&ctx, 0x0000), Some(0xAA));

        // 切换到 bank 1
        mapper.cpu_write(&ctx, 0x8000, 0x01);
        assert_eq!(mapper.ppu_read(&ctx, 0x0000), Some(0xBB));
    }

    #[test]
    fn test_cpu_read_prg() {
        let rom = make_rom_16k_chr();
        let mut mapper = Mapper003Cnrom::new(&rom);
        let ctx = MapperContext::new(
            rom.prg_rom.clone(),
            ChrStorage::Rom(rom.chr_rom.clone().unwrap_or_default()),
            Box::new(NullEventSink),
        )
        .into_rc();

        // PRG 读取应正常工作
        assert!(mapper.cpu_read(&ctx, 0x8000).is_some());
    }
}
