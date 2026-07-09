//! 状态桥接 — 游戏状态语义化 + 存档
//! 将 NES RAM 中的状态映射为可读的 GameState 字段

use serde::{Deserialize, Serialize};

/// 存档数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveState {
    pub version: u32,
    pub ram: Vec<u8>,
    pub pc: u16,
    pub sp: u8,
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub status: u8,
    pub ppu_ctrl: u8,
    pub ppu_mask: u8,
    pub frame_count: u64,
}

impl SaveState {
    pub fn new() -> Self {
        SaveState {
            version: 1,
            ram: vec![0u8; 0x800],
            pc: 0,
            sp: 0xFD,
            a: 0,
            x: 0,
            y: 0,
            status: 0x24,
            ppu_ctrl: 0,
            ppu_mask: 0,
            frame_count: 0,
        }
    }

    /// 从 NES 系统保存状态
    pub fn save_from(system: &nptk_core::system::NesSystem) -> Self {
        SaveState {
            version: 1,
            ram: system.ram().to_vec(),
            pc: system.cpu.registers.program_counter,
            sp: system.cpu.registers.stack_pointer.0,
            a: system.cpu.registers.accumulator,
            x: system.cpu.registers.index_x,
            y: system.cpu.registers.index_y,
            status: system.cpu.registers.status.bits(),
            ppu_ctrl: system.cpu.memory.ppu.ctrl,
            ppu_mask: system.cpu.memory.ppu.mask,
            frame_count: system.frame_count,
        }
    }

    /// 序列化为 JSON
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// 从 JSON 反序列化
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// 恢复到 NES 系统
    pub fn restore_to(&self, system: &mut nptk_core::system::NesSystem) {
        system.cpu.registers.accumulator = self.a;
        system.cpu.registers.index_x = self.x;
        system.cpu.registers.index_y = self.y;
        system.cpu.registers.stack_pointer.0 = self.sp;
        system.cpu.registers.program_counter = self.pc;
        system.cpu.registers.status = nptk_core::cpu_ref::MosStatus::from_bits_truncate(self.status);
        system.cpu.memory.ram.copy_from_slice(&self.ram);
        system.cpu.memory.ppu.ctrl = self.ppu_ctrl;
        system.cpu.memory.ppu.mask = self.ppu_mask;
        system.frame_count = self.frame_count;
    }
}

impl Default for SaveState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_save_state_roundtrip() {
        let state = SaveState::new();
        let json = state.to_json().unwrap();
        let restored = SaveState::from_json(&json).unwrap();
        assert_eq!(restored.version, 1);
        assert_eq!(restored.sp, 0xFD);
        assert_eq!(restored.status, 0x24);
    }
}
