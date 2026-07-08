//! Integration tests for nptk-core — end-to-end CPU + PPU + Bus scenarios.

use nptk_core::bus::{NesBus, NesBusImpl};
use nptk_core::rom::parse_rom;
use nptk_core::system::NesSystem;

/// Create a minimal valid NROM ROM with given PRG data.
fn make_rom(prg: &[u8]) -> nptk_core::rom::NesRom {
    let mut data = vec![0u8; 16 + 16384 + 8192];
    data[0..4].copy_from_slice(b"NES\x1a");
    data[4] = 1; // 1 PRG bank (16KB)
    data[5] = 1; // 1 CHR bank (8KB)
    let prg_off = 0x10;
    let copy_len = prg.len().min(16384);
    data[prg_off..prg_off + copy_len].copy_from_slice(&prg[..copy_len]);
    // Set reset vector to $8000
    data[prg_off + 0x3FFC] = 0x00;
    data[prg_off + 0x3FFD] = 0x80;
    parse_rom(&data).unwrap()
}

fn make_system(prg: &[u8]) -> NesSystem {
    let rom = make_rom(prg);
    // 优先使用 linkme 注册的 mapper，回退到内置 NROM
    let mapper = nptk_core::mapper::create_mapper(0, &rom)
        .unwrap_or_else(|| nptk_core::mapper::registry::builtin_nrom(&rom));
    let cartridge = nptk_core::mapper::Cartridge::new_simple(
        nptk_core::mapper::CartridgeMetadata {
            mapper_id: 0,
            submapper_id: 0,
            prg_rom_size: 1,
            chr_rom_size: 1,
            has_sram: false,
            has_trainer: false,
            battery_backed: false,
        },
        rom.prg_rom.clone(),
        nptk_core::mapper::ChrStorage::Rom(rom.chr_rom.clone().unwrap_or_default()),
        mapper,
    );
    NesSystem::new(NesBusImpl::new(cartridge))
}

// ── CPU integration ──────────────────────────────────────────────────────

#[test]
fn test_cpu_reset_jumps_to_vector() {
    // Program at $8000: LDA #$42, STA $50, JMP $8000
    let prg = &[0xA9, 0x42, 0x85, 0x50, 0x4C, 0x00, 0x80];
    let sys = make_system(prg);
    // Reset should set PC to $8000
    assert_eq!(sys.cpu.pc, 0x8000);
}

#[test]
fn test_cpu_ram_read_write() {
    let prg = &[0xA9, 0x42, 0x85, 0x50, 0x4C, 0x00, 0x80];
    let mut sys = make_system(prg);
    // Execute first instruction: LDA #$42
    sys.step_cpu();
    assert_eq!(sys.cpu.a, 0x42);
    // Execute second: STA $50
    sys.step_cpu();
    assert_eq!(sys.ram()[0x0050], 0x42);
}

#[test]
fn test_cpu_flag_behavior() {
    // LDA #$00 → zero flag set, negative clear
    let prg = &[0xA9, 0x00, 0x4C, 0x00, 0x80];
    let mut sys = make_system(prg);
    sys.step_cpu();
    assert!(sys.cpu.status.zero);
    assert!(!sys.cpu.status.negative);
}

// ── PPU integration ──────────────────────────────────────────────────────

#[test]
fn test_ppu_frame_produces_framebuffer() {
    let prg = &[0x4C, 0x00, 0x80]; // JMP $8000 (infinite loop)
    let mut sys = make_system(prg);
    let fb = sys.run_frame();
    assert_eq!(fb.len(), 256 * 240);
}

#[test]
fn test_ppu_vblank_sets_nmi() {
    let prg = &[0x4C, 0x00, 0x80]; // JMP $8000
    let mut sys = make_system(prg);
    // Enable NMI in PPU control register ($2000)
    sys.bus.cpu_write(0x2000, 0x80);
    sys.run_frame();
    assert!(sys.cpu.nmi_pending, "NMI should be pending after a frame");
}

#[test]
fn test_ppu_palette_all_indices_valid() {
    let prg = &[0x4C, 0x00, 0x80];
    let mut sys = make_system(prg);
    let fb = sys.run_frame();
    for &pixel in fb.iter() {
        assert!(pixel < 64, "NES palette index {} should be 0-63", pixel);
    }
}

// ── APU integration ──────────────────────────────────────────────────────

#[test]
fn test_apu_registers_accessible() {
    let prg = &[0x4C, 0x00, 0x80];
    let mut sys = make_system(prg);
    // Write to APU registers through the bus
    sys.bus.cpu_write(0x4000, 0x9F); // Pulse 1 duty/volume
    sys.bus.cpu_write(0x4001, 0x00); // Pulse 1 sweep
    sys.bus.cpu_write(0x4002, 0x42); // Pulse 1 timer low
    sys.bus.cpu_write(0x4003, 0x00); // Pulse 1 length/timer high
    // Read back through bus
    assert!(sys.bus.cpu_read(0x4000) != 0xFF, "APU should respond");
}

// ── Controller integration ───────────────────────────────────────────────

#[test]
fn test_controller_strobe_and_read() {
    let prg = &[0x4C, 0x00, 0x80];
    let mut sys = make_system(prg);

    // Set button state: A pressed
    let mut state = nptk_core::controller::NesControllerState::default();
    state.a = true;
    sys.bus.controller[0].set_current(state);

    // Strobe: latch the state
    sys.bus.controller[0].write_strobe(1);
    sys.bus.controller[0].write_strobe(0);

    // First read should return A (bit 0)
    let first = sys.bus.controller[0].read();
    assert_eq!(first & 1, 1, "First read should return A (pressed)");
}

// ── System integration ───────────────────────────────────────────────────

#[test]
fn test_multiple_frames_deterministic() {
    // Simple program that writes incrementing values to RAM
    let prg = &[
        0xA9, 0x01, // LDA #$01
        0x85, 0x50, // STA $50
        0x4C, 0x00, 0x80, // JMP $8000
    ];

    let run = || -> Vec<u64> {
        let mut sys = make_system(prg);
        let mut hashes = Vec::new();
        for _ in 0..3 {
            let fb = sys.run_frame();
            let hash: u64 = fb
                .iter()
                .enumerate()
                .map(|(i, &b)| (b as u64).wrapping_mul(i as u64 + 1))
                .fold(0, |a, b| a ^ b);
            hashes.push(hash);
        }
        hashes
    };

    let h1 = run();
    let h2 = run();
    assert_eq!(h1, h2, "Frame hashes should be deterministic across runs");
}

#[test]
fn test_system_ram_persists_across_frames() {
    // LDA #$42, STA $50 → $50 should stay $42 across frames
    let prg = &[0xA9, 0x42, 0x85, 0x50, 0x4C, 0x00, 0x80];
    let mut sys = make_system(prg);
    // Execute the program to set RAM
    sys.step_cpu(); // LDA #$42
    sys.step_cpu(); // STA $50
    assert_eq!(sys.ram()[0x0050], 0x42);
    // Run a frame — RAM should persist
    sys.run_frame();
    assert_eq!(sys.ram()[0x0050], 0x42, "RAM should persist across frames");
}
