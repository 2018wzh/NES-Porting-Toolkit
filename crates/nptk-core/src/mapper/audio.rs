//! ExpansionAudio trait — 扩展音频接口
//!
//! 某些 Mapper（如 VRC6、VRC7、MMC5、Namco 163、Sunsoft 5B、FDS）
//! 包含额外的音频通道。此 trait 允许 Mapper 暴露扩展音频输出。

/// 扩展音频接口
pub trait ExpansionAudio {
    /// 返回音频采样率（Hz）
    fn sample_rate(&self) -> u32;

    /// 渲染音频数据到缓冲区
    ///
    /// `buffer` 中的每个元素是一个 f32 采样点，范围 [-1.0, 1.0]。
    /// 实现应填充 buffer 的前 N 个采样点，并返回实际填充的采样数。
    fn render(&mut self, buffer: &mut [f32]) -> usize;
}