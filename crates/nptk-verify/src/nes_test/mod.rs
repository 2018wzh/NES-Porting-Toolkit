//! NES 测试 ROM 运行器
//!
//! 提供运行权威 NES 测试 ROM（nestest、blargg 等）并验证结果的功能。
//!
//! # 测试 ROM 来源
//!
//! - **nestest.nes** (kevtris): 最权威的 6502 CPU 测试，运行约 9000 条指令后
//!   输出 CPU 状态日志，可与官方 `nestest.log` 逐行对比。
//! - **blargg's NES test ROMs**: 涵盖 CPU、PPU、APU 的全面测试套件。
//!
//! # 使用方法
//!
//! 测试 ROM 文件应放在 `tests/roms/` 目录下（已加入 .gitignore）。
//!
//! ```ignore
//! use nptk_verify::nes_test::nestest::NestestRunner;
//!
//! let mut runner = NestestRunner::new("tests/roms/nestest/nestest.nes")?;
//! runner.run_all();
//! let log = runner.log();
//! // 与 nestest.log 对比...
//! ```

pub mod blargg;
pub mod nestest;

use nptk_core::bus::NesBusImpl;
use nptk_core::mapper::Cartridge;
use nptk_core::rom::NesRom;

/// 从 ROM 数据创建 Cartridge（用于测试）
pub fn create_test_cartridge(rom: &NesRom) -> Result<Cartridge, Box<dyn std::error::Error>> {
    nptk_mapper::init();
    let mapper = nptk_core::mapper::create_mapper(rom.header.mapper_id, rom)
        .ok_or_else(|| format!("Mapper {} not supported", rom.header.mapper_id))?;
    Ok(nptk_core::mapper::Cartridge::new_simple(
        nptk_core::mapper::CartridgeMetadata {
            mapper_id: rom.header.mapper_id,
            submapper_id: rom.header.submapper_id,
            prg_rom_size: rom.header.prg_rom_size,
            chr_rom_size: rom.header.chr_rom_size,
            has_sram: rom.header.has_sram,
            has_trainer: rom.header.has_trainer,
            battery_backed: false,
        },
        rom.prg_rom.clone(),
        nptk_core::mapper::ChrStorage::Rom(rom.chr_rom.clone().unwrap_or_default()),
        mapper,
    ))
}

/// 从 ROM 文件路径创建 NesBusImpl（用于测试）
pub fn create_test_bus(rom_path: &str) -> Result<NesBusImpl, Box<dyn std::error::Error>> {
    let data = std::fs::read(rom_path)?;
    let parsed = nptk_core::rom::parse_rom(&data)?;
    let cartridge = create_test_cartridge(&parsed)?;
    Ok(NesBusImpl::new(cartridge))
}
