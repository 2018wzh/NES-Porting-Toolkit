//! NES 总线抽象
//!
//! CPU 地址空间:
//!   $0000-$07FF: 2KB internal RAM
//!   $0800-$1FFF: RAM mirrors (mirrors every 0x800)
//!   $2000-$2007: PPU registers (mirrored $2008-$3FFF)
//!   $4000-$4017: APU & I/O registers
//!   $4018-$401F: CPU test/debug
//!   $4020-$5FFF: Expansion ROM
//!   $6000-$7FFF: SRAM / Battery-backed
//!   $8000-$FFFF: PRG-ROM (via Mapper)
//!
//! PPU 地址空间:
//!   $0000-$1FFF: Pattern table (CHR)
//!   $2000-$2FFF: Nametable
//!   $3000-$3EFF: Nametable mirrors
//!   $3F00-$3FFF: Palette RAM

use crate::apu_compat::ApuCompat;
use crate::controller::NesControllerPort;
use crate::mapper::Mapper;
use crate::ppu_compat::PpuCompat;
use std::boxed::Box;

/// NesBus trait — 所有总线实现需实现此接口
pub trait NesBus {
    fn cpu_read(&mut self, addr: u16) -> u8;
    fn cpu_write(&mut self, addr: u16, value: u8);
    fn ppu_read(&mut self, addr: u16) -> u8;
    fn ppu_write(&mut self, addr: u16, value: u8);
    fn tick_cpu(&mut self, cycles: u32);
    fn tick(&mut self); // tick PPU 3 cycles per CPU cycle
}

/// 完整 NES 总线实现
#[repr(C)]
pub struct NesBusImpl {
    pub ram: [u8; 0x800],
    pub ppu: PpuCompat,
    pub apu: ApuCompat,
    pub mapper: Box<dyn Mapper>,
    pub controller: [NesControllerPort; 2],
    pub cycles: u64,
}

impl NesBusImpl {
    pub fn new(mapper: Box<dyn Mapper>) -> Self {
        NesBusImpl {
            ram: [0; 0x800],
            ppu: PpuCompat::new(mapper.mirroring()),
            apu: ApuCompat::new(),
            mapper,
            controller: [NesControllerPort::new(), NesControllerPort::new()],
            cycles: 0,
        }
    }

    /// Render the PPU frame via safe field-level borrow splitting.
    ///
    /// The compiler sees that `self.ppu` and `self.mapper` are disjoint
    /// fields of `NesBusImpl`, making this call sound.
    pub fn render_ppu_frame(&mut self) {
        self.ppu.render_frame(&mut *self.mapper);
    }
}

impl NesBus for NesBusImpl {
    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => {
                // Internal RAM + mirrors
                self.ram[(addr & 0x07FF) as usize]
            }
            0x2000..=0x3FFF => {
                // PPU registers (mirrored every 8 bytes)
                self.ppu.read_register(0x2000 + (addr & 0x0007))
            }
            0x4000..=0x4017 => {
                match addr {
                    0x4016 => self.controller[0].read(),
                    0x4017 => {
                        // bit 0 = controller port 2, bit 1-7 = open bus
                        (self.controller[1].read()) | 0x40
                    }
                    _ => {
                        // APU reads
                        self.apu.read_register(addr)
                    }
                }
            }
            0x4018..=0x401F => 0,
            0x4020..=0x5FFF => 0,
            0x6000..=0x7FFF => 0,
            0x8000..=0xFFFF => {
                // PRG-ROM via Mapper
                self.mapper.cpu_read(addr).unwrap_or(0)
            }
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF => {
                self.ram[(addr & 0x07FF) as usize] = value;
            }
            0x2000..=0x3FFF => {
                self.ppu.write_register(0x2000 + (addr & 0x0007), value);
            }
            0x4000..=0x4017 => {
                match addr {
                    0x4014 => {
                        // OAM DMA
                        let page = (value as u16) << 8;
                        let mut data = [0u8; 256];
                        for i in 0..256usize {
                            data[i] = self.cpu_read(page + i as u16);
                        }
                        self.ppu.oam_dma(&data);
                        // DMA takes 513-514 cycles
                        self.tick_cpu(513);
                    }
                    0x4016 => {
                        // Controller strobe
                        self.controller[0].write_strobe(value);
                        self.controller[1].write_strobe(value);
                    }
                    _ => {
                        self.apu.write_register(addr, value);
                    }
                }
            }
            0x4018..=0x401F => {}
            0x4020..=0x5FFF => {}
            0x6000..=0x7FFF => {}
            0x8000..=0xFFFF => {
                self.mapper.cpu_write(addr, value);
            }
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => {
                // Pattern table
                self.mapper.ppu_read(addr).unwrap_or(0)
            }
            0x2000..=0x3EFF => {
                // Nametable + mirrors
                self.ppu.read_nametable(addr, self.mapper.mirroring())
            }
            0x3F00..=0x3FFF => {
                // Palette
                self.ppu.read_palette(addr)
            }
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF => {
                if self.mapper.ppu_write(addr, value) {
                    // CHR-RAM write
                }
            }
            0x2000..=0x3EFF => {
                self.ppu
                    .write_nametable(addr, value, self.mapper.mirroring());
            }
            0x3F00..=0x3FFF => {
                self.ppu.write_palette(addr, value);
            }
            _ => {}
        }
    }

    fn tick_cpu(&mut self, cycles: u32) {
        self.cycles += cycles as u64;
        // PPU runs at 3x CPU clock — advance PPU cycles*3 times
        self.ppu.step(cycles * 3);
        // APU runs at same rate as CPU
        self.apu.step(cycles);
    }

    fn tick(&mut self) {
        // Single tick for bus-level operations
        self.ppu.step(3);
        self.apu.step(1);
    }
}
