//! 差异报告生成与多帧对比会话
//!
//! 提供 `ComparisonSession` 来驱动多帧对比，以及 `ComparisonReport`
//! 来汇总和输出结果。

use std::path::Path;

use nptk_core::bus::NesBusImpl;
use nptk_core::mapper::Cartridge;
use nptk_core::rom::NesRom;
use nptk_core::system::NesSystem;
use nptk_native_runtime::runtime::RecompiledRuntime;

use crate::compare::{
    DiffImageLayout, FB_PIXELS, FramebufferDiff, compare_framebuffers, diff_to_image, frame_hash,
};

/// 对比模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyMode {
    /// 解释器 vs 重编译（两者都用软件渲染）
    InterpreterVsRecompiled,
    /// 解释器软件渲染 vs 重编译 + WGPU Native 渲染
    /// （需要 GPU 上下文，当前暂不支持 GPU readback）
    SoftwareVsNative,
}

/// 多帧比较结果汇总
#[derive(Debug, Clone)]
pub struct ComparisonReport {
    /// 对比模式
    pub mode: VerifyMode,
    /// 总帧数
    pub total_frames: u32,
    /// 存在差异的帧数
    pub mismatched_frames: u32,
    /// 每帧比较结果
    pub per_frame: Vec<FramebufferDiff>,
    /// 参考帧哈希（解释器）
    pub frame_hashes_ref: Vec<u32>,
    /// 待验证帧哈希（重编译）
    pub frame_hashes_actual: Vec<u32>,
}

impl ComparisonReport {
    /// 所有帧是否完全一致
    pub fn all_identical(&self) -> bool {
        self.mismatched_frames == 0
    }

    /// 生成文本摘要
    pub fn summary(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("=== NES Porting Verification Report ===\n"));
        s.push_str(&format!("Mode: {:?}\n", self.mode));
        s.push_str(&format!("Total frames: {}\n", self.total_frames));
        s.push_str(&format!("Mismatched frames: {}\n", self.mismatched_frames));

        if self.all_identical() {
            s.push_str("Result: ALL PASSED ✓\n");
        } else {
            s.push_str("Result: SOME FAILED ✗\n");
            // 统计差异最大的几帧
            let mut with_diffs: Vec<(usize, &FramebufferDiff)> = self
                .per_frame
                .iter()
                .enumerate()
                .filter(|(_, d)| !d.is_identical())
                .collect();
            with_diffs.sort_by(|a, b| b.1.mismatched_pixels.cmp(&a.1.mismatched_pixels));

            s.push_str(&format!("\nTop mismatched frames:\n"));
            for (i, (frame_idx, diff)) in with_diffs.iter().take(10).enumerate() {
                s.push_str(&format!(
                    "  {}. Frame {:4}: {} / {} pixels mismatched ({:.2}%), max_diff={}, mean_diff={:.2}\n",
                    i + 1,
                    frame_idx,
                    diff.mismatched_pixels,
                    diff.total_pixels,
                    diff.ratio() * 100.0,
                    diff.max_diff,
                    diff.mean_diff,
                ));
            }
        }
        s
    }

    /// 将差异图像写入指定目录
    ///
    /// 返回写入的文件路径列表。
    pub fn write_diff_images(
        &self,
        output_dir: &Path,
        ref_frames: &[[u8; FB_PIXELS]],
        actual_frames: &[[u8; FB_PIXELS]],
    ) -> std::io::Result<Vec<std::path::PathBuf>> {
        std::fs::create_dir_all(output_dir)?;
        let mut written = Vec::new();

        for (i, diff) in self.per_frame.iter().enumerate() {
            if diff.is_identical() {
                continue;
            }

            let png_data = diff_to_image(
                diff,
                &ref_frames[i],
                &actual_frames[i],
                DiffImageLayout::SideBySide,
            );

            let filename = format!("frame_{:04}_diff.png", i);
            let path = output_dir.join(&filename);
            std::fs::write(&path, &png_data)?;
            written.push(path);
        }

        // 写入摘要报告
        let report_path = output_dir.join("report.txt");
        std::fs::write(&report_path, self.summary())?;
        written.push(report_path);

        Ok(written)
    }
}

