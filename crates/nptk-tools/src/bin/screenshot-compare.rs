//! 启动截图比较工具
//!
//! 运行解释器和重编译版本各 60 帧，生成启动截图并比较。
//!
//! 使用方法:
//! ```
//! cargo run --release --bin screenshot-compare -- --rom "roms/BattleCity (Japan).nes" --frames 60 --output verify_output
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

// NES 标准调色板（64 色 RGB）
const NES_PALETTE: [(u8, u8, u8); 64] = nptk_wgpu::palette::NES_PALETTE;

// 帧缓冲大小
const FB_SIZE: usize = 256 * 240;
const FB_WIDTH: u32 = 256;
const FB_HEIGHT: u32 = 240;
const FB_PIXELS: usize = (FB_WIDTH * FB_HEIGHT) as usize;

#[derive(Parser, Debug)]
#[command(name = "screenshot-compare")]
#[command(about = "比较解释器和重编译版本的启动截图")]
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

/// 将 RGB 图像保存为 PNG
fn save_png(img: &RgbImage, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    )?;
    std::fs::write(path, bytes)?;
    Ok(())
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

/// 生成并排比较图像（解释器 | 重编译 | 差异高亮）
fn create_comparison_image(ref_fb: &[u8; FB_PIXELS], actual_fb: &[u8; FB_PIXELS]) -> RgbImage {
    let img_width = FB_WIDTH * 3;
    let img_height = FB_HEIGHT;
    let mut img = RgbImage::new(img_width, img_height);

    for y in 0..FB_HEIGHT {
        for x in 0..FB_WIDTH {
            let idx = (y as usize) * FB_WIDTH as usize + (x as usize);
            let ref_idx = ref_fb[idx] as usize % 64;
            let actual_idx = actual_fb[idx] as usize % 64;
            let (ref_r, ref_g, ref_b) = NES_PALETTE[ref_idx];
            let (actual_r, actual_g, actual_b) = NES_PALETTE[actual_idx];

            // 左栏：解释器（参考）
            img.put_pixel(x, y, image::Rgb([ref_r, ref_g, ref_b]));

            // 中栏：重编译
            img.put_pixel(FB_WIDTH + x, y, image::Rgb([actual_r, actual_g, actual_b]));

            // 右栏：差异高亮
            if ref_fb[idx] != actual_fb[idx] {
                // 差异像素：红色高亮
                img.put_pixel(FB_WIDTH * 2 + x, y, image::Rgb([255, 0, 0]));
            } else {
                // 匹配像素：原色显示
                img.put_pixel(FB_WIDTH * 2 + x, y, image::Rgb([ref_r, ref_g, ref_b]));
            }
        }
    }

    img
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    println!("=== NES 启动截图比较 ===");
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

    // ── 解释器（参考） ──
    println!("运行解释器 (NesSystem)...");
    let cartridge_ref = create_cartridge(&parsed_rom)?;
    let mut interpreter = NesSystem::from_cartridge(cartridge_ref);

    let mut ref_framebuffer: [u8; FB_SIZE] = [0u8; FB_SIZE];
    for frame in 0..args.frames {
        // Battle City 输入序列：
        // 帧 1-59: 无输入（等待初始化）
        // 帧 60-64: 按 Start（进入标题画面）
        // 帧 65+: 释放 Start（等待标题画面出现）
        if frame == 60 {
            interpreter.cpu.memory.controller[0].set_current(NesControllerState {
                start: true,
                ..Default::default()
            });
        } else if frame == 65 {
            interpreter.cpu.memory.controller[0].set_current(NesControllerState::default());
        }
        let fb = interpreter.run_frame();
        // 保存最后一帧
        if frame == args.frames - 1 {
            ref_framebuffer = *fb;
        }
        if frame % 100 == 0 {
            println!("  解释器: 运行 {} 帧...", frame);
        }
    }
    println!("  解释器: 完成 {} 帧", args.frames);
    println!();

    // ── 重编译（待验证） ──
    println!("运行重编译 (RecompiledRuntime)...");
    let cartridge_actual = create_cartridge(&parsed_rom)?;
    let bus_actual = NesBusImpl::new(cartridge_actual);
    let ppu_sink: Box<dyn nptk_native_runtime::runtime::PpuEventSink> =
        Box::new(nptk_native_runtime::ppu_bridge::PpuBridge::new());
    let audio_sink: Box<dyn nptk_native_runtime::runtime::AudioEventSink> = Box::new(NullAudioSink);
    let mut recompiled = RecompiledRuntime::new(bus_actual, ppu_sink, audio_sink);

    let mut actual_framebuffer: [u8; FB_SIZE] = [0u8; FB_SIZE];
    for frame in 0..args.frames {
        // 在第 60 帧按 Start 键（进入标题画面）
        if frame == 60 {
            recompiled.cpu.memory.controller[0].set_current(NesControllerState {
                start: true,
                ..Default::default()
            });
        } else if frame == 65 {
            // 释放 Start 键

            recompiled.cpu.memory.controller[0].set_current(NesControllerState::default());
        }
        recompiled.run_frame();
        if frame == args.frames - 1 {
            actual_framebuffer = *recompiled.framebuffer();
        }
        if frame % 100 == 0 {
            println!("  重编译: 运行 {} 帧...", frame);
        }
    }
    println!("  重编译: 完成 {} 帧", args.frames);
    println!();

    // ── 比较帧缓冲 ──
    println!("比较帧缓冲...");
    let mut mismatched = 0usize;
    let total_pixels = FB_PIXELS;
    for i in 0..total_pixels {
        if ref_framebuffer[i] != actual_framebuffer[i] {
            mismatched += 1;
        }
    }
    let ratio = mismatched as f64 / total_pixels as f64 * 100.0;
    println!("  总像素: {}", total_pixels);
    println!("  差异像素: {}", mismatched);
    println!("  差异率: {:.4}%", ratio);
    println!();

    if mismatched == 0 {
        println!("✓ 帧缓冲完全一致！");
    } else {
        println!("✗ 帧缓冲存在差异！");
    }
    println!();

    // ── 生成截图 ──
    println!("生成截图...");

    // 解释器截图
    let ref_img = index_to_rgb_image(&ref_framebuffer);
    let ref_path = output_dir.join("interpreter_frame.png");
    save_png(&ref_img, &ref_path)?;
    println!("  解释器截图: {}", ref_path.display());

    // 重编译截图
    let actual_img = index_to_rgb_image(&actual_framebuffer);
    let actual_path = output_dir.join("recompiled_frame.png");
    save_png(&actual_img, &actual_path)?;
    println!("  重编译截图: {}", actual_path.display());

    // 并排比较图
    let comparison_img = create_comparison_image(&ref_framebuffer, &actual_framebuffer);
    let comparison_path = output_dir.join("comparison_frame.png");
    save_png(&comparison_img, &comparison_path)?;
    println!("  比较截图: {}", comparison_path.display());

    // ── 生成报告 ──
    let report = format!(
        "=== NES 启动截图比较报告 ===\n\
        \n\
        ROM: {}\n\
        帧数: {}\n\
        \n\
        帧缓冲比较:\n\
        总像素: {}\n\
        差异像素: {}\n\
        差异率: {:.4}%\n\
        \n\
        结果: {}\n\
        \n\
        生成文件:\n\
        - {}\n\
        - {}\n\
        - {}\n",
        args.rom,
        args.frames,
        total_pixels,
        mismatched,
        ratio,
        if mismatched == 0 {
            "完全一致 ✓"
        } else {
            "存在差异 ✗"
        },
        ref_path.display(),
        actual_path.display(),
        comparison_path.display(),
    );

    let report_path = output_dir.join("screenshot_report.txt");
    std::fs::write(&report_path, &report)?;
    println!("  报告: {}", report_path.display());
    println!();
    println!("{}", report);

    Ok(())
}

/// 空音频接收器
struct NullAudioSink;
impl nptk_native_runtime::runtime::AudioEventSink for NullAudioSink {
    fn push_sample(&mut self, _sample: f32) {}
}
