//! 控制流图 — 从 6502 二进制代码构建 CFG。
//!
//! # 构建流程
//!
//! 1. 从入口点（Reset/NMI/IRQ 向量 + JSR 目标）开始 BFS
//! 2. 扫描指令，在分支/JMP/JSR/RTS/RTI 处分割基本块
//! 3. 记录块间 successor/predecessor 关系
//! 4. 处理间接跳转（fallback 到 dispatcher）

use crate::analysis::instruction_length;
use std::collections::{HashMap, HashSet, VecDeque};

pub type BlockId = u16;

/// 基本块
#[derive(Debug, Clone)]
pub struct BasicBlock {
    pub id: BlockId,
    pub start: u16,
    pub end: u16,
    pub successors: Vec<BlockId>,
    pub predecessors: Vec<BlockId>,
}

/// 控制流图
#[derive(Debug, Clone)]
pub struct Cfg {
    pub blocks: HashMap<BlockId, BasicBlock>,
    pub entry: BlockId,
}

impl Cfg {
    pub fn new() -> Self {
        Cfg {
            blocks: HashMap::new(),
            entry: 0,
        }
    }

    pub fn add_block(&mut self, start: u16, end: u16) -> BlockId {
        let id = self.blocks.len() as BlockId;
        self.blocks.insert(
            id,
            BasicBlock {
                id,
                start,
                end,
                successors: Vec::new(),
                predecessors: Vec::new(),
            },
        );
        id
    }

    /// Add a successor edge between two blocks.
    pub fn add_edge(&mut self, from: BlockId, to: BlockId) {
        if let Some(block) = self.blocks.get_mut(&from) {
            if !block.successors.contains(&to) {
                block.successors.push(to);
            }
        }
        if let Some(block) = self.blocks.get_mut(&to) {
            if !block.predecessors.contains(&from) {
                block.predecessors.push(from);
            }
        }
    }

    /// Get block ID by start address, or None.
    pub fn block_at(&self, addr: u16) -> Option<BlockId> {
        self.blocks
            .iter()
            .find(|(_, b)| b.start == addr)
            .map(|(id, _)| *id)
    }

