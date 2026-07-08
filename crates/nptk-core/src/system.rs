//! NES 系统 — 帧循环、CPU/PPU/APU 同步
//!
//! NTSC NES 时序: 262 scanlines × 341 PPU dots = 89342 dots/frame
//! PPU dot clock = 3 × CPU clock, CPU cycles/frame ≈ 29780

use crate::bus::{NesBus, NesBusImpl};
use crate::mapper::Cartridge;

pub const CPU_CYCLES_PER_FRAME: u32 = 341 * 262 / 3;

pub struct NesSystem {
    pub bus: NesBusImpl,
    pub cpu: crate::cpu_ref::Cpu6502,
    pub frame_count: u64,
    pub cpu_cycle: u32,
    pub ppu_dot: u32,
}

impl NesSystem {
    pub fn new(mut bus: NesBusImpl) -> Self {
        let mut cpu = crate::cpu_ref::Cpu6502::new();
        cpu.reset(&mut bus);
        NesSystem {
            bus,
            cpu,
            frame_count: 0,
            cpu_cycle: 0,
            ppu_dot: 0,
        }
    }

    /// 从 Cartridge 创建 NesSystem（便捷构造函数）
    pub fn from_cartridge(cartridge: Cartridge) -> Self {
        let bus = NesBusImpl::new(cartridge);
        Self::new(bus)
    }

    /// 执行一帧，返回 framebuffer
    pub fn run_frame(&mut self) -> &[u8; 256 * 240] {
        self.bus.ppu.clear_frame_complete();

        // NMI 由 PPU tick() 在 VBlank 开始时设置 has_nmi，
        // 本帧开头执行上一帧触发的 NMI
        if self.cpu.nmi_pending {
            self.cpu.nmi_pending = false;
            self.cpu.trigger_nmi(&mut self.bus);
        }

        self.cpu_cycle = 0;
        self.ppu_dot = 0;

        while self.cpu_cycle < CPU_CYCLES_PER_FRAME {
            let cycles = self.cpu.step(&mut self.bus) as u32;
            self.cpu_cycle += cycles;
            self.bus.tick_cpu(cycles);
            self.ppu_dot = self.ppu_dot.wrapping_add(cycles * 3);

            // 检查 PPU 是否触发了 NMI
            if self.bus.ppu.take_nmi() {
                self.cpu.nmi_pending = true;
            }
        }

        // 帧结束：渲染 PPU 帧
        self.bus.render_ppu_frame();
        self.frame_count += 1;
        self.bus.ppu.frame()
    }

    pub fn step_cpu(&mut self) -> u32 {
        let c = self.cpu.step(&mut self.bus);
        self.cpu_cycle += c;
        self.bus.tick_cpu(c);
        c
    }

    pub fn ram(&self) -> &[u8; 0x800] {
        &self.bus.ram
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mapper::{
        Cartridge, CartridgeMetadata, ChrStorage, MapperChip, MapperContext, MapperSaveState,
    };
    use crate::rom::parse_rom;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Minimal MapperChip for testing
    struct TestMapper {
        mirroring: crate::rom::Mirroring,
    }
    impl MapperChip for TestMapper {
        fn mapper_id(&self) -> u16 {
            0
        }
        fn name(&self) -> &'static str {
            "Test"
        }
        fn cpu_read(&mut self, ctx: &Rc<RefCell<MapperContext>>, addr: u16) -> Option<u8> {
            match addr {
                0x8000..=0xFFFF => {
                    let ctx = ctx.borrow();
                    let prg = &ctx.prg_rom;
                    if prg.is_empty() {
                        return Some(0);
                    }
                    Some(prg[(addr as usize - 0x8000) % prg.len()])
                }
                _ => None,
            }
        }
        fn cpu_write(&mut self, _ctx: &Rc<RefCell<MapperContext>>, _addr: u16, _value: u8) -> bool {
            false
        }
        fn ppu_read(&mut self, _ctx: &Rc<RefCell<MapperContext>>, _addr: u16) -> Option<u8> {
            None
        }
        fn ppu_write(&mut self, _ctx: &Rc<RefCell<MapperContext>>, _addr: u16, _value: u8) -> bool {
            false
        }
        fn mirroring(&self) -> crate::rom::Mirroring {
            self.mirroring
        }
        fn save_state(&self) -> MapperSaveState {
            MapperSaveState::new(0)
        }
        fn load_state(&mut self, _state: &MapperSaveState) {}
    }

    fn make_test_system() -> NesSystem {
        let mut data = vec![0u8; 16 + 16384 + 8192];
        data[0..4].copy_from_slice(b"NES\x1a");
        data[4] = 1;
        data[5] = 1;
        let prg = 0x10;
        // LDA #$01, STA $51, JMP $8000
        data[prg..prg + 7].copy_from_slice(&[0xA9, 0x01, 0x85, 0x51, 0x4C, 0x00, 0x80]);
        data[prg + 0x3FFC] = 0x00;
        data[prg + 0x3FFD] = 0x80;
        let rom = parse_rom(&data).unwrap();
        let cartridge = Cartridge::new_simple(
            CartridgeMetadata {
                mapper_id: 0,
                submapper_id: 0,
                prg_rom_size: 1,
                chr_rom_size: 1,
                has_sram: false,
                has_trainer: false,
                battery_backed: false,
            },
            rom.prg_rom.clone(),
            ChrStorage::Rom(rom.chr_rom.clone().unwrap_or_default()),
            Box::new(TestMapper {
                mirroring: rom.header.mirroring,
            }),
        );
        NesSystem::from_cartridge(cartridge)
    }

    #[test]
    fn test_frame_loop_runs() {
        let mut sys = make_test_system();
        let _ = sys.run_frame();
        assert!(sys.cpu_cycle > 100);
    }

    #[test]
    fn test_cpu_writes_ram() {
        let mut sys = make_test_system();
        for _ in 0..5 {
            sys.step_cpu();
        }
        assert_eq!(sys.ram()[0x0051], 1);
    }

    #[test]
    fn test_nmi_fires() {
        let mut sys = make_test_system();
        // Enable NMI in PPU control register ($2000)
        sys.bus.cpu_write(0x2000, 0x80);
        assert!(!sys.cpu.nmi_pending);
        let _ = sys.run_frame();
        // After one frame, NMI should be pending for next frame
        assert!(sys.cpu.nmi_pending);
    }
}
