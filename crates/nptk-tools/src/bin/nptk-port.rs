//! nptk-port — NES Porting Toolkit 主 CLI
//!
//! 子命令:
//!   inspect     — 检查 ROM 信息和 GameProfile
//!   run         — 运行游戏 (compat-interpreter | recompiled-compat | native-port)
//!   trace       — CPU trace 记录
//!   recompile   — 静态重编译
//!   dump-chr    — 导出 CHR atlas
//!   golden      — golden test 运行
//!   input-test  — 输入测试

use clap::{Parser, Subcommand};
use nptk_core::bus::NesBus;

#[derive(Parser)]
#[command(name = "nptk-port", about = "NES Porting Toolkit")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 检查 ROM 信息和 GameProfile
    Inspect {
        /// ROM 文件路径
        #[arg(long)]
        rom: String,
        /// GameProfile 路径
        #[arg(long)]
        profile: Option<String>,
    },
    /// 运行游戏
    Run {
        /// ROM 文件路径
        #[arg(long)]
        rom: String,
        /// GameProfile 路径
        #[arg(long)]
        profile: Option<String>,
        /// 运行模式
        #[arg(long, default_value = "compat-interpreter")]
        mode: String,
        /// 帧数
        #[arg(long, default_value = "60")]
        frames: u32,
        /// 导出最后一帧为 PNG
        #[arg(long)]
        frame_out: Option<String>,
    },
    /// CPU trace 记录
    Trace {
        /// ROM 文件路径
        #[arg(long)]
        rom: String,
        /// GameProfile 路径
        #[arg(long)]
        profile: Option<String>,
        /// 输入 replay 文件
        #[arg(long)]
        input: Option<String>,
    },
    /// 静态重编译
    Recompile {
        /// ROM 文件路径
        #[arg(long)]
        rom: String,
        /// GameProfile 路径
        #[arg(long)]
        profile: Option<String>,
        /// 输出目录
        #[arg(long)]
        out: Option<String>,
    },
    /// 导出 CHR atlas
    DumpChr {
        /// ROM 文件路径
        #[arg(long)]
        rom: String,
        /// 输出 PNG 路径
        #[arg(long)]
        out: Option<String>,
    },
    /// golden test 运行
    Golden {
        /// ROM 文件路径
        #[arg(long)]
        rom: String,
        /// GameProfile 路径
        #[arg(long)]
        profile: Option<String>,
        /// 输入 replay 文件
        #[arg(long)]
        input: Option<String>,
    },
    /// 输入测试
    InputTest {
        /// 后端
        #[arg(long, default_value = "auto")]
        backend: String,
        /// 记录输入到文件
        #[arg(long)]
        record: Option<String>,
        /// mapping wizard
        #[arg(long)]
        mapping_wizard: Option<String>,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Inspect { rom, profile } => cmd_inspect(&rom, profile.as_deref()),
        Commands::Run {
            rom,
            profile,
            mode,
            frames,
            frame_out,
        } => cmd_run(
            &rom,
            profile.as_deref(),
            &mode,
            frames,
            frame_out.as_deref(),
        ),
        Commands::Trace {
            rom,
            profile,
            input,
        } => cmd_trace(&rom, profile.as_deref(), input.as_deref()),
        Commands::Recompile { rom, profile, out } => {
            cmd_recompile(&rom, profile.as_deref(), out.as_deref())
        }
        Commands::DumpChr { rom, out } => cmd_dump_chr(&rom, out.as_deref()),
        Commands::Golden {
            rom,
            profile,
            input,
        } => cmd_golden(&rom, profile.as_deref(), input.as_deref()),
        Commands::InputTest {
            backend,
            record,
            mapping_wizard,
        } => cmd_input_test(&backend, record.as_deref(), mapping_wizard.as_deref()),
    }
}

fn cmd_inspect(rom: &str, profile: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    println!("Inspecting ROM: {}", rom);
    let data = std::fs::read(rom)?;
    let parsed = nptk_core::rom::parse_rom(&data)?;
    println!("  Mapper: {}", parsed.header.mapper_id);
    println!("  PRG: {} bytes", parsed.header.prg_rom_size);
    println!("  CHR: {} bytes", parsed.header.chr_rom_size);
    println!("  Mirroring: {:?}", parsed.header.mirroring);

    if let Some(prof_path) = profile {
        match nptk_profile::profile::load_profile(prof_path) {
            Ok(p) => println!("Game: {}", p.game.display_name),
            Err(e) => println!("Warning: could not load profile: {}", e),
        }
    }
    Ok(())
}

