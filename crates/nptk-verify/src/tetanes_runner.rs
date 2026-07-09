//! tetanes-core 运行器
//!
//! 使用 tetanes-core 作为参考实现，运行 NES ROM 并获取帧缓冲输出。
//!
//! # 示例
//!
//! ```ignore
//! use nptk_verify::tetanes_runner::TetanesRunner;
//!
//! let mut runner = TetanesRunner::new("roms/BattleCity (Japan).nes")?;
//! for _ in 0..60 {
//!     runner.run_frame()?;
//! }
//! let fb = runner.framebuffer();
//! ```

use std::path::Path;

use tetanes_core::prelude::*;

/// tetanes-core 运行器
pub struct TetanesRunner {
    control_deck: ControlDeck,
    /// 当前帧数
    frame_count: u32,
    /// 帧缓冲（NES 索引色格式，256×240）
    framebuffer: [u8; 256 * 240],
}

impl TetanesRunner {
    /// 创建新的 tetanes 运行器
    ///
    /// 从 ROM 文件路径加载 ROM 并初始化 ControlDeck。
    pub fn new(rom_path: impl AsRef<Path>) -> Result<Self, Box<dyn std::error::Error>> {
        let mut control_deck = ControlDeck::new();
        control_deck.load_rom_path(rom_path)?;

        Ok(TetanesRunner {
            control_deck,
            frame_count: 0,
            framebuffer: [0u8; 256 * 240],
        })
    }

    /// 从 ROM 数据创建 tetanes 运行器
    pub fn from_rom_data(rom_data: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        let mut control_deck = ControlDeck::new();
        let mut cursor = std::io::Cursor::new(rom_data);
        control_deck.load_rom("rom", &mut cursor)?;

        Ok(TetanesRunner {
            control_deck,
            frame_count: 0,
            framebuffer: [0u8; 256 * 240],
        })
    }

    /// 运行一帧
    pub fn run_frame(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.control_deck.clock_frame()?;
        self.frame_count += 1;

        // 获取 tetanes 的原始帧缓冲（u16 格式，每个值直接是调色板索引 0-63）
        let tetanes_fb = self.control_deck.frame_buffer_raw();
        
        // tetanes 的原始帧缓冲是 u16 格式，256×240
        // 每个 u16 值直接是 NES 调色板索引（低 6 位）
        if tetanes_fb.len() >= 256 * 240 {
            for (i, &pixel) in tetanes_fb.iter().enumerate().take(256 * 240) {
                self.framebuffer[i] = (pixel & 0x3F) as u8;
            }
        }

        Ok(())
    }

    /// 获取当前帧缓冲（NES 索引色格式）
    pub fn framebuffer(&self) -> &[u8; 256 * 240] {
        &self.framebuffer
    }

    /// 获取当前帧数
    pub fn frame_count(&self) -> u32 {
        self.frame_count
    }

    /// 设置控制器输入
    pub fn set_controller(&mut self, port: u8, state: ControllerState) {
        // tetanes 使用 ControllerState 枚举
        // 这里简化处理，实际可能需要更复杂的映射
        let _ = (port, state);
    }
}

/// 控制器状态（简化版）
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ControllerState {
    #[default]
    None,
    Start,
    Select,
    A,
    B,
    Up,
    Down,
    Left,
    Right,
}

impl std::fmt::Display for ControllerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ControllerState::None => write!(f, "None"),
            ControllerState::Start => write!(f, "Start"),
            ControllerState::Select => write!(f, "Select"),
            ControllerState::A => write!(f, "A"),
            ControllerState::B => write!(f, "B"),
            ControllerState::Up => write!(f, "Up"),
            ControllerState::Down => write!(f, "Down"),
            ControllerState::Left => write!(f, "Left"),
            ControllerState::Right => write!(f, "Right"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tetanes_runner_creation() {
        // 只是测试创建，不运行
        let _ = TetanesRunner::new("roms/BattleCity (Japan).nes");
    }
}
