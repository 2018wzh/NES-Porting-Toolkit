//! Battle City — 构建时 AOT 编译（静态链接）
//!
//! 在 cargo build 时自动读取 ROM，通过 Cranelift 编译为原生目标文件 (.o)，
//! 打包为静态库 (.a)，然后静态链接到最终二进制。
//! Cranelift 生成的代码通过 `extern "C"` 函数 `nes_read8`/`nes_write8` 访问 NES 内存，
//! 这些函数由 `nptk-native-runtime` crate 提供。

use std::path::Path;
use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 通过环境变量 NES_AOT=0 可禁用
    let enable_aot = std::env::var("NES_AOT").unwrap_or_else(|_| "1".to_string());
    if enable_aot != "1" {
        println!(
            "cargo:warning=NES AOT compilation disabled (NES_AOT={})",
            enable_aot
        );
        return Ok(());
    }

    // 定位 ROM 文件
    let rom_path = find_rom()?;
    println!(
        "cargo:warning=Building AOT blocks from ROM: {}",
        rom_path.display()
    );

    // 读取并解析 ROM
    let rom_data = std::fs::read(&rom_path)?;
    let rom = match nptk_core::rom::parse_rom(&rom_data) {
        Ok(r) => r,
        Err(e) => {
            println!("cargo:warning=Failed to parse ROM: {}. AOT disabled.", e);
            return Ok(());
        }
    };

    // 只支持 Mapper 0 (NROM)
    if rom.header.mapper_id != 0 {
        println!(
            "cargo:warning=Mapper {} AOT: blocks discovered from linear PRG-ROM. \
             Bank-switched code will fall back to interpreter.",
            rom.header.mapper_id
        );
    }

    // 提取向量表
    let prg = &rom.prg_rom;
    let end = prg.len();
    let nmi_vec = prg[end - 6] as u16 | ((prg[end - 5] as u16) << 8);
    let reset_vec = prg[end - 4] as u16 | ((prg[end - 3] as u16) << 8);
    let irq_vec = prg[end - 2] as u16 | ((prg[end - 1] as u16) << 8);

    // BFS 发现基本块
    let to_offset = |addr: u16| -> Option<usize> {
        if addr < 0x8000 {
            None
        } else {
            Some((addr as usize - 0x8000) % prg.len())
        }
    };

    fn insn_len(op: u8) -> u16 {
        match op {
            0x20 | 0x4C | 0x6C | 0x0D | 0x0E | 0x1D | 0x1E | 0x2D | 0x2E | 0x3D | 0x3E | 0x4D
            | 0x4E | 0x5D | 0x5E | 0x6D | 0x6E | 0x7D | 0x7E | 0x8D | 0x8E | 0x9D | 0x9E | 0xAD
            | 0xAE | 0xBD | 0xBE | 0xCD | 0xCE | 0xDD | 0xDE | 0xED | 0xEE | 0xFD | 0xFE | 0x0C
            | 0x1C | 0x2C | 0x3C | 0x5C | 0x7C | 0x8C | 0xAC | 0xBC | 0xCC | 0xDC | 0xEC | 0xFC
            | 0x19 | 0x1B | 0x39 | 0x3B | 0x59 | 0x5B | 0x79 | 0x7B | 0x99 | 0x9B | 0xB9 | 0xBB
            | 0xD9 | 0xDB | 0xF9 | 0xFB | 0x0F | 0x1F | 0x2F | 0x3F | 0x4F | 0x5F | 0x6F | 0x7F
            | 0x8F | 0x9F | 0xAF | 0xBF | 0xCF | 0xDF | 0xEF | 0xFF => 3,
            0x00 | 0x08 | 0x18 | 0x28 | 0x38 | 0x48 | 0x58 | 0x68 | 0x78 | 0x88 | 0x98 | 0xA8
            | 0xB8 | 0xC8 | 0xD8 | 0xE8 | 0xF8 | 0x0A | 0x2A | 0x4A | 0x6A | 0x8A | 0x9A | 0xAA
            | 0xBA | 0xCA | 0xEA | 0x40 | 0x60 | 0x1A | 0x3A | 0x5A | 0x7A | 0xDA | 0xFA => 1,
            _ => 2,
        }
    }

    use std::collections::{HashSet, VecDeque};
    let mut visited: HashSet<u16> = HashSet::new();
    let mut queue: VecDeque<u16> = VecDeque::from([reset_vec, nmi_vec, irq_vec]);

    let mut block_data: Vec<(u16, Vec<u8>)> = Vec::new();

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

            for i in 0..len {
                if let Some(&byte) = prg.get(off + i as usize) {
                    block_bytes.push(byte);
                }
            }

            let is_terminal = match b {
                0x60 | 0x40 | 0x00 => true,
                0x4C => {
                    let lo = prg.get(off + 1).copied().unwrap_or(0) as u16;
                    let hi = prg.get(off + 2).copied().unwrap_or(0) as u16;
                    let target = lo | (hi << 8);
                    if target >= 0x8000 {
                        queue.push_back(target);
                    }
                    true
                }
                0x6C => true,
                0x20 => {
                    let lo = prg.get(off + 1).copied().unwrap_or(0) as u16;
                    let hi = prg.get(off + 2).copied().unwrap_or(0) as u16;
                    let target = lo | (hi << 8);
                    if target >= 0x8000 {
                        queue.push_back(target);
                    }
                    queue.push_back(next);
                    true
                }
                0x10 | 0x30 | 0x50 | 0x70 | 0x90 | 0xB0 | 0xD0 | 0xF0 => {
                    let offset = prg.get(off + 1).copied().unwrap_or(0) as i8;
                    let taken = ((next as i32) + (offset as i32)) as u16;
                    if taken >= 0x8000 {
                        queue.push_back(taken);
                    }
                    queue.push_back(next);
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
            block_data.push((addr, block_bytes));
        }
    }

    println!("cargo:warning=Found {} basic blocks", block_data.len());

    if block_data.is_empty() {
        println!("cargo:warning=No blocks found. AOT disabled.");
        return Ok(());
    }

    // 使用 Cranelift AOT 编译
    let mut aot = nptk_recompiler::codegen::CraneliftAot::new()
        .map_err(|e| format!("CraneliftAot::new: {}", e))?;

    for &(addr, ref bytes) in &block_data {
        let ir_ops = nptk_recompiler::ir_builder::IrBuilder::lift_block(bytes, addr);
        aot.compile_block(addr, &ir_ops)
            .map_err(|e| format!("compile block ${:04X}: {}", addr, e))?;
    }

    // 完成编译，获取 .o 字节和块信息
    let (obj_bytes, compiled_blocks, block_names) =
        aot.finish().map_err(|e| format!("finish: {}", e))?;

    // 输出目录: target/{profile}/
    let out_dir = std::env::var("OUT_DIR").map_err(|e| format!("OUT_DIR: {}", e))?;
    let target_dir = Path::new(&out_dir)
        .parent()
        .and_then(|p| p.parent())
        .ok_or_else(|| "Cannot determine target directory".to_string())?;

    // 写入 .o 文件
    let obj_path = target_dir.join("nes_blocks_battle_city.o");
    std::fs::write(&obj_path, &obj_bytes).map_err(|e| format!("write .o: {}", e))?;

    // 打包为静态库 (.a)
    // Cranelift 生成的 .o 引用了 nes_read8/nes_write8 外部符号，
    // 这些符号由 nptk-native-runtime crate 提供（#[no_mangle] extern "C"）。
    // 静态链接时，链接器会从 nptk-native-runtime 中解析这些符号。
    let lib_path = target_dir.join("libnes_blocks_battle_city.a");
    let ar_result = Command::new("ar")
        .args(&["crs"])
        .arg(&lib_path)
        .arg(&obj_path)
        .output();

    if let Ok(output) = ar_result {
        if output.status.success() {
            println!(
                "cargo:warning=Static library created: {}",
                lib_path.display()
            );
            // 告诉 cargo 链接到这个静态库
            println!("cargo:rustc-link-search={}", target_dir.display());
            println!("cargo:rustc-link-lib=static=nes_blocks_battle_city");
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            println!(
                "cargo:warning=ar failed: {}. Trying MSVC lib.exe...",
                stderr
            );
            try_msvc_lib(&obj_path, &lib_path);
        }
    } else {
        println!("cargo:warning=ar not found. Trying MSVC lib.exe...");
        try_msvc_lib(&obj_path, &lib_path);
    }

    // 生成 Rust 绑定代码（静态 extern 声明 + dispatch 表）
    let bindings = generate_bindings(&compiled_blocks, &block_names);
    let bindings_path = Path::new(&out_dir).join("nes_blocks.rs");
    std::fs::write(&bindings_path, &bindings).map_err(|e| format!("write bindings: {}", e))?;

    println!(
        "cargo:warning=AOT compilation complete: {} blocks statically linked",
        compiled_blocks.len()
    );
    Ok(())
}