/// 多帧对比会话
///
/// 同时持有解释器 `NesSystem` 和重编译 `RecompiledRuntime`，
/// 每帧同步输入并比较输出。
pub struct ComparisonSession {
    /// 解释器系统（参考）
    interpreter: NesSystem,
    /// 重编译运行时（待验证）
    recompiled: RecompiledRuntime,
    /// 对比模式
    mode: VerifyMode,
    /// 参考帧缓冲（解释器输出）
    ref_frames: Vec<[u8; FB_PIXELS]>,
    /// 待验证帧缓冲（重编译输出）
    actual_frames: Vec<[u8; FB_PIXELS]>,
    /// 每帧比较结果
    diffs: Vec<FramebufferDiff>,
}

impl ComparisonSession {
    /// 创建新的对比会话
    ///
    /// 从同一个 ROM 数据创建两个独立的 NES 系统实例。
    pub fn new(rom: &NesRom, cartridge: Cartridge, mode: VerifyMode) -> Self {
        // 解释器系统
        let bus_ref = NesBusImpl::new(cartridge);
        let interpreter = NesSystem::new(bus_ref);

        // 重编译运行时（使用同一个 ROM 数据创建独立的 bus）
        let bus_actual = NesBusImpl::new(Self::clone_cartridge(rom));
        let ppu_sink: Box<dyn nptk_native_runtime::runtime::PpuEventSink> =
            Box::new(nptk_native_runtime::ppu_bridge::PpuBridge::new());
        let audio_sink: Box<dyn nptk_native_runtime::runtime::AudioEventSink> =
            Box::new(NullAudioSink);
        let recompiled = RecompiledRuntime::new(bus_actual, ppu_sink, audio_sink);

        ComparisonSession {
            interpreter,
            recompiled,
            mode,
            ref_frames: Vec::new(),
            actual_frames: Vec::new(),
            diffs: Vec::new(),
        }
    }

    /// 从 ROM 数据克隆 Cartridge
    fn clone_cartridge(rom: &NesRom) -> Cartridge {
        let mapper = nptk_core::mapper::create_mapper(rom.header.mapper_id, rom)
            .expect("Mapper not registered");
        nptk_core::mapper::Cartridge::new_simple(
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
        )
    }

    /// 运行指定帧数的对比
    ///
    /// `input_provider` — 每帧提供输入状态的函数。
    /// 如果为 `None`，则使用空输入。
    pub fn run_frames(
        &mut self,
        num_frames: u32,
        input_provider: Option<&dyn Fn(u32) -> nptk_core::controller::NesControllerState>,
    ) -> ComparisonReport {
        self.ref_frames.clear();
        self.actual_frames.clear();
        self.diffs.clear();

        for frame in 0..num_frames {
            // 获取输入状态
            let input = input_provider.map(|f| f(frame)).unwrap_or_default();

            // 同步输入到两个系统
            self.interpreter.cpu.memory.controller[0].set_current(input);
            self.recompiled.cpu.memory.controller[0].set_current(input);

            // 执行帧
            let fb_ref = self.interpreter.run_frame();
            self.recompiled.run_frame();
            let fb_actual = self.recompiled.framebuffer();

            // 复制帧缓冲
            self.ref_frames.push(*fb_ref);
            self.actual_frames.push(*fb_actual);

            // 比较
            let diff = compare_framebuffers(fb_ref, fb_actual);
            self.diffs.push(diff);
        }

        self.build_report()
    }