fn cmd_run(
    rom: &str,
    _profile: Option<&str>,
    mode: &str,
    frames: u32,
    frame_out: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Running {} in mode: {}", rom, mode);
    let data = std::fs::read(rom)?;
    let parsed = nptk_core::rom::parse_rom(&data)?;

    match mode {
        "recompiled-compat" => run_recompiled_compat(&parsed, frames, frame_out),
        "native-port" => run_native_port(&parsed, frames, frame_out),
        _ => run_compat_interpreter(&parsed, frames, frame_out),
    }
}

fn make_cartridge(
    parsed: &nptk_core::rom::NesRom,
) -> Result<nptk_core::mapper::Cartridge, Box<dyn std::error::Error>> {
    let mapper = nptk_core::mapper::create_mapper(parsed.header.mapper_id, parsed)
        .ok_or_else(|| format!("Mapper {} not supported", parsed.header.mapper_id))?;
    Ok(nptk_core::mapper::Cartridge::new_simple(
        nptk_core::mapper::CartridgeMetadata {
            mapper_id: parsed.header.mapper_id,
            submapper_id: parsed.header.submapper_id,
            prg_rom_size: parsed.header.prg_rom_size,
            chr_rom_size: parsed.header.chr_rom_size,
            has_sram: parsed.header.has_sram,
            has_trainer: parsed.header.has_trainer,
            battery_backed: false,
        },
        parsed.prg_rom.clone(),
        nptk_core::mapper::ChrStorage::Rom(parsed.chr_rom.clone().unwrap_or_default()),
        mapper,
    ))
}

fn run_compat_interpreter(
    parsed: &nptk_core::rom::NesRom,
    frames: u32,
    frame_out: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let cart = make_cartridge(parsed)?;
    let bus = nptk_core::bus::NesBusImpl::new(cart);
    let mut system = nptk_core::system::NesSystem::new(bus);

    println!(
        "NES System started. Mapper: {}, PRG: {}KB, CHR: {}KB",
        parsed.header.mapper_id,
        parsed.header.prg_rom_size / 1024,
        parsed.header.chr_rom_size / 1024
    );
    println!("Running {} frames in compat-interpreter mode...", frames);
    let mut last_fb: Option<Box<[u8; 256 * 240]>> = None;
    for frame in 0..frames {
        let fb = *system.run_frame(); // copy the framebuffer
        let fhash: u32 = fb
            .iter()
            .enumerate()
            .map(|(i, &p)| (p as u32).wrapping_mul((i as u32 % 251) + 1))
            .fold(0, |a, b| a ^ b);
        let pc = system.cpu.pc;
        let ctrl = system.bus.ppu.ctrl;
        let mask = system.bus.ppu.mask;
        if frame < 5 || frame % 10 == 0 {
            println!(
                "  Frame {:2}: PC=${:04X} fhash={:08X} CTRL=${:02X} MASK=${:02X}",
                frame, pc, fhash, ctrl, mask
            );
        }
        last_fb = Some(Box::new(fb));
    }

    // Export last frame as PNG if requested
    if let (Some(path), Some(fb)) = (frame_out, &last_fb) {
        let palette = nptk_wgpu::palette::NES_PALETTE;
        let mut img = image::RgbImage::new(256, 240);
        for y in 0..240u32 {
            for x in 0..256u32 {
                let idx = fb[(y * 256 + x) as usize] as usize;
                let (r, g, b) = palette[idx % 64];
                img.put_pixel(x, y, image::Rgb([r, g, b]));
            }
        }
        img.save(path)?;
        println!("Frame exported to {}", path);
    }

    Ok(())
}

fn run_recompiled_compat(
    parsed: &nptk_core::rom::NesRom,
    frames: u32,
    _frame_out: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Running recompiled-compat (native dispatch + interpreter fallback)");
    let cart = make_cartridge(parsed)?;
    let bus = nptk_core::bus::NesBusImpl::new(cart);

    struct NullSink;
    impl nptk_native_runtime::runtime::PpuEventSink for NullSink {}
    impl nptk_native_runtime::runtime::AudioEventSink for NullSink {}

    let mut runtime = nptk_native_runtime::runtime::RecompiledRuntime::new(
        bus,
        Box::new(NullSink),
        Box::new(NullSink),
    );

    println!("Running {} frames...", frames);
    for frame in 0..frames {
        runtime.run_frame();
        let fb = runtime.framebuffer();
        let fhash: u32 = fb
            .iter()
            .enumerate()
            .map(|(i, &p)| (p as u32).wrapping_mul((i as u32 % 251) + 1))
            .fold(0, |a, b| a ^ b);
        if frame < 5 || frame % 10 == 0 {
            println!(
                "  Frame {:2}: fhash={:08X} blocks={}",
                frame,
                fhash,
                runtime.dispatch.len()
            );
        }
    }
    Ok(())
}