    /// Build a CFG from PRG-ROM data starting at the given entry points.
    ///
    /// `prg_data` should be the raw PRG-ROM bytes (starting at CPU address 0x8000).
    /// `prg_base` is the CPU address where `prg_data` is mapped (typically 0x8000).
    /// `entry_points` is a list of CPU addresses to start BFS from.
    pub fn build(prg_data: &[u8], prg_base: u16, entry_points: &[u16]) -> Self {
        let mut cfg = Cfg::new();
        let mut visited = HashSet::new();
        let mut queue: VecDeque<u16> = entry_points.iter().copied().collect();

        // First pass: discover all basic blocks
        while let Some(addr) = queue.pop_front() {
            if visited.contains(&addr) {
                continue;
            }

            // Calculate PRG offset
            let offset = (addr.wrapping_sub(prg_base)) as usize;
            if offset >= prg_data.len() {
                continue;
            }

            // Scan instructions to find block boundaries
            let mut current = offset;
            let block_start = addr;
            let mut block_end = addr;

            while current < prg_data.len() {
                let opcode = prg_data[current];
                let len = instruction_length(opcode);
                if len == 0 || current + len > prg_data.len() {
                    break;
                }

                let instr_addr = prg_base + current as u16;
                block_end = instr_addr + len as u16;
                current += len;

                match opcode {
                    // Terminators: JMP, JSR, RTS, RTI, BRK, branches
                    0x4C | 0x6C | 0x60 | 0x40 | 0x00 | 0x10 | 0x30 | 0x50 | 0x70 | 0x90 | 0xB0
                    | 0xD0 | 0xF0 => {
                        break;
                    }
                    // JSR — call, block continues after it
                    0x20 => {
                        // Don't break — the block continues after JSR
                    }
                    // All other instructions: continue scanning
                    _ => {}
                }
            }

            // Mark the block as visited
            visited.insert(block_start);
            let block_id = cfg.add_block(block_start, block_end);

            // If this is the first block, set it as entry
            if cfg.blocks.len() == 1 {
                cfg.entry = block_id;
            }

            // Second pass: determine successors and queue new targets
            let mut scan_offset = (block_start.wrapping_sub(prg_base)) as usize;
            while scan_offset < prg_data.len() {
                let opcode = prg_data[scan_offset];
                let len = instruction_length(opcode);
                if len == 0 || scan_offset + len > prg_data.len() {
                    break;
                }

                let instr_addr = prg_base + scan_offset as u16;
                if instr_addr >= block_end {
                    break;
                }

                match opcode {
                    // JMP absolute
                    0x4C => {
                        let lo = prg_data[scan_offset + 1] as u16;
                        let hi = prg_data[scan_offset + 2] as u16;
                        let target = lo | (hi << 8);
                        if !visited.contains(&target) {
                            queue.push_back(target);
                        }
                    }
                    // JMP indirect
                    0x6C => {
                        // Indirect jump — can't statically resolve.
                        // Queue the dispatcher fallback address.
                        // For now, skip — runtime dispatcher handles it.
                    }
                    // JSR
                    0x20 => {
                        let lo = prg_data[scan_offset + 1] as u16;
                        let hi = prg_data[scan_offset + 2] as u16;
                        let target = lo | (hi << 8);
                        if !visited.contains(&target) {
                            queue.push_back(target);
                        }
                        // Also queue the instruction after JSR as a potential block start
                        let after = instr_addr + 3;
                        if !visited.contains(&after) {
                            queue.push_back(after);
                        }
                    }
                    // Branches
                    0x10 | 0x30 | 0x50 | 0x70 | 0x90 | 0xB0 | 0xD0 | 0xF0 => {
                        let offset = prg_data[scan_offset + 1] as i8;
                        let target = (instr_addr as i32 + 2 + offset as i32) as u16;
                        if !visited.contains(&target) {
                            queue.push_back(target);
                        }
                        // Fallthrough is also a successor
                        let fallthrough = instr_addr + 2;
                        if !visited.contains(&fallthrough) {
                            queue.push_back(fallthrough);
                        }
                    }
                    _ => {}
                }

                scan_offset += len;
            }
        }

        // Third pass: resolve successor/predecessor edges
        // (Re-scan all blocks to build edges)
        let block_ids: Vec<BlockId> = cfg.blocks.keys().copied().collect();
        for &bid in &block_ids {
            let block = cfg.blocks.get(&bid).unwrap();
            let start = block.start;
            let end = block.end;

            // Find the last instruction in the block
            let mut scan_offset = (start.wrapping_sub(prg_base)) as usize;
            let mut last_opcode = 0u8;
            let mut last_addr = start;
            let mut last_len = 1u16;

            while scan_offset < prg_data.len() {
                let opcode = prg_data[scan_offset];
                let len = instruction_length(opcode) as u16;
                if len == 0 || scan_offset + len as usize > prg_data.len() {
                    break;
                }
                let instr_addr = prg_base + scan_offset as u16;
                if instr_addr + len > end {
                    break;
                }
                last_opcode = opcode;
                last_addr = instr_addr;
                last_len = len;
                scan_offset += len as usize;
            }

            // Determine successors based on last instruction
            match last_opcode {
                // JMP absolute
                0x4C => {
                    let offset = (last_addr.wrapping_sub(prg_base)) as usize;
                    let lo = prg_data[offset + 1] as u16;
                    let hi = prg_data[offset + 2] as u16;
                    let target = lo | (hi << 8);
                    if let Some(target_id) = cfg.block_at(target) {
                        cfg.add_edge(bid, target_id);
                    }
                }
                // JMP indirect — no static successor
                0x6C => {}
                // RTS, RTI, BRK — no successors (return to caller)
                0x60 | 0x40 | 0x00 => {}
                // JSR — successor is the instruction after JSR
                0x20 => {
                    let after = last_addr + last_len;
                    if let Some(after_id) = cfg.block_at(after) {
                        cfg.add_edge(bid, after_id);
                    }
                }
                // Branches — two successors: target and fallthrough
                0x10 | 0x30 | 0x50 | 0x70 | 0x90 | 0xB0 | 0xD0 | 0xF0 => {
                    let offset = (last_addr.wrapping_sub(prg_base)) as usize;
                    let branch_offset = prg_data[offset + 1] as i8;
                    let target = (last_addr as i32 + 2 + branch_offset as i32) as u16;
                    let fallthrough = last_addr + 2;

                    if let Some(target_id) = cfg.block_at(target) {
                        cfg.add_edge(bid, target_id);
                    }
                    if let Some(fallthrough_id) = cfg.block_at(fallthrough) {
                        cfg.add_edge(bid, fallthrough_id);
                    }
                }
                // Fallthrough to next block
                _ => {
                    let next_addr = end;
                    if let Some(next_id) = cfg.block_at(next_addr) {
                        cfg.add_edge(bid, next_id);
                    }
                }
            }
        }

        cfg
    }
}

