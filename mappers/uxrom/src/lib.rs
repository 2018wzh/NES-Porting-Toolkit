//! mapper-uxrom — UxROM (Mapper 2) 实现
//!
//! UxROM 提供 16 KiB PRG bank switching。
//! - CPU $8000-$BFFF: 可切换的 16KB PRG bank
//! - CPU $C000-$FFFF: 固定到最后一个 16KB PRG bank
//! - CPU 写 $8000-$FFFF: 选择 PRG bank（低 4 位有效）
//! - CHR: 固定 CHR-ROM 或 CHR-RAM（无 bank switching）

use std::cell::RefCell;
use std::rc::Rc;

use nptk_core::mapper::types::{IrqState, MapperDebugInfo, MapperSaveState, PpuBusEvent};
use nptk_core::mapper::{MapperChip, MapperContext};
use nptk_core::rom::Mirroring;
use nptk_core::rom::NesRom;

/// UxROM (Mapper 2) 实现
pub struct Mapper002Uxrom {
    selected_prg_bank: usize,
    prg_bank_count: usize,
    mirroring: Mirroring,
    irq_state: IrqState,
}

impl Mapper002Uxrom {
    pub fn new(rom: &NesRom) -> Self {
        let prg_len = rom.prg_rom.len();
        let prg_bank_count = prg_len / 0x4000; // 16KB banks
        Mapper002Uxrom {
            selected_prg_bank: 0,
            prg_bank_count: prg_bank_count.max(1),
            mirroring: rom.header.mirroring,
            irq_state: IrqState::Inactive,
        }
    }
}

impl MapperChip for Mapper002Uxrom {
    fn mapper_id(&self) -> u16 {
        2
    }

    fn name(&self) -> &'static str {
        "UxROM"
    }

    fn cpu_read(&mut self, ctx: &Rc<RefCell<MapperContext>>, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xBFFF => {
                let ctx = ctx.borrow();
                let prg = &ctx.prg_rom;
                if prg.is_empty() {
                    return Some(0);
                }
                let bank = self.selected_prg_bank % self.prg_bank_count;
                let offset = bank * 0x4000 + (addr as usize - 0x8000);
                Some(prg[offset.min(prg.len() - 1)])
            }
            0xC000..=0xFFFF => {
                let ctx = ctx.borrow();
                let prg = &ctx.prg_rom;
                if prg.is_empty() {
                    return Some(0);
                }
                let bank = self.prg_bank_count - 1;
                let offset = bank * 0x4000 + (addr as usize - 0xC000);
                Some(prg[offset.min(prg.len() - 1)])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, _ctx: &Rc<RefCell<MapperContext>>, addr: u16, value: u8) -> bool {
        match addr {
            0x8000..=0xFFFF => {
                self.selected_prg_bank = (value & 0x0F) as usize;
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, ctx: &Rc<RefCell<MapperContext>>, addr: u16) -> Option<u8> {
        match addr {
            0x0000..=0x1FFF => {
                let ctx = ctx.borrow();
                Some(ctx.chr.read(addr))
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
            "selected_prg_bank": self.selected_prg_bank,
        });
        MapperSaveState { mapper_id: 2, data }
    }

    fn load_state(&mut self, state: &MapperSaveState) {
        if let Some(bank) = state.data.get("selected_prg_bank").and_then(|v| v.as_u64()) {
            self.selected_prg_bank = bank as usize;
        }
    }

    fn debug_info(&self) -> MapperDebugInfo {
        MapperDebugInfo {
            registers: vec![("PRG Bank".into(), format!("{}", self.selected_prg_bank))],
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

    /// 创建 32KB PRG-ROM 的测试 ROM（2 个 bank）
    fn make_rom_32k() -> NesRom {
        let mut data = vec![0u8; 16 + 32768 + 8192];
        data[0..4].copy_from_slice(b"NES\x1a");
        data[4] = 2; // 2 × 16KB PRG
        data[5] = 1; // 1 × 8KB CHR
        // Bank 0: fill with 0xAA
        for i in 0..0x4000 {
            data[0x10 + i] = 0xAA;
        }
        // Bank 1: fill with 0xBB
        for i in 0..0x4000 {
            data[0x10 + 0x4000 + i] = 0xBB;
        }
        parse_rom(&data).unwrap()
    }

    #[test]
    fn test_mapper_id() {
        let rom = make_rom_32k();
        let mapper = Mapper002Uxrom::new(&rom);
        assert_eq!(mapper.mapper_id(), 2);
        assert_eq!(mapper.name(), "UxROM");
    }

    #[test]
    fn test_default_bank_0() {
        let rom = make_rom_32k();
        let mut mapper = Mapper002Uxrom::new(&rom);
        let ctx = MapperContext::new(
            rom.prg_rom.clone(),
            ChrStorage::Rom(rom.chr_rom.clone().unwrap_or_default()),
            Box::new(NullEventSink),
        )
        .into_rc();

        // 默认 bank 0 → 读取 0xAA
        assert_eq!(mapper.cpu_read(&ctx, 0x8000), Some(0xAA));
        // 固定 bank (最后一个) → 读取 0xBB
        assert_eq!(mapper.cpu_read(&ctx, 0xC000), Some(0xBB));
    }

    #[test]
    fn test_switch_to_bank_1() {
        let rom = make_rom_32k();
        let mut mapper = Mapper002Uxrom::new(&rom);
        let ctx = MapperContext::new(
            rom.prg_rom.clone(),
            ChrStorage::Rom(rom.chr_rom.clone().unwrap_or_default()),
            Box::new(NullEventSink),
        )
        .into_rc();

        // 切换到 bank 1
        mapper.cpu_write(&ctx, 0x8000, 0x01);
        assert_eq!(mapper.cpu_read(&ctx, 0x8000), Some(0xBB));
    }

    #[test]
    fn test_fixed_bank_unchanged() {
        let rom = make_rom_32k();
        let mut mapper = Mapper002Uxrom::new(&rom);
        let ctx = MapperContext::new(
            rom.prg_rom.clone(),
            ChrStorage::Rom(rom.chr_rom.clone().unwrap_or_default()),
            Box::new(NullEventSink),
        )
        .into_rc();

        // 切换 bank 不应影响固定 bank
        mapper.cpu_write(&ctx, 0x8000, 0x00);
        assert_eq!(mapper.cpu_read(&ctx, 0xC000), Some(0xBB));
    }
}