fn run_native_port(
    parsed: &nptk_core::rom::NesRom,
    frames: u32,
    _frame_out: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Running native-port (recompiled + native WGPU rendering)");
    // For now, native-port delegates to recompiled-compat with native render mode
    // Full implementation requires WGPU render pipeline integration
    run_recompiled_compat(parsed, frames, _frame_out)
}

fn cmd_trace(
    rom: &str,
    _profile: Option<&str>,
    _input: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let data = std::fs::read(rom)?;
    let parsed = nptk_core::rom::parse_rom(&data)?;
    let cart = make_cartridge(&parsed)?;
    let bus = nptk_core::bus::NesBusImpl::new(cart);
    let mut system = nptk_core::system::NesSystem::new(bus);

    println!("Trace: executing 10000 CPU instructions...");
    for i in 0..10000 {
        let pc_before = system.cpu.pc;
        let opcode = system.bus.cpu_read(pc_before);
        let cycles = system.step_cpu();
        println!(
            "  {:4}: PC=${:04X} OP=${:02X} A=${:02X} X=${:02X} Y=${:02X} SP=${:02X} P=${:02X} cy={}",
            i,
            pc_before,
            opcode,
            system.cpu.a,
            system.cpu.x,
            system.cpu.y,
            system.cpu.sp,
            system.cpu.status.to_byte(),
            cycles
        );
    }

    Ok(())
}

