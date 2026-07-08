//! PPU 桥接 — 连接 PPU 兼容层与原生渲染器

use crate::runtime::PpuEventSink;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PpuMode { Compat, TilemapNative }

pub struct PpuBridge {
    pub mode: PpuMode,
    pub frame_ready: bool,
    pub frame_count: u64,
    framebuffer: Box<[u8; 256 * 240]>,
}

impl PpuBridge {
    pub fn new() -> Self {
        PpuBridge {
            mode: PpuMode::Compat,
            frame_ready: false,
            frame_count: 0,
            framebuffer: Box::new([0u8; 256 * 240]),
        }
    }

    pub fn framebuffer(&self) -> &[u8; 256 * 240] { &self.framebuffer }
}

impl PpuEventSink for PpuBridge {
    fn on_frame_complete(&mut self, fb: &[u8; 256 * 240]) {
        self.framebuffer = Box::new(*fb);
        self.frame_ready = true;
        self.frame_count += 1;
    }
}

impl Default for PpuBridge {
    fn default() -> Self { Self::new() }
}