/// 尝试 MSVC lib.exe 创建静态库
fn try_msvc_lib(obj_path: &Path, lib_path: &Path) {
    if let Ok(output) = Command::new("lib.exe")
        .args(&["/NOLOGO", "/OUT:"])
        .arg(lib_path)
        .arg(obj_path)
        .output()
    {
        if output.status.success() {
            println!("cargo:warning=Static library created with MSVC lib.exe");
            return;
        }
    }
    // 如果 ar 和 lib.exe 都失败，直接链接 .o 文件
    println!("cargo:warning=Static library creation failed. Linking .o directly.");
    println!(
        "cargo:rustc-link-search={}",
        obj_path.parent().unwrap().display()
    );
    // 对于 GCC 链接器，可以直接链接 .o
    if cfg!(target_os = "windows") {
        // 在 Windows 上，尝试将 .o 重命名为 .obj 并链接
        let obj_renamed = obj_path.with_extension("obj");
        let _ = std::fs::copy(obj_path, &obj_renamed);
        println!("cargo:rustc-link-lib=nes_blocks_battle_city.obj");
    }
}

/// 查找 ROM 文件
fn find_rom() -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    // 尝试多个可能的路径
    let candidates = [
        "roms/BattleCity (Japan).nes",
        "../roms/BattleCity (Japan).nes",
        "../../roms/BattleCity (Japan).nes",
        "roms/battle_city.nes",
        "../roms/battle_city.nes",
    ];

    for path in &candidates {
        if std::path::Path::new(path).exists() {
            return Ok(std::path::PathBuf::from(path));
        }
    }

    // 尝试从环境变量读取
    if let Ok(path) = std::env::var("NES_ROM") {
        if std::path::Path::new(&path).exists() {
            return Ok(std::path::PathBuf::from(path));
        }
    }

    Err("ROM file not found. Place BattleCity (Japan).nes in roms/ directory or set NES_ROM env var.".into())
}