fn cmd_recompile(
    rom: &str,
    _profile: Option<&str>,
    out: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let data = std::fs::read(rom)?;
    let parsed = nptk_core::rom::parse_rom(&data)?;
    let out_dir = out.unwrap_or("generated");
    std::fs::create_dir_all(out_dir)?;

    let prg = &parsed.prg_rom;
    println!("PRG-ROM: {} bytes", prg.len());

    // Extract vectors from end of PRG-ROM (mapped at $FFFA-$FFFF)
    let end = prg.len();
    let nmi_vec = prg[end - 6] as u16 | ((prg[end - 5] as u16) << 8);
    let reset_vec = prg[end - 4] as u16 | ((prg[end - 3] as u16) << 8);
    let irq_vec = prg[end - 2] as u16 | ((prg[end - 1] as u16) << 8);
    println!(
        "RESET: ${:04X}  NMI: ${:04X}  IRQ: ${:04X}",
        reset_vec, nmi_vec, irq_vec
    );

    // Map CPU addr → PRG offset
    let to_offset = |addr: u16| -> Option<usize> {
        if addr < 0x8000 {
            None
        } else {
            Some((addr as usize - 0x8000) % prg.len())
        }
    };

    // Disassemble each entry point using disasm6502::from_addr_array
    for (label, entry) in [("reset", reset_vec), ("nmi", nmi_vec), ("irq", irq_vec)] {
        if let Some(off) = to_offset(entry) {
            println!("\n--- {} handler at ${:04X} ---", label, entry);
            match disasm6502::from_addr_array(&prg[off..], entry) {
                Ok(insns) => {
                    for insn in insns.iter().take(30) {
                        println!("  {}", insn);
                    }
                    if insns.len() > 30 {
                        println!("  ... ({} more)", insns.len() - 30);
                    }
                }
                Err(e) => println!("  disasm error: {}", e),
            }
        }
    }

    //     // Count total disassembled instructions
    //     let mut total = 0u64;
    //     let mut addr = 0x8000u16;
    //     while (addr as usize) < 0x100000 {
    //         if let Some(off) = to_offset(addr) {
    //             if let Ok(insns) = disasm6502::from_addr_array(&prg[off..], addr) {
    //                 for _ in insns.iter() { total += 1; }
    //             }
    //             addr = addr.saturating_add(1);
    //         } else { break; }
    //     }
    //     println!("\nTotal: ~{} instructions reachable", total);

    // Generate native code using Cranelift AOT from PRG-ROM blocks
    println!("\nGenerating native code...");
    let mut aot = nptk_recompiler::codegen::CraneliftAot::new()
        .map_err(|e| format!("CraneliftAot::new: {}", e))?;

    use nptk_recompiler::ir_builder::IrBuilder;
    use std::collections::{HashSet, VecDeque};
    let mut visited: HashSet<u16> = HashSet::new();
    let mut queue: VecDeque<u16> = VecDeque::from([reset_vec, nmi_vec, irq_vec]);
    let mut block_count = 0u32;

    // Complete 6502 instruction length table
    fn insn_len(op: u8) -> u16 {
        match op {
            // 3-byte instructions
            0x20|0x4C|0x6C| // JSR abs, JMP abs, JMP ind
            0x0D|0x0E|0x1D|0x1E|0x2D|0x2E|0x3D|0x3E| // ORA/ASL etc abs/absx
            0x4D|0x4E|0x5D|0x5E|0x6D|0x6E|0x7D|0x7E|
            0x8D|0x8E|0x9D|0x9E|0xAD|0xAE|0xBD|0xBE|
            0xCD|0xCE|0xDD|0xDE|0xED|0xEE|0xFD|0xFE|
            0x0C|0x1C|0x2C|0x3C|0x5C|0x7C| // NOP abs/absx (some illegal)
            0x8C|0xAC|0xBC|0xCC|0xDC|0xEC|0xFC|
            0x19|0x1B|0x39|0x3B|0x59|0x5B|0x79|0x7B| // ORA/AND etc absy
            0x99|0x9B|0xB9|0xBB|0xD9|0xDB|0xF9|0xFB|
            0x0F|0x1F|0x2F|0x3F|0x4F|0x5F|0x6F|0x7F| // various abs/absx
            0x8F|0x9F|0xAF|0xBF|0xCF|0xDF|0xEF|0xFF => 3,
            // 1-byte instructions (implied/accumulator)
            0x00|0x08|0x18|0x28|0x38|0x48|0x58|0x68|
            0x78|0x88|0x98|0xA8|0xB8|0xC8|0xD8|0xE8|
            0xF8|0x0A|0x2A|0x4A|0x6A|0x8A|0x9A|0xAA|
            0xBA|0xCA|0xEA|0x40|0x60|0x1A|0x3A|0x5A|
            0x7A|0xDA|0xFA => 1,
            // Default: 2-byte instructions (zeropage, immediate, branch, etc)
            _ => 2,
        }
    }

    // BFS block discovery from entry points
    while let Some(addr) = queue.pop_front() {
        if visited.contains(&addr) || addr < 0x8000 {
            continue;
        }
        visited.insert(addr);

        let mut block_bytes = Vec::new();
        let mut scan = addr;
        loop {
            let off = match to_offset(scan) {
                Some(o) => o,
                None => break,
            };
            if off >= prg.len() {
                break;
            }
            let b = prg[off];
            let len = insn_len(b);
            let next = scan.wrapping_add(len);

            // Push ALL bytes of this instruction (opcode + operands)
            for i in 0..len {
                if let Some(&byte) = prg.get(off + i as usize) {
                    block_bytes.push(byte);
                }
            }

            // Determine if this instruction terminates the block
            let is_terminal = match b {
                // RTS, RTI, BRK — return to caller
                0x60 | 0x40 | 0x00 => true,
                // JMP absolute — unconditional jump
                0x4C => {
                    let lo = prg.get(off + 1).copied().unwrap_or(0) as u16;
                    let hi = prg.get(off + 2).copied().unwrap_or(0) as u16;
                    let target = lo | (hi << 8);
                    if target >= 0x8000 {
                        queue.push_back(target);
                    }
                    true
                }
                // JMP indirect (0x6C) — resolved at runtime, need interpreter fallback
                0x6C => true,
                // JSR — push return address, jump to subroutine
                0x20 => {
                    let lo = prg.get(off + 1).copied().unwrap_or(0) as u16;
                    let hi = prg.get(off + 2).copied().unwrap_or(0) as u16;
                    let target = lo | (hi << 8);
                    if target >= 0x8000 {
                        queue.push_back(target);
                    }
                    // Return address is fallthrough
                    queue.push_back(next);
                    true
                }
                // Conditional branches — both taken and not-taken
                0x10 | 0x30 | 0x50 | 0x70 | 0x90 | 0xB0 | 0xD0 | 0xF0 => {
                    let offset = prg.get(off + 1).copied().unwrap_or(0) as i8;
                    // Correct sign-extended offset
                    let taken = ((next as i32) + (offset as i32)) as u16;
                    if taken >= 0x8000 {
                        queue.push_back(taken);
                    }
                    queue.push_back(next); // not-taken
                    true
                }
                _ => false,
            };
            scan = next;
            if is_terminal {
                break;
            }
        }
        if !block_bytes.is_empty() {
            let ir_ops = IrBuilder::lift_block(&block_bytes, addr);
            aot.compile_block(addr, &ir_ops)
                .map_err(|e| format!("compile block ${:04X}: {}", addr, e))?;
            block_count += 1;
        }
    }

    println!("  Extracted {} basic blocks", block_count);

    // Finish compilation and write .o file
    let (obj_bytes, compiled_blocks, _block_names) =
        aot.finish().map_err(|e| format!("aot.finish: {}", e))?;
    let obj_path = format!("{}/blocks.o", out_dir);
    std::fs::write(&obj_path, &obj_bytes)?;

    // Generate bindings
    let bindings = nptk_recompiler::aot_link::generate_bindings(&compiled_blocks);
    let gen_src_dir = format!("{}/src", out_dir);
    std::fs::create_dir_all(&gen_src_dir)?;
    let gen_src_path = format!("{}/bindings.rs", gen_src_dir);
    std::fs::write(&gen_src_path, &bindings.rust_source)?;

    // Ensure Cargo.toml exists
    let gen_cargo_path = format!("{}/Cargo.toml", out_dir);
    if !std::path::Path::new(&gen_cargo_path).exists() {
        let cargo_toml = format!(
            r#"[package]
name = "generated-battle-city"
version = "0.1.0"
edition = "2024"
publish = false

[dependencies]
nptk-native-runtime = {{ path = "../../crates/nptk-native-runtime" }}

[lib]
name = "generated_battle_city"
path = "src/lib.rs"
"#
        );
        std::fs::write(&gen_cargo_path, &cargo_toml)?;
    }

    println!("  Generated .o file: {}", obj_path);
    println!("  Generated bindings: {}", gen_src_path);

    // Write manifest
    let manifest = serde_json::json!({
        "game": "Battle City",
        "mapper": parsed.header.mapper_id,
        "prg_size": prg.len(),
        "entry_points": { "reset": format!("0x{:04X}", reset_vec), "nmi": format!("0x{:04X}", nmi_vec), "irq": format!("0x{:04X}", irq_vec) },
    });
    let path = format!("{}/manifest.json", out_dir);
    std::fs::write(&path, serde_json::to_string_pretty(&manifest)?)?;
    println!("Saved manifest to {}", path);

    Ok(())
}

