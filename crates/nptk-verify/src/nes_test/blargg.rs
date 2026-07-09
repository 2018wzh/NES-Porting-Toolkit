//! blargg's NES test ROMs 运行器
//!
//! blargg 的测试 ROM 套件涵盖 CPU、PPU、APU 等各个方面。
//! 测试 ROM 通过调色板颜色或特定内存地址报告结果：
//!
//! - **成功**: 屏幕变为绿色（或特定成功颜色）
//! - **失败**: 屏幕变为红色，并显示失败编号
//! - **详细结果**: 通过 $6000-$7FFF 或 PPU 调色板输出
//!
//! # 可用测试
//!
//! - `cpu_test.nes` — CPU 指令集全面测试
//! - `ppu_test.nes` — PPU 渲染测试
//! - `apu_test.nes` — APU 音频测试
//! - `controller_test.nes` — 控制器读取测试
//! - `vbl_nmi_timing.nes` — VBlank/NMI 时序测试

use nptk_core::bus::NesBus;
use nptk_core::system::NesSystem;

use super::create_test_bus;

/// blargg 测试结果
#[derive(Debug, Clone)]
pub enum BlarggTestResult {
    /// 通过
    Passed,
    /// 失败（包含失败编号和描述）
    Failed(u8, String),
    /// 无法确定结果
    Unknown(String),
}

/// blargg 测试运行器
pub struct BlarggRunner {
    system: NesSystem,
    /// 最大帧数
    max_frames: u32,
    /// 当前帧
    current_frame: u32,
    /// 是否已完成
    done: bool,
    /// 结果
    result: Option<BlarggTestResult>,
}

impl BlarggRunner {
    /// 创建新的 blargg 测试运行器
    ///
    /// `max_frames` — 最大运行帧数（防止无限循环）
    pub fn new(rom_path: &str, max_frames: u32) -> Result<Self, Box<dyn std::error::Error>> {
        let bus = create_test_bus(rom_path)?;
        let system = NesSystem::new(bus);

        Ok(BlarggRunner {
            system,
            max_frames,
            current_frame: 0,
            done: false,
            result: None,
        })
    }

    /// 运行一帧
    pub fn step_frame(&mut self) -> bool {
        if self.done {
            return false;
        }

        self.system.run_frame();
        self.current_frame += 1;

        // 检查测试结果
        self.check_result();

        if self.current_frame >= self.max_frames {
            self.done = true;
            if self.result.is_none() {
                self.result = Some(BlarggTestResult::Unknown(format!(
                    "Reached max frames ({}) without completion",
                    self.max_frames
                )));
            }
        }

        !self.done
    }

    /// 运行所有帧直到完成
    pub fn run_all(&mut self) {
        while !self.done {
            self.step_frame();
        }
    }

    /// 检查测试结果
    ///
    /// blargg 测试 ROM 通常通过以下方式报告结果：
    /// 1. 调色板颜色（绿色=通过，红色=失败）
    /// 2. $6000-$7FFF 区域的状态码
    /// 3. PPU 帧缓冲中的特定像素
    fn check_result(&mut self) {
        // 方法 1: 检查调色板
        // blargg 测试通过后通常将背景色设为绿色 ($1A)
        // 失败时设为红色 ($16)
        let bg_color = self.system.cpu.memory.ppu.read_palette(0x3F00);

        // 方法 2: 检查 $6000 区域的状态码（某些测试使用）
        let status = self.system.cpu.memory.cpu_read(0x6000);

        // 方法 3: 检查帧缓冲中心像素
        let fb = *self.system.cpu.memory.ppu.frame();
        let center_idx = 128 + 120 * 256; // 近似中心
        let center_pixel = fb[center_idx];

        // 判断逻辑
        if status == 0x01 || bg_color == 0x1A {
            // 通过标志
            self.result = Some(BlarggTestResult::Passed);
            self.done = true;
        } else if status != 0 || bg_color == 0x16 || center_pixel == 0x16 {
            // 失败标志
            let fail_code = status;
            self.result = Some(BlarggTestResult::Failed(
                fail_code,
                format!(
                    "Status=${:02X}, BG=${:02X}, Center=${:02X}",
                    status, bg_color, center_pixel
                ),
            ));
            self.done = true;
        }

        // 某些测试在帧数达到特定值时完成
        // 这里不做额外判断，由 max_frames 兜底
    }

    /// 获取测试结果
    pub fn result(&self) -> Option<&BlarggTestResult> {
        self.result.as_ref()
    }

    /// 获取当前帧数
    pub fn current_frame(&self) -> u32 {
        self.current_frame
    }

    /// 获取 NES 系统的引用
    pub fn system(&self) -> &NesSystem {
        &self.system
    }

    /// 获取 NES 系统的可变引用
    pub fn system_mut(&mut self) -> &mut NesSystem {
        &mut self.system
    }
}

/// 运行 blargg 测试并返回结果
pub fn run_blargg_test(rom_path: &str, max_frames: u32) -> BlarggTestResult {
    let mut runner = match BlarggRunner::new(rom_path, max_frames) {
        Ok(r) => r,
        Err(e) => return BlarggTestResult::Unknown(format!("Failed to create runner: {}", e)),
    };
    runner.run_all();
    runner
        .result()
        .cloned()
        .unwrap_or(BlarggTestResult::Unknown(
            "No result determined".to_string(),
        ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 检查 blargg 测试 ROM 是否存在
    fn find_test_rom(name: &str) -> Option<String> {
        let paths = [
            format!("tests/roms/blargg/{}", name),
            format!("../tests/roms/blargg/{}", name),
            format!("../../tests/roms/blargg/{}", name),
        ];
        for p in &paths {
            if std::path::Path::new(p).exists() {
                return Some(p.to_string());
            }
        }
        None
    }

    #[test]
    #[ignore] // 需要 blargg 测试 ROM 文件
    fn test_cpu_test_runs() {
        let rom = find_test_rom("cpu_test.nes").expect("cpu_test.nes not found");
        let result = run_blargg_test(&rom, 600);
        match &result {
            BlarggTestResult::Passed => println!("CPU test: PASSED"),
            BlarggTestResult::Failed(code, desc) => {
                println!("CPU test: FAILED (code={}, {})", code, desc)
            }
            BlarggTestResult::Unknown(desc) => println!("CPU test: UNKNOWN ({})", desc),
        }
    }

    #[test]
    #[ignore] // 需要 blargg 测试 ROM 文件
    fn test_ppu_test_runs() {
        let rom = find_test_rom("ppu_test.nes").expect("ppu_test.nes not found");
        let result = run_blargg_test(&rom, 600);
        match &result {
            BlarggTestResult::Passed => println!("PPU test: PASSED"),
            BlarggTestResult::Failed(code, desc) => {
                println!("PPU test: FAILED (code={}, {})", code, desc)
            }
            BlarggTestResult::Unknown(desc) => println!("PPU test: UNKNOWN ({})", desc),
        }
    }
}