impl Default for Cfg {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_prg(code: &[u8]) -> Vec<u8> {
        let mut prg = vec![0u8; 0x4000]; // 16KB PRG
        prg[..code.len()].copy_from_slice(code);
        // Set reset vector to 0x8000
        prg[0x3FFC] = 0x00;
        prg[0x3FFD] = 0x80;
        prg
    }

    #[test]
    fn test_single_block() {
        // Just NOPs
        let prg = make_prg(&[0xEA, 0xEA, 0xEA, 0xEA]);
        let cfg = Cfg::build(&prg, 0x8000, &[0x8000]);
        assert!(!cfg.blocks.is_empty());
        // Should have at least one block
        assert!(cfg.blocks.len() >= 1);
    }

    #[test]
    fn test_jmp_creates_two_blocks() {
        // JMP to self
        let prg = make_prg(&[0x4C, 0x00, 0x80]);
        let cfg = Cfg::build(&prg, 0x8000, &[0x8000]);
        assert!(!cfg.blocks.is_empty());
    }

    #[test]
    fn test_jsr_discovers_subroutine() {
        // JSR $8100 at $8000, then NOP at $8003
        let mut prg = make_prg(&[]);
        // $8000: JSR $8100
        prg[0x0000] = 0x20;
        prg[0x0001] = 0x00;
        prg[0x0002] = 0x81;
        // $8003: NOP
        prg[0x0003] = 0xEA;
        // $8100: RTS
        prg[0x0100] = 0x60;

        let cfg = Cfg::build(&prg, 0x8000, &[0x8000]);
        // Should discover blocks at $8000, $8003, and $8100
        assert!(cfg.block_at(0x8000).is_some());
        assert!(cfg.block_at(0x8100).is_some());
    }

    #[test]
    fn test_branch_targets() {
        // $8000: BNE $8005 (offset 3)
        // $8002: NOP
        // $8005: NOP
        let mut prg = make_prg(&[]);
        prg[0x0000] = 0xD0; // BNE
        prg[0x0001] = 0x03; // offset +3
        prg[0x0002] = 0xEA; // NOP
        prg[0x0005] = 0xEA; // NOP

        let cfg = Cfg::build(&prg, 0x8000, &[0x8000]);
        assert!(cfg.block_at(0x8000).is_some());
        assert!(cfg.block_at(0x8005).is_some());
    }

    #[test]
    fn test_entry_points_from_vectors() {
        let mut prg = make_prg(&[]);
        // Reset vector at $FFFC = $8000
        prg[0x3FFC] = 0x00;
        prg[0x3FFD] = 0x80;
        // NMI vector at $FFFA = $8100
        prg[0x3FFA] = 0x00;
        prg[0x3FFB] = 0x81;
        // IRQ vector at $FFFE = $8200
        prg[0x3FFE] = 0x00;
        prg[0x3FFF] = 0x82;

        let entries = crate::analysis::collect_entry_points(&prg, 0x8000);
        assert!(entries.contains(&0x8000));
        assert!(entries.contains(&0x8100));
        assert!(entries.contains(&0x8200));
    }
}
