//! Battle City 原生运行时
//! 组合 nptk-core 与各个原生子系统

use crate::game_state::BattleCityStateView;
use nptk::mapper::{Cartridge, CartridgeMetadata, ChrStorage};
use nptk::rom::NesRom;

/// Battle City 原生移植运行时
pub struct BattleCityRuntime {
    pub bus: nptk::bus::NesBusImpl,
    _rom: NesRom,
}

impl BattleCityRuntime {
    pub fn new(rom: NesRom) -> Result<Self, Box<dyn std::error::Error>> {
        let mapper = nptk::mapper::create_mapper(rom.header.mapper_id, &rom)
            .unwrap_or_else(|| nptk::mapper::registry::builtin_nrom(&rom));
        let cartridge = Cartridge::new_simple(
            CartridgeMetadata {
                mapper_id: rom.header.mapper_id,
                submapper_id: rom.header.submapper_id,
                prg_rom_size: rom.header.prg_rom_size,
                chr_rom_size: rom.header.chr_rom_size,
                has_sram: rom.header.has_sram,
                has_trainer: rom.header.has_trainer,
                battery_backed: false,
            },
            rom.prg_rom.clone(),
            ChrStorage::Rom(rom.chr_rom.clone().unwrap_or_default()),
            mapper,
        );

        Ok(Self {
            bus: nptk::bus::NesBusImpl::new(cartridge),
            _rom: rom,
        })
    }

    /// 获取游戏状态视图 (按需生成)
    pub fn state_view(&self) -> BattleCityStateView<'_> {
        BattleCityStateView::new(&self.bus.ram)
    }

    /// 读取游戏 RAM
    pub fn ram(&self) -> &[u8; 0x800] {
        &self.bus.ram
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use nptk::bus::NesBus;
    fn make_test_rom() -> NesRom {
        let mut data = vec![0u8; 16 + 16384 + 8192];
        data[0..4].copy_from_slice(b"NES\x1a");
        data[4] = 1;
        data[5] = 1;
        nptk::rom::parse_rom(&data).unwrap()
    }

    fn make_cartridge(rom: &NesRom) -> Cartridge {
        let mapper = nptk::mapper::create_mapper(rom.header.mapper_id, rom)
            .unwrap_or_else(|| nptk::mapper::registry::builtin_nrom(rom));
        Cartridge::new_simple(
            CartridgeMetadata {
                mapper_id: rom.header.mapper_id,
                submapper_id: rom.header.submapper_id,
                prg_rom_size: rom.header.prg_rom_size,
                chr_rom_size: rom.header.chr_rom_size,
                has_sram: rom.header.has_sram,
                has_trainer: rom.header.has_trainer,
                battery_backed: false,
            },
            rom.prg_rom.clone(),
            ChrStorage::Rom(rom.chr_rom.clone().unwrap_or_default()),
            mapper,
        )
    }

    #[test]
    fn test_runtime_creation() {
        let rom = make_test_rom();
        let rt = BattleCityRuntime::new(rom);
        assert!(rt.is_ok());
    }

    #[test]
    fn test_ram_accessible() {
        let rom = make_test_rom();
        let mut rt = BattleCityRuntime::new(rom).unwrap();
        rt.bus.cpu_write(0x0051, 3);
        assert_eq!(rt.ram()[0x0051], 3);
        assert_eq!(rt.state_view().lives(), 3);
    }

    // ── Golden frame tests ──────────────────────────────────────────

    /// Compute a simple hash of the framebuffer for golden comparison.
    fn frame_hash(fb: &[u8; 256 * 240]) -> u64 {
        let mut h: u64 = 0xDEADBEEF;
        for (i, &b) in fb.iter().enumerate() {
            h = h.wrapping_mul(31).wrapping_add(b as u64);
            if i % 101 == 0 {
                h = h.rotate_left(7);
            }
        }
        h
    }

    /// Load the Battle City ROM. Returns None if the ROM file is not found
    /// (e.g. in CI without the ROM).
    fn load_battle_city_rom() -> Option<NesRom> {
        let path = "roms/BattleCity (Japan).nes";
        let data = std::fs::read(path).ok()?;
        nptk::rom::parse_rom(&data).ok()
    }

    #[test]
    fn golden_rom_loads() {
        let rom = match load_battle_city_rom() {
            Some(r) => r,
            None => {
                eprintln!("Skipping: ROM not found");
                return;
            }
        };
        assert_eq!(rom.header.mapper_id, 0);
        assert_eq!(rom.header.prg_rom_size, 16384);
        assert_eq!(rom.header.chr_rom_size, 8192);
        assert_eq!(rom.header.mirroring, nptk::rom::Mirroring::Horizontal);
    }

    #[test]
    fn golden_title_screen_frame() {
        let rom = match load_battle_city_rom() {
            Some(r) => r,
            None => return,
        };

        let cart = make_cartridge(&rom);
        let bus = nptk::bus::NesBusImpl::new(cart);
        let mut system = nptk::system::NesSystem::new(bus);

        // Run enough frames to reach title screen (typically 120+ frames)
        let mut hashes: Vec<u64> = Vec::new();
        for _frame in 0..180 {
            let fb = system.run_frame();
            hashes.push(frame_hash(fb));
        }

        // The title screen should have rendered something non-black
        let fb = system.run_frame();
        let hash = frame_hash(fb);
        assert!(hash != 0, "Title screen should not be all black");

        // Check that rendering produced non-zero pixels
        let ppu = &system.bus.ppu;
        let raw_fb = ppu.frame();
        let non_zero = raw_fb.iter().filter(|&&b| b != 0).count();
        assert!(
            non_zero > 100,
            "Title screen should have visible pixels (got {})",
            non_zero
        );
    }

    #[test]
    fn golden_deterministic_frames() {
        let rom = match load_battle_city_rom() {
            Some(r) => r,
            None => return,
        };

        // Run two independent NES instances and compare frame hashes
        let run_frames = |count: u32| -> Vec<u64> {
            let cart = make_cartridge(&rom);
            let bus = nptk::bus::NesBusImpl::new(cart);
            let mut system = nptk::system::NesSystem::new(bus);
            let mut hashes = Vec::new();
            for _ in 0..count {
                let fb = system.run_frame();
                hashes.push(frame_hash(fb));
            }
            hashes
        };

        let h1 = run_frames(60);
        let h2 = run_frames(60);

        // Frame hashes should be identical across independent runs
        assert_eq!(h1.len(), h2.len());
        for (i, (a, b)) in h1.iter().zip(h2.iter()).enumerate() {
            assert_eq!(a, b, "Frame {} hash differs: {:016X} vs {:016X}", i, a, b);
        }
    }

    #[test]
    fn golden_game_state_after_frames() {
        let rom = match load_battle_city_rom() {
            Some(r) => r,
            None => return,
        };

        let cart = make_cartridge(&rom);
        let bus = nptk::bus::NesBusImpl::new(cart);
        let mut system = nptk::system::NesSystem::new(bus);

        // Run 180 frames (~3 seconds) to reach title screen
        for _ in 0..180 {
            system.run_frame();
        }

        let state = BattleCityStateView::new(system.ram());
        // At title screen: game_mode should be 0 (title), lives should be 0
        assert_eq!(state.game_mode(), 0, "Should be at title screen");
        let fb = system.bus.ppu.frame();
        let non_zero = fb.iter().filter(|&&b| b != 0).count();
        assert!(non_zero > 100, "Title screen should render visible content");
    }
}
