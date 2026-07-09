//! 三向帧缓冲比较工具
//!
//! 使用 tetanes-core 作为参考实现，比较：
//! 1. NPTK 解释器 (NesSystem)
//! 2. NPTK 重编译 (RecompiledRuntime)
//! 3. tetanes-core (ControlDeck)
//!
//! 使用方法:
//! ```
//! cargo run --release --bin tri-compare -- --rom "roms/BattleCity (Japan).nes" --frames 60 --output verify_output
//! ```

use std::path::Path;

use clap::Parser;
use image::RgbImage;
use nptk_core::bus::NesBusImpl;
use nptk_core::controller::NesControllerState;
use nptk_core::mapper::Cartridge;
use nptk_core::rom::NesRom;
use nptk_core::system::NesSystem;
use nptk_native_runtime::runtime::RecompiledRuntime;
use nptk_verify::tetanes_runner::TetanesRunner;
use nptk_verify::tri_compare::{
    compare_three_framebuffers, create_tri_comparison_image, save_rgb_png,
};

// NES 标准调色板（64 色 RGB）
const NES_PALETTE: [(u8, u8, u8); 64] = nptk_wgpu::palette::NES_PALETTE;
const FB_WIDTH: u32 = 256;
const FB_HEIGHT: u32 = 240;
const FB_PIXELS: usize = (FB_WIDTH * FB_HEIGHT) as usize;

#[derive(Parser, Debug)]
#[command(name = "tri-compare")]
#[command(about = "三向帧缓冲比较：NPTK 解释器 vs 重编译 vs tetanes-core")]
struct Args {
    /// ROM 文件路径
    #[arg(long)]
    rom: String,

    /// 运行帧数
    #[arg(long, default_value = "60")]
    frames: u32,

    /// 输出目录
    #[arg(long, default_value = "verify_output")]
    output: String,
}