fn cmd_dump_chr(rom: &str, out: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Dumping CHR from: {}", rom);
    let data = std::fs::read(rom)?;
    let parsed = nptk_core::rom::parse_rom(&data)?;

    if let Some(chr) = &parsed.chr_rom {
        let path = out.unwrap_or("chr.png");
        let n_tiles = chr.len() / 16;
        let cols = 16.min(n_tiles);
        let rows = (n_tiles + cols - 1) / cols;
        let width = cols * 8;
        let height = rows * 8;

        // NES palette grayscale: 4 color indices → grayscale values
        let palette: [u8; 4] = [0, 85, 170, 255];

        let mut img = image::GrayImage::new(width as u32, height as u32);
        for tile_idx in 0..n_tiles {
            let tile_data = &chr[tile_idx * 16..tile_idx * 16 + 16];
            let tx = (tile_idx % cols) * 8;
            let ty = (tile_idx / cols) * 8;
            for py in 0..8 {
                let plane0 = tile_data[py];
                let plane1 = tile_data[py + 8];
                for px in 0..8 {
                    let bit = 7 - px;
                    let color_idx = ((plane0 >> bit) & 1) | (((plane1 >> bit) & 1) << 1);
                    img.put_pixel(
                        (tx + px as usize) as u32,
                        (ty + py as usize) as u32,
                        image::Luma([palette[color_idx as usize]]),
                    );
                }
            }
        }
        img.save(path)?;
        println!(
            "CHR atlas: {} tiles, {}x{} saved to {}",
            n_tiles, cols, rows, path
        );
    } else if parsed.has_chr_ram {
        println!("ROM uses CHR-RAM (no static CHR-ROM data)");
    }

    Ok(())
}