/// 生成 Rust 绑定代码（静态 extern 声明 + dispatch 表）
///
/// 生成的内容：
/// 1. 每个基本块的 `extern "C"` 函数声明
/// 2. `get_dispatch()` 函数返回 `Vec<(u16, CAbiBlockFn)>`
///
/// 这些符号在链接时从 .a 静态库中解析。
fn generate_bindings(
    blocks: &[nptk_recompiler::codegen::CompiledBlock],
    _block_names: &std::collections::HashMap<u16, String>,
) -> String {
    let mut src = String::new();
    src.push_str("// Auto-generated bindings for recompiled NES blocks\n");
    src.push_str("// Statically linked via build.rs\n\n");

    src.push_str("use nptk_native_runtime::runtime::CAbiBlockFn;\n");
    src.push_str("use nptk_native_runtime::runtime::NativeCpuState;\n");
    src.push_str("use nptk_core::bus::NesBusImpl;\n\n");

    // 为每个块生成 extern "C" 声明
    src.push_str("// ── Extern function declarations (from Cranelift AOT .a) ──\n");
    for block in blocks {
        src.push_str(&format!(
            "unsafe extern \"C\" {{ pub fn {}(bus: *mut NesBusImpl, cpu: *mut NativeCpuState) -> u32; }}\n",
            block.name
        ));
    }

    // 生成 dispatch 表
    src.push_str("\n/// Get the dispatch table of all compiled blocks\n");
    src.push_str("pub fn get_dispatch() -> Vec<(u16, CAbiBlockFn)> {\n");
    src.push_str("    vec![\n");
    for block in blocks {
        src.push_str(&format!(
            "        (0x{:04X}, {} as CAbiBlockFn),\n",
            block.address, block.name
        ));
    }
    src.push_str("    ]\n");
    src.push_str("}\n");

    src
}