/// 从 ROM 数据创建 Cartridge
fn create_cartridge(rom: &NesRom) -> Result<Cartridge, Box<dyn std::error::Error>> {
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

/// 空音频接收器
struct NullAudioSink;
impl nptk_native_runtime::runtime::AudioEventSink for NullAudioSink {
    fn push_sample(&mut self, _sample: f32) {}
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    println!("=== 三向帧缓冲比较 ===");
    println!("ROM: {}", args.rom);
    println!("帧数: {}", args.frames);
    println!("输出: {}", args.output);
    println!();

    // 加载 ROM
    let rom_data = std::fs::read(&args.rom)?;
    let parsed_rom = nptk_core::rom::parse_rom(&rom_data)?;
    println!("ROM 信息:");
    println!("  Mapper: {}", parsed_rom.header.mapper_id);
    println!("  PRG ROM: {} bytes", parsed_rom.header.prg_rom_size);
    println!("  CHR ROM: {} bytes", parsed_rom.header.chr_rom_size);
    println!();

    // 创建输出目录
    let output_dir = Path::new(&args.output);
    std::fs::create_dir_all(output_dir)?;

    // ── 1. NPTK 解释器（参考） ──
    println!("1/3 运行 NPTK 解释器 (NesSystem)...");
    let cartridge_ref = create_cartridge(&parsed_rom)?;
    let mut interpreter = NesSystem::from_cartridge(cartridge_ref);

    let mut interp_framebuffer: [u8; FB_PIXELS] = [0u8; FB_PIXELS];
    for frame in 0..args.frames {
        // 在第 60 帧按 Start 键
        if frame == 60 {
            interpreter.cpu.memory.controller[0].set_current(NesControllerState {
                start: true,
                ..Default::default()
            });
        } else if frame == 65 {
            interpreter.cpu.memory.controller[0].set_current(NesControllerState::default());
        }
        let fb = interpreter.run_frame();
        if frame == args.frames - 1 {
            interp_framebuffer = *fb;
        }
        if frame % 100 == 0 {
            println!("  解释器: 运行 {} 帧...", frame);
        }
    }
    println!("  解释器: 完成 {} 帧", args.frames);
    println!();

    // ── 2. NPTK 重编译（待验证） ──
    println!("2/3 运行 NPTK 重编译 (RecompiledRuntime)...");
    let cartridge_actual = create_cartridge(&parsed_rom)?;
    let bus_actual = NesBusImpl::new(cartridge_actual);
    let ppu_sink: Box<dyn nptk_native_runtime::runtime::PpuEventSink> =
        Box::new(nptk_native_runtime::ppu_bridge::PpuBridge::new());
    let audio_sink: Box<dyn nptk_native_runtime::runtime::AudioEventSink> = Box::new(NullAudioSink);
    let mut recompiled = RecompiledRuntime::new(bus_actual, ppu_sink, audio_sink);

    let mut recomp_framebuffer: [u8; FB_PIXELS] = [0u8; FB_PIXELS];
    for frame in 0..args.frames {
        // 在第 60 帧按 Start 键
        if frame == 60 {
            recompiled.cpu.memory.controller[0].set_current(NesControllerState {
                start: true,
                ..Default::default()
            });
        } else if frame == 65 {
            recompiled.cpu.memory.controller[0].set_current(NesControllerState::default());
        }
        recompiled.run_frame();
        if frame == args.frames - 1 {
            recomp_framebuffer = *recompiled.framebuffer();
        }
        if frame % 100 == 0 {
            println!("  重编译: 运行 {} 帧...", frame);
        }
    }
    println!("  重编译: 完成 {} 帧", args.frames);
    println!();

    // ── 3. tetanes-core（权威参考） ──
    println!("3/3 运行 tetanes-core (ControlDeck)...");
    let mut tetanes = TetanesRunner::from_rom_data(&rom_data)?;

    let mut tetanes_framebuffer: [u8; FB_PIXELS] = [0u8; FB_PIXELS];
    for frame in 0..args.frames {
        tetanes.run_frame()?;
        if frame == args.frames - 1 {
            tetanes_framebuffer = *tetanes.framebuffer();
        }
        if frame % 100 == 0 {
            println!("  tetanes: 运行 {} 帧...", frame);
        }
    }
    println!("  tetanes: 完成 {} 帧", args.frames);
    println!();

    // ── 三向比较 ──
    println!("进行三向比较...");
    let result = compare_three_framebuffers(
        &interp_framebuffer,
        &recomp_framebuffer,
        &tetanes_framebuffer,
    );
    println!("{}", result.summary());
    println!();

    // ── 生成截图 ──
    println!("生成截图...");

    // 三向比较图
    let comparison_img = create_tri_comparison_image(
        &interp_framebuffer,
        &recomp_framebuffer,
        &tetanes_framebuffer,
    );
    let comparison_path = output_dir.join("tri_comparison.png");
    save_rgb_png(&comparison_img, &comparison_path)?;
    println!("  三向比较图: {}", comparison_path.display());

    // 单独保存每个实现的截图
    let interp_img = index_to_rgb_image(&interp_framebuffer);
    let interp_path = output_dir.join("nptk_interpreter.png");
    save_rgb_png(&interp_img, &interp_path)?;
    println!("  NPTK 解释器: {}", interp_path.display());

    let recomp_img = index_to_rgb_image(&recomp_framebuffer);
    let recomp_path = output_dir.join("nptk_recompiled.png");
    save_rgb_png(&recomp_img, &recomp_path)?;
    println!("  NPTK 重编译: {}", recomp_path.display());

    let tetanes_img = index_to_rgb_image(&tetanes_framebuffer);
    let tetanes_path = output_dir.join("tetanes.png");
    save_rgb_png(&tetanes_img, &tetanes_path)?;
    println!("  tetanes-core: {}", tetanes_path.display());

    // ── 生成报告 ──
    let report = format!(
        "=== 三向帧缓冲比较报告 ===\n\n\
        ROM: {}\n\
        帧数: {}\n\n\
        {}\n\
        生成文件:\n\
        - {}\n\
        - {}\n\
        - {}\n\
        - {}\n",
        args.rom,
        args.frames,
        result.summary(),
        comparison_path.display(),
        interp_path.display(),
        recomp_path.display(),
        tetanes_path.display(),
    );

    let report_path = output_dir.join("tri_compare_report.txt");
    std::fs::write(&report_path, &report)?;
    println!("  报告: {}", report_path.display());
    println!();
    println!("{}", report);

    Ok(())
}

/// 将 NES 索引色帧缓冲转换为 RGB 图像
fn index_to_rgb_image(fb: &[u8; FB_PIXELS]) -> RgbImage {
    let mut img = RgbImage::new(FB_WIDTH, FB_HEIGHT);
    for y in 0..FB_HEIGHT {
        for x in 0..FB_WIDTH {
            let idx = (y as usize) * FB_WIDTH as usize + (x as usize);
            let palette_idx = fb[idx] as usize % 64;
            let (r, g, b) = NES_PALETTE[palette_idx];
            img.put_pixel(x, y, image::Rgb([r, g, b]));
        }
    }
    img
}
