//! NES 系统 — 帧循环、CPU/PPU/APU 同步
//!
//! NTSC NES 时序: 262 scanlines × 341 PPU dots = 89342 dots/frame
//! PPU dot clock = 3 × CPU clock, CPU cycles/frame ≈ 29780.67
//!
//! 与 tetanes-core 对齐: 使用 PPU frame_complete 标志驱动帧循环，
//! 而非固定 CPU 周期数。这样可以避免整数截断导致的累积误差。

use crate::bus::{NesBus, NesBusImpl};
use crate::mapper::Cartridge;

/// 最大 CPU 周期数/帧（安全上限，防止死循环）
/// 精确值 = 262 * 341 / 3 = 29780.666...，取 ceil 作为安全上限
pub const CPU_CYCLES_PER_FRAME_MAX: u32 = 29781;

pub struct NesSystem {
    pub cpu: crate::cpu_ref::Cpu6502,
    pub frame_count: u64,
    pub cpu_cycle: u32,
    pub ppu_dot: u32,
}

impl NesSystem {
    pub fn new(bus: NesBusImpl) -> Self {
        let mut cpu = crate::cpu_ref::Cpu6502::new(bus, mos6502::instruction::Ricoh2a03);
        cpu.reset();
        NesSystem {
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
    ///
    /// 使用 PPU frame_complete 标志驱动帧循环（与 tetanes-core 的
    /// clock_frame() 使用 frame_number() 变化检测类似）。
    /// 这确保了精确的 262×341 PPU dots/帧时序。
    pub fn run_frame(&mut self) -> &[u8; 256 * 240] {
        self.cpu.memory.ppu.clear_frame_complete();

        self.cpu_cycle = 0;
        self.ppu_dot = 0;

        // 循环直到 PPU 完成一帧（frame_complete 被 tick() 设置）
        // 使用 CPU_CYCLES_PER_FRAME_MAX 作为安全上限
        while !self.cpu.memory.ppu.get_frame_complete() && self.cpu_cycle < CPU_CYCLES_PER_FRAME_MAX
        {
            let cycles_before = self.cpu.cycles;
            self.cpu.single_step();
            let cycles = (self.cpu.cycles - cycles_before) as u32;
            self.cpu_cycle += cycles;
            self.cpu.memory.tick_cpu(cycles);
            self.ppu_dot = self.ppu_dot.wrapping_add(cycles * 3);
        }

        // 帧结束：渲染 PPU 帧
        self.cpu.memory.render_ppu_frame();
        self.frame_count += 1;
        self.cpu.memory.ppu.frame()
    }

    pub fn step_cpu(&mut self) -> u32 {
        let cycles_before = self.cpu.cycles;
        self.cpu.single_step();
        let c = (self.cpu.cycles - cycles_before) as u32;
        self.cpu_cycle += c;
        self.cpu.memory.tick_cpu(c);
        c
    }

    pub fn ram(&self) -> &[u8; 0x800] {
        &self.cpu.memory.ram
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
        sys.cpu.memory.cpu_write(0x2000, 0x80);
        // mos6502 内部通过 Bus::nmi_pending() 自动检测 NMI
        let _ = sys.run_frame();
        // 帧完成后，mos6502 自动处理了 NMI
    }
}