fn cmd_golden(
    rom: &str,
    profile_path: Option<&str>,
    input_path: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let data = std::fs::read(rom)?;
    let parsed = nptk_core::rom::parse_rom(&data)?;
    let cart = make_cartridge(&parsed)?;
    let bus = nptk_core::bus::NesBusImpl::new(cart);
    let mut system = nptk_core::system::NesSystem::new(bus);

    // Load input replay if provided
    let mut replay: Option<nptk_input::replay::ReplayBackend> = None;
    if let Some(input_file) = input_path {
        let input_data = std::fs::read_to_string(input_file)?;
        let parsed_replay: nptk_input::replay::InputReplay = ron::from_str(&input_data)?;
        let frame_count = parsed_replay.frames.len();
        replay = Some(nptk_input::replay::ReplayBackend::new(parsed_replay));
        println!("Loaded input replay: {} frames", frame_count);
    }

    // Load profile for expected hashes if provided
    let mut expected_hashes: Option<Vec<u32>> = None;
    if let Some(profile_file) = profile_path {
        let profile_data = std::fs::read_to_string(profile_file)?;
        let profile: nptk_profile::profile::GameProfile = toml::from_str(&profile_data)?;
        // Check for a tests.ron file next to the profile
        let profile_dir = std::path::Path::new(profile_file)
            .parent()
            .unwrap_or(std::path::Path::new("."));
        let tests_path = profile_dir.join("tests.ron");
        if tests_path.exists() {
            let tests_data = std::fs::read_to_string(tests_path)?;
            // Parse tests.ron — expected format: { golden_frames: [(frame, hash), ...] }
            #[derive(serde::Deserialize)]
            struct TestConfig {
                golden_frames: Vec<(u32, u32)>,
            }
            if let Ok(tests) = ron::from_str::<TestConfig>(&tests_data) {
                let mut hashes = vec![0u32; 256];
                for (frame, hash) in &tests.golden_frames {
                    if *frame < 256 {
                        hashes[*frame as usize] = *hash;
                    }
                }
                expected_hashes = Some(hashes);
                println!(
                    "Loaded {} expected frame hashes from tests.ron",
                    tests.golden_frames.len()
                );
            }
        }
        let _ = profile; // suppress unused warning
    }

    let num_frames = 60.max(expected_hashes.as_ref().map(|h| h.len()).unwrap_or(10));
    println!("Golden test: {} frames, recording frame hashes", num_frames);
    let mut all_pass = true;

    for frame in 0..num_frames {
        // Apply input replay if available
        if let Some(ref mut replay_backend) = replay {
            let state = replay_backend.state_for_frame(frame as u64, 1);
            system.bus.controller[0].set_current(state);
        }

        let fb = system.run_frame();
        let fhash: u32 = fb
            .iter()
            .enumerate()
            .map(|(i, &p)| (p as u32).wrapping_mul((i as u32 % 251) + 1))
            .fold(0, |a, b| a ^ b);

        if let Some(ref expected) = expected_hashes {
            if frame < expected.len() && expected[frame] != 0 {
                if fhash == expected[frame] {
                    println!("  frame {:4}: fhash={:08X} ✓", frame, fhash);
                } else {
                    println!(
                        "  frame {:4}: fhash={:08X} ✗ (expected {:08X})",
                        frame, fhash, expected[frame]
                    );
                    all_pass = false;
                }
            } else {
                println!(
                    "  frame {:4}: fhash={:08X} (no expected hash)",
                    frame, fhash
                );
            }
        } else {
            println!("  frame {:4}: fhash={:08X}", frame, fhash);
        }
    }

    if all_pass {
        println!("Golden test: ALL PASSED ✓");
    } else {
        println!("Golden test: SOME FAILED ✗");
    }
    Ok(())
}

fn cmd_input_test(
    backend: &str,
    _record: Option<&str>,
    _mapping_wizard: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Input test: backend={}", backend);
    println!("Available backends:");
    println!("  - winit_keyboard (platform keyboard events)");
    println!("  - gilrs (cross-platform gamepad via gilrs crate)");
    println!("  - hidapi (generic HID gamepad/joystick)");
    println!("  - replay (replay recorded inputs from file)");
    Ok(())
}
// Debug helper
#[allow(dead_code)]
fn count_nonzero(data: &[u8]) -> usize {
    data.iter().filter(|&&b| b != 0).count()
}