    /// 构建报告
    fn build_report(&self) -> ComparisonReport {
        let mismatched_frames = self.diffs.iter().filter(|d| !d.is_identical()).count() as u32;

        let frame_hashes_ref: Vec<u32> = self.ref_frames.iter().map(|fb| frame_hash(fb)).collect();
        let frame_hashes_actual: Vec<u32> =
            self.actual_frames.iter().map(|fb| frame_hash(fb)).collect();

        ComparisonReport {
            mode: self.mode,
            total_frames: self.diffs.len() as u32,
            mismatched_frames,
            per_frame: self.diffs.clone(),
            frame_hashes_ref,
            frame_hashes_actual,
        }
    }

    /// 获取参考帧缓冲
    pub fn ref_frames(&self) -> &[[u8; FB_PIXELS]] {
        &self.ref_frames
    }

    /// 获取待验证帧缓冲
    pub fn actual_frames(&self) -> &[[u8; FB_PIXELS]] {
        &self.actual_frames
    }

    /// 获取解释器系统的引用
    pub fn interpreter(&self) -> &NesSystem {
        &self.interpreter
    }

    /// 获取解释器系统的可变引用
    pub fn interpreter_mut(&mut self) -> &mut NesSystem {
        &mut self.interpreter
    }

    /// 获取重编译运行时的引用
    pub fn recompiled(&self) -> &RecompiledRuntime {
        &self.recompiled
    }

    /// 获取重编译运行时的可变引用
    pub fn recompiled_mut(&mut self) -> &mut RecompiledRuntime {
        &mut self.recompiled
    }
}

/// 空音频接收器（用于对比会话中不需要音频输出）
struct NullAudioSink;
impl nptk_native_runtime::runtime::AudioEventSink for NullAudioSink {
    fn push_sample(&mut self, _sample: f32) {}
}

// ── 便捷函数 ──

/// 从 ROM 文件路径创建对比会话
pub fn create_session_from_rom(
    rom_path: &str,
    mode: VerifyMode,
) -> Result<ComparisonSession, Box<dyn std::error::Error>> {
    let data = std::fs::read(rom_path)?;
    let parsed = nptk_core::rom::parse_rom(&data)?;
    let cartridge = create_cartridge(&parsed)?;
    Ok(ComparisonSession::new(&parsed, cartridge, mode))
}

/// 从 ROM 数据创建 Cartridge
pub fn create_cartridge(rom: &NesRom) -> Result<Cartridge, Box<dyn std::error::Error>> {
    // 确保 mapper 已注册
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

// ── 单元测试 ──

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compare::FB_PIXELS;

    #[test]
    fn test_report_all_identical() {
        let report = ComparisonReport {
            mode: VerifyMode::InterpreterVsRecompiled,
            total_frames: 10,
            mismatched_frames: 0,
            per_frame: vec![
                FramebufferDiff {
                    total_pixels: FB_PIXELS,
                    mismatched_pixels: 0,
                    max_diff: 0,
                    mean_diff: 0.0,
                    mismatches: vec![],
                };
                10
            ],
            frame_hashes_ref: vec![0; 10],
            frame_hashes_actual: vec![0; 10],
        };
        assert!(report.all_identical());
        let summary = report.summary();
        assert!(summary.contains("ALL PASSED"));
    }

    #[test]
    fn test_report_with_mismatches() {
        let mut diffs = vec![
            FramebufferDiff {
                total_pixels: FB_PIXELS,
                mismatched_pixels: 0,
                max_diff: 0,
                mean_diff: 0.0,
                mismatches: vec![],
            };
            10
        ];
        diffs[3].mismatched_pixels = 100;
        diffs[3].max_diff = 5;
        diffs[3].mean_diff = 2.5;

        let report = ComparisonReport {
            mode: VerifyMode::InterpreterVsRecompiled,
            total_frames: 10,
            mismatched_frames: 1,
            per_frame: diffs,
            frame_hashes_ref: vec![0; 10],
            frame_hashes_actual: vec![0; 10],
        };
        assert!(!report.all_identical());
        let summary = report.summary();
        assert!(summary.contains("SOME FAILED"));
        assert!(summary.contains("Frame    3"));
    }
}
