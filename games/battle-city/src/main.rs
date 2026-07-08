#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Battle City — NES 原生移植
//!
//! 游戏特定逻辑入口。所有平台通用代码（窗口/渲染/音频/输入/调试 UI）
//! 由 `nptk-native-runtime::app` 模块提供。
//!
//! 本文件只包含：
//! - ROM 加载与 NES 系统初始化
//! - AOT 重编译块注册
//! - 帧回调（执行 NES 帧 + 音频混音 + 调试数据收集）
//! - 原生渲染数据上传（CHR/nametable/OAM）

use nptk::debug::DebugData;
use nptk::prelude::*;
use nptk::render::RenderMode;
use nptk::runtime::{AudioEventSink, PpuEventSink, RecompiledRuntime};
use nptk::{ExecMode, FrameContext, GameHandlers, NesApp};

// ── ROM 嵌入 ─────────────────────────────────────────────────────────────

/// Battle City ROM 数据（编译时嵌入）
/// 通过 `include_bytes!` 将 ROM 文件嵌入到二进制中，
/// 运行时无需外部 ROM 文件。
const ROM_DATA: &[u8] = include_bytes!("../../../roms/BattleCity (Japan).nes");

// ── 游戏结构体 ───────────────────────────────────────────────────────────

/// Battle City 游戏 — 实现 `GameHandlers` trait
struct BattleCityGame {
    system: NesSystem,
    recompiled: Option<RecompiledRuntime>,
    exec_mode: ExecMode,
}

impl BattleCityGame {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        // 初始化 mapper 注册表
        nptk::mapper::init();

        let rom = parse_rom(ROM_DATA)?;
        let mapper = nptk::mapper::create_mapper(rom.header.mapper_id, &rom)
            .ok_or_else(|| format!("Mapper {} not supported", rom.header.mapper_id))?;
        let cartridge = nptk::mapper::Cartridge::new_simple(
            nptk::mapper::CartridgeMetadata {
                mapper_id: rom.header.mapper_id,
                submapper_id: rom.header.submapper_id,
                prg_rom_size: rom.header.prg_rom_size,
                chr_rom_size: rom.header.chr_rom_size,
                has_sram: rom.header.has_sram,
                has_trainer: rom.header.has_trainer,
                battery_backed: false,
            },
            rom.prg_rom.clone(),
            nptk::mapper::ChrStorage::Rom(rom.chr_rom.clone().unwrap_or_default()),
            mapper,
        );

        println!("Battle City — NES Porting Toolkit");
        println!(
            "  Mapper: {}, PRG: {}KB, CHR: {}KB, Mirroring: {:?}",
            rom.header.mapper_id,
            rom.header.prg_rom_size / 1024,
            rom.header.chr_rom_size / 1024,
            rom.header.mirroring
        );
        println!("  Controls: Z/X=AB, Arrows=DPad, Enter=Start, RShift=Select");
        println!("  F1=Render mode, F2=Debug UI, Space=Pause, Esc=Exit");

        let bus = NesBusImpl::new(cartridge);
        let system = NesSystem::new(bus);

        Ok(BattleCityGame {
            system,
            recompiled: None,
            exec_mode: ExecMode::Interpreter,
        })
    }
}

// ── GameHandlers 实现 ────────────────────────────────────────────────────

impl GameHandlers for BattleCityGame {
    fn window_title(&self) -> &str {
        "Battle City — NES Porting Toolkit"
    }

    fn run_frame(&mut self, ctx: &mut FrameContext) {
        // 首次帧时初始化重编译运行时
        if self.recompiled.is_none() {
            // 创建一个新的 NesBusImpl 用于重编译运行时
            let rom = parse_rom(ROM_DATA).unwrap();
            let mapper = nptk::mapper::create_mapper(rom.header.mapper_id, &rom)
                .expect("Mapper not registered");
            let cartridge = nptk::mapper::Cartridge::new_simple(
                nptk::mapper::CartridgeMetadata {
                    mapper_id: rom.header.mapper_id,
                    submapper_id: rom.header.submapper_id,
                    prg_rom_size: rom.header.prg_rom_size,
                    chr_rom_size: rom.header.chr_rom_size,
                    has_sram: rom.header.has_sram,
                    has_trainer: rom.header.has_trainer,
                    battery_backed: false,
                },
                rom.prg_rom.clone(),
                nptk::mapper::ChrStorage::Rom(rom.chr_rom.clone().unwrap_or_default()),
                mapper,
            );
            let bus = NesBusImpl::new(cartridge);
            struct NullSink;
            impl PpuEventSink for NullSink {}
            impl AudioEventSink for NullSink {}
            let mut rt = RecompiledRuntime::new(bus, Box::new(NullSink), Box::new(NullSink));
            let dispatch = nptk_battle_city::nes_blocks::get_dispatch();
            for (addr, func) in dispatch {
                rt.add_cabi_block(addr, func);
            }
            tracing::info!(
                "Registered {} AOT blocks (statically linked)",
                rt.cabi_dispatch.len()
            );
            self.recompiled = Some(rt);
        }

        // 同步输入状态到 NES 系统
        self.system.bus.controller[0].set_current(*ctx.input_state);

        // 执行 NES 帧
        let fb = match self.exec_mode {
            ExecMode::Recompiled => {
                if let Some(ref mut rt) = self.recompiled {
                    rt.bus.controller[0].set_current(*ctx.input_state);
                    rt.run_frame();
                    *rt.framebuffer()
                } else {
                    *self.system.run_frame()
                }
            }
            ExecMode::Interpreter => *self.system.run_frame(),
        };

        // 更新帧缓冲区
        *ctx.framebuffer = fb;

        // 音频混音
        if let Some(ref tx) = *ctx.audio_tx {
            let apu = &self.system.bus.apu;
            let p1 = apu.pulse1_output();
            let p2 = apu.pulse2_output();
            let tri = apu.triangle_output();
            let noise = apu.noise_output();
            ctx.apu_mixer
                .mix(nptk::system::CPU_CYCLES_PER_FRAME, p1, p2, tri, noise);
            let samples = ctx.apu_mixer.drain_samples();
            for s in samples {
                let _ = tx.send(s);
            }
        }

        // 原生渲染数据上传
        if *ctx.render_mode == RenderMode::Native {
            // CHR 数据由渲染器在外部处理
        }

        // 调试数据收集（发送到 FLTK 调试窗口）
        if *ctx.show_debug {
            let cpu = &self.system.cpu;
            let ppu = &self.system.bus.ppu;

            let mut hash: u64 = 0;
            for (i, &b) in fb.iter().enumerate() {
                hash = hash.wrapping_mul(31).wrapping_add(b as u64);
                if i % 101 == 0 {
                    hash = hash.rotate_left(7);
                }
            }

            ctx.debug_collector.update(DebugData {
                cpu_a: cpu.a,
                cpu_x: cpu.x,
                cpu_y: cpu.y,
                cpu_sp: cpu.sp,
                cpu_pc: cpu.pc,
                cpu_flag_c: cpu.status.carry,
                cpu_flag_z: cpu.status.zero,
                cpu_flag_i: cpu.status.interrupt_disable,
                cpu_flag_d: cpu.status.decimal,
                cpu_flag_v: cpu.status.overflow,
                cpu_flag_n: cpu.status.negative,
                cpu_cycles: cpu.cycles,
                cpu_cycle_count: self.system.cpu_cycle,
                ppu_ctrl: ppu.ctrl,
                ppu_mask: ppu.mask,
                ppu_status: ppu.status,
                ppu_scanline: ppu.scanline,
                ppu_cycle: ppu.cycle,
                ppu_dot: self.system.ppu_dot,
                frame_count: self.system.frame_count,
                frame_hash: hash,
                ram: Some(*self.system.ram()),
            });
        }
    }
}

// ── 入口 ──────────────────────────────────────────────────────────────────

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let game = BattleCityGame::new()?;
    NesApp::new(game).run()
}
