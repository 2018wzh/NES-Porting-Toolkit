//! IR Builder: Lift 6502 disassembly into IR6502 IR.

use crate::ir6502::{IrOp, IrBlock, Label, Operand, BranchCondition};
use std::collections::HashMap;

/// IR Builder: converts bytes and opcodes into structured IR.
pub struct IrBuilder {
    /// Maps address to label for block entry points
    labels: HashMap<u16, Label>,
    next_label: Label,
}

impl IrBuilder {
    pub fn new() -> Self {
        Self {
            labels: HashMap::new(),
            next_label: 0,
        }
    }

    /// Generate a unique label for a given address
    pub fn get_label(&mut self, addr: u16) -> Label {
        *self.labels.entry(addr).or_insert_with(|| {
            let label = self.next_label;
            self.next_label += 1;
            label
        })
    }

    /// Lift a sequence of 6502 bytes starting at `start_addr`
    /// into a single IR block.
    ///
    /// Returns a list of IR ops that represent the block's instructions.
    /// The process stops when the next instruction would jump out of the
    /// block or when an unhandled opcode is encountered.
    pub fn lift_block(
        bytes: &[u8],
        start_addr: u16,
    ) -> Vec<IrOp> {
        let mut ops = Vec::new();
        let mut offset = 0;

        while offset < bytes.len() {
            let opcode = bytes[offset];
            let instr_addr = start_addr.wrapping_add(offset as u16);
            match Self::lift_opcode(opcode, bytes, offset, instr_addr) {
                Some((ir_ops, consumed)) => {
                    ops.extend(ir_ops);
                    // 每条指令后追加周期推进
                    let cycles = Self::instruction_cycles(opcode);
                    ops.push(IrOp::AdvanceCycles(cycles));
                    offset += consumed;
                }
                None => {
                    // Unknown opcode — emit NOP as placeholder
                    ops.push(IrOp::Nop);
                    ops.push(IrOp::AdvanceCycles(2)); // 未知指令按 2 周期估算
                    offset += 1;
                }
            }
        }

        ops
    }

    /// 返回 6502 指令的 CPU 周期数（不考虑跨页惩罚）
    fn instruction_cycles(opcode: u8) -> u8 {
        match opcode {
            // 1-byte implied/accumulator
            0x00|0x08|0x18|0x28|0x38|0x48|0x58|0x68|0x78|0x88|0x98|0xA8|0xB8|0xC8|0xD8|0xE8|0xF8 => 2,
            0x0A|0x2A|0x4A|0x6A|0x8A|0x9A|0xAA|0xBA|0xCA|0xEA => 2,
            0x40|0x60 => 6, // RTI, RTS
            // 2-byte immediate
            0xA9|0xA2|0xA0|0x69|0xE9|0xC9|0xE0|0xC0|0x29|0x09|0x49 => 2,
            // 2-byte zeropage
            0xA5|0x85|0xA6|0x86|0xA4|0x84|0x65|0xE5|0x25|0x05|0x45|0xC5|0xE4|0xC4|0xE6|0xC6|0x24|0x06|0x46|0x26|0x66 => 3,
            0xB5|0x95|0xB4|0x94|0x75|0xF5|0x35|0x15|0x55|0xD5|0xF6|0xD6|0x16|0x56|0x36|0x76 => 4,
            0xB6|0x96 => 4,
            // indirect
            0xA1|0x81|0x61|0xE1|0x21|0x01|0x41|0xC1 => 6,
            0xB1|0x91|0x71|0xF1|0x31|0x11|0x51|0xD1 => 5,
            // 3-byte absolute
            0xAD|0x8D|0xAE|0x8E|0xAC|0x8C|0x6D|0xED|0x2D|0x0D|0x4D|0xCD|0xEC|0xCC|0xEE|0xCE|0x2C|0x0E|0x4E|0x2E|0x6E => 4,
            0xBD|0x9D|0xBC|0xBE|0x7D|0xFD|0x3D|0x1D|0x5D|0xDD|0xFE|0xDE|0x1E|0x5E|0x3E|0x7E => 4,
            0xB9|0x99|0x79|0xF9|0x39|0x19|0x59|0xD9 => 4,
            // JMP/JSR
            0x4C => 3, 0x6C => 5, 0x20 => 6,
            // branches (not taken)
            0x10|0x30|0x50|0x70|0x90|0xB0|0xD0|0xF0 => 2,
            _ => 2,
        }
    }

    /// Convert a single 6502 opcode and its operands into IR ops.
    /// Returns the list of IR ops and the number of bytes consumed.
    fn lift_opcode(
        opcode: u8,
        bytes: &[u8],
        offset: usize,
        instr_addr: u16,
    ) -> Option<(Vec<IrOp>, usize)> {
        let addr = |bytes: &[u8], off: usize, len: usize| -> u16 {
            let mut result = 0u16;
            for i in 0..len {
                if off + i >= bytes.len() { break; }
                result |= (bytes[off + i] as u16) << (i * 8);
            }
            result
        };
        let imm = |bytes: &[u8], off: usize| -> u8 {
            if off < bytes.len() { bytes[off] } else { 0 }
        };

        match opcode {
            // LDA variants
            0xA9 => {
                let val = imm(bytes, offset + 1);
                Some((vec![IrOp::LoadA(Operand::Immediate(val))], 2))
            }
            0xA5 => {
                let zp = imm(bytes, offset + 1);
                Some((vec![IrOp::LoadA(Operand::Zeropage(zp))], 2))
            }
            0xAD => {
                let abs = addr(bytes, offset + 1, 2);
                Some((vec![IrOp::LoadA(Operand::Absolute(abs))], 3))
            }
            0xB5 => {
                let zp = imm(bytes, offset + 1);
                Some((vec![IrOp::LoadA(Operand::ZeropageX(zp))], 2))
            }
            0xBD => {
                let abs = addr(bytes, offset + 1, 2);
                Some((vec![IrOp::LoadA(Operand::AbsoluteX(abs))], 3))
            }
            0xB9 => {
                let abs = addr(bytes, offset + 1, 2);
                Some((vec![IrOp::LoadA(Operand::AbsoluteY(abs))], 3))
            }
            0xA1 => {
                let zp = imm(bytes, offset + 1);
                Some((vec![IrOp::LoadA(Operand::IndirectX(zp))], 2))
            }
            0xB1 => {
                let zp = imm(bytes, offset + 1);
                Some((vec![IrOp::LoadA(Operand::IndirectY(zp))], 2))
            }

            // STA variants
            0x85 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::StoreA(zp as u16)], 2)) }
            0x95 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::StoreA(zp as u16)], 2)) } // STA zp,X
            0x8D => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::StoreA(abs)], 3)) }
            0x9D => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::StoreA(abs)], 3)) } // STA abs,X
            0x99 => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::StoreA(abs)], 3)) } // STA abs,Y
            0x81 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::StoreA(zp as u16)], 2)) } // STA (indirect,X)
            0x91 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::StoreA(zp as u16)], 2)) } // STA (indirect),Y

            // LDX variants
            0xA2 => {
                let val = imm(bytes, offset + 1);
                Some((vec![IrOp::LoadX(Operand::Immediate(val))], 2))
            }
            0xA6 => {
                let zp = imm(bytes, offset + 1);
                Some((vec![IrOp::LoadX(Operand::Zeropage(zp))], 2))
            }
            0xAE => {
                let abs = addr(bytes, offset + 1, 2);
                Some((vec![IrOp::LoadX(Operand::Absolute(abs))], 3))
            }
            0xB6 => {
                let zp = imm(bytes, offset + 1);
                Some((vec![IrOp::LoadX(Operand::ZeropageY(zp))], 2))
            }
            0xBE => {
                let abs = addr(bytes, offset + 1, 2);
                Some((vec![IrOp::LoadX(Operand::AbsoluteY(abs))], 3))
            }

            // STX variants
            0x86 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::StoreX(zp as u16)], 2)) }
            0x96 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::StoreX(zp as u16)], 2)) } // STX zp,Y
            0x8E => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::StoreX(abs)], 3)) }

            // LDY variants
            0xA0 => { let val = imm(bytes, offset + 1); Some((vec![IrOp::LoadY(Operand::Immediate(val))], 2)) }
            0xA4 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::LoadY(Operand::Zeropage(zp))], 2)) }
            0xAC => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::LoadY(Operand::Absolute(abs))], 3)) }
            0xB4 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::LoadY(Operand::ZeropageX(zp))], 2)) }
            0xBC => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::LoadY(Operand::AbsoluteX(abs))], 3)) }

            // STY variants
            0x84 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::StoreY(zp as u16)], 2)) }
            0x94 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::StoreY(zp as u16)], 2)) } // STY zp,X
            0x8C => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::StoreY(abs)], 3)) }

            // ADC variants (all 8 addressing modes)
            0x69 => { let val = imm(bytes, offset + 1); Some((vec![IrOp::AddWithCarry(Operand::Immediate(val))], 2)) }
            0x65 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::AddWithCarry(Operand::Zeropage(zp))], 2)) }
            0x75 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::AddWithCarry(Operand::ZeropageX(zp))], 2)) }
            0x6D => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::AddWithCarry(Operand::Absolute(abs))], 3)) }
            0x7D => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::AddWithCarry(Operand::AbsoluteX(abs))], 3)) }
            0x79 => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::AddWithCarry(Operand::AbsoluteY(abs))], 3)) }
            0x61 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::AddWithCarry(Operand::IndirectX(zp))], 2)) }
            0x71 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::AddWithCarry(Operand::IndirectY(zp))], 2)) }

            // SBC variants (all 8 addressing modes)
            0xE9 => { let val = imm(bytes, offset + 1); Some((vec![IrOp::SubWithCarry(Operand::Immediate(val))], 2)) }
            0xE5 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::SubWithCarry(Operand::Zeropage(zp))], 2)) }
            0xF5 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::SubWithCarry(Operand::ZeropageX(zp))], 2)) }
            0xED => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::SubWithCarry(Operand::Absolute(abs))], 3)) }
            0xFD => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::SubWithCarry(Operand::AbsoluteX(abs))], 3)) }
            0xF9 => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::SubWithCarry(Operand::AbsoluteY(abs))], 3)) }
            0xE1 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::SubWithCarry(Operand::IndirectX(zp))], 2)) }
            0xF1 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::SubWithCarry(Operand::IndirectY(zp))], 2)) }

            // AND variants (all 8 addressing modes)
            0x29 => { let val = imm(bytes, offset + 1); Some((vec![IrOp::And(Operand::Immediate(val))], 2)) }
            0x25 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::And(Operand::Zeropage(zp))], 2)) }
            0x35 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::And(Operand::ZeropageX(zp))], 2)) }
            0x2D => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::And(Operand::Absolute(abs))], 3)) }
            0x3D => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::And(Operand::AbsoluteX(abs))], 3)) }
            0x39 => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::And(Operand::AbsoluteY(abs))], 3)) }
            0x21 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::And(Operand::IndirectX(zp))], 2)) }
            0x31 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::And(Operand::IndirectY(zp))], 2)) }

            // ORA variants (all 8 addressing modes)
            0x09 => { let val = imm(bytes, offset + 1); Some((vec![IrOp::Or(Operand::Immediate(val))], 2)) }
            0x05 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::Or(Operand::Zeropage(zp))], 2)) }
            0x15 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::Or(Operand::ZeropageX(zp))], 2)) }
            0x0D => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::Or(Operand::Absolute(abs))], 3)) }
            0x1D => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::Or(Operand::AbsoluteX(abs))], 3)) }
            0x19 => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::Or(Operand::AbsoluteY(abs))], 3)) }
            0x01 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::Or(Operand::IndirectX(zp))], 2)) }
            0x11 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::Or(Operand::IndirectY(zp))], 2)) }

            // EOR variants (all 8 addressing modes)
            0x49 => { let val = imm(bytes, offset + 1); Some((vec![IrOp::Xor(Operand::Immediate(val))], 2)) }
            0x45 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::Xor(Operand::Zeropage(zp))], 2)) }
            0x55 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::Xor(Operand::ZeropageX(zp))], 2)) }
            0x4D => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::Xor(Operand::Absolute(abs))], 3)) }
            0x5D => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::Xor(Operand::AbsoluteX(abs))], 3)) }
            0x59 => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::Xor(Operand::AbsoluteY(abs))], 3)) }
            0x41 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::Xor(Operand::IndirectX(zp))], 2)) }
            0x51 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::Xor(Operand::IndirectY(zp))], 2)) }

            // CMP variants (all 8 addressing modes)
            0xC9 => { let val = imm(bytes, offset + 1); Some((vec![IrOp::Compare(Operand::Immediate(val))], 2)) }
            0xC5 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::Compare(Operand::Zeropage(zp))], 2)) }
            0xD5 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::Compare(Operand::ZeropageX(zp))], 2)) }
            0xCD => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::Compare(Operand::Absolute(abs))], 3)) }
            0xDD => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::Compare(Operand::AbsoluteX(abs))], 3)) }
            0xD9 => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::Compare(Operand::AbsoluteY(abs))], 3)) }
            0xC1 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::Compare(Operand::IndirectX(zp))], 2)) }
            0xD1 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::Compare(Operand::IndirectY(zp))], 2)) }

            // CPX variants
            0xE0 => { let val = imm(bytes, offset + 1); Some((vec![IrOp::Compare(Operand::Immediate(val))], 2)) }
            0xE4 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::Compare(Operand::Zeropage(zp))], 2)) }
            0xEC => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::Compare(Operand::Absolute(abs))], 3)) }

            // CPY variants
            0xC0 => { let val = imm(bytes, offset + 1); Some((vec![IrOp::Compare(Operand::Immediate(val))], 2)) }
            0xC4 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::Compare(Operand::Zeropage(zp))], 2)) }
            0xCC => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::Compare(Operand::Absolute(abs))], 3)) }

            // INC variants
            0xE6 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::Inc(zp as u16)], 2)) }
            0xF6 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::Inc(zp as u16)], 2)) } // INC zp,X
            0xEE => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::Inc(abs)], 3)) }
            0xFE => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::Inc(abs)], 3)) } // INC abs,X

            // DEC variants
            0xC6 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::Dec(zp as u16)], 2)) }
            0xD6 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::Dec(zp as u16)], 2)) } // DEC zp,X
            0xCE => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::Dec(abs)], 3)) }
            0xDE => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::Dec(abs)], 3)) } // DEC abs,X

            // INX / INY / DEX / DEY
            0xE8 => Some((vec![IrOp::Inc(0xFFFF)], 1)), // INX — special: inc X register
            0xC8 => Some((vec![IrOp::Inc(0xFFFE)], 1)), // INY — special: inc Y register
            0xCA => Some((vec![IrOp::Dec(0xFFFF)], 1)), // DEX — special: dec X register
            0x88 => Some((vec![IrOp::Dec(0xFFFE)], 1)), // DEY — special: dec Y register

            // Transfers
            0xAA => Some((vec![IrOp::SetFlag { flag: 0xAA, value: false }], 1)), // TAX — special: A→X
            0x8A => Some((vec![IrOp::SetFlag { flag: 0x8A, value: false }], 1)), // TXA — special: X→A
            0xA8 => Some((vec![IrOp::SetFlag { flag: 0xA8, value: false }], 1)), // TAY — special: A→Y
            0x98 => Some((vec![IrOp::SetFlag { flag: 0x98, value: false }], 1)), // TYA — special: Y→A
            0xBA => Some((vec![IrOp::SetFlag { flag: 0xBA, value: false }], 1)), // TSX — special: SP→X
            0x9A => Some((vec![IrOp::SetFlag { flag: 0x9A, value: false }], 1)), // TXS — special: X→SP

            // JMP
            0x4C => {
                let target = addr(bytes, offset + 1, 2);
                Some((vec![IrOp::Jump(target)], 3))
            }
            0x6C => {
                let ptr = addr(bytes, offset + 1, 2);
                Some((vec![IrOp::JumpIndirect(ptr)], 3))
            }

            // JSR
            0x20 => {
                let target = addr(bytes, offset + 1, 2);
                Some((vec![IrOp::Call(target)], 3))
            }

            // RTS
            0x60 => Some((vec![IrOp::Return], 1)),
            // RTI
            0x40 => Some((vec![IrOp::Return], 1)),

            // BRK
            0x00 => Some((vec![IrOp::SetFlag { flag: 0xFF, value: false }], 1)), // BRK — special: software interrupt

            // NOP
            0xEA => Some((vec![IrOp::Nop], 1)),

            // Branches
            0x10 => {
                let offset = bytes.get(offset + 1).copied().unwrap_or(0) as i8;
                Some((vec![IrOp::Branch { condition: BranchCondition::Pl, target: instr_addr.wrapping_add_signed(offset as i16 + 2) }], 2))
            }
            0x30 => {
                let offset = bytes.get(offset + 1).copied().unwrap_or(0) as i8;
                Some((vec![IrOp::Branch { condition: BranchCondition::Mi, target: instr_addr.wrapping_add_signed(offset as i16 + 2) }], 2))
            }
            0x50 => {
                let offset = bytes.get(offset + 1).copied().unwrap_or(0) as i8;
                Some((vec![IrOp::Branch { condition: BranchCondition::Vc, target: instr_addr.wrapping_add_signed(offset as i16 + 2) }], 2))
            }
            0x70 => {
                let offset = bytes.get(offset + 1).copied().unwrap_or(0) as i8;
                Some((vec![IrOp::Branch { condition: BranchCondition::Vs, target: instr_addr.wrapping_add_signed(offset as i16 + 2) }], 2))
            }
            0x90 => {
                let offset = bytes.get(offset + 1).copied().unwrap_or(0) as i8;
                Some((vec![IrOp::Branch { condition: BranchCondition::Cc, target: instr_addr.wrapping_add_signed(offset as i16 + 2) }], 2))
            }
            0xB0 => {
                let offset = bytes.get(offset + 1).copied().unwrap_or(0) as i8;
                Some((vec![IrOp::Branch { condition: BranchCondition::Cs, target: instr_addr.wrapping_add_signed(offset as i16 + 2) }], 2))
            }
            0xD0 => {
                let offset = bytes.get(offset + 1).copied().unwrap_or(0) as i8;
                Some((vec![IrOp::Branch { condition: BranchCondition::Ne, target: instr_addr.wrapping_add_signed(offset as i16 + 2) }], 2))
            }
            0xF0 => {
                let offset = bytes.get(offset + 1).copied().unwrap_or(0) as i8;
                Some((vec![IrOp::Branch { condition: BranchCondition::Eq, target: instr_addr.wrapping_add_signed(offset as i16 + 2) }], 2))
            }

            // Shifts — accumulator
            0x0A => Some((vec![IrOp::ShiftLeft(0xFFFF)], 1)), // ASL A — special: shift A
            0x4A => Some((vec![IrOp::ShiftRight(0xFFFF)], 1)), // LSR A — special: shift A
            0x2A => Some((vec![IrOp::ShiftLeft(0xFFFE)], 1)), // ROL A — special: rotate A left
            0x6A => Some((vec![IrOp::ShiftRight(0xFFFE)], 1)), // ROR A — special: rotate A right
            // Shifts — memory (ASL)
            0x06 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::ShiftLeft(zp as u16)], 2)) }
            0x16 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::ShiftLeft(zp as u16)], 2)) } // ASL zp,X
            0x0E => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::ShiftLeft(abs)], 3)) }
            0x1E => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::ShiftLeft(abs)], 3)) } // ASL abs,X
            // Shifts — memory (LSR)
            0x46 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::ShiftRight(zp as u16)], 2)) }
            0x56 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::ShiftRight(zp as u16)], 2)) } // LSR zp,X
            0x4E => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::ShiftRight(abs)], 3)) }
            0x5E => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::ShiftRight(abs)], 3)) } // LSR abs,X
            // Shifts — memory (ROL)
            0x26 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::ShiftLeft(zp as u16)], 2)) } // ROL zp
            0x36 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::ShiftLeft(zp as u16)], 2)) } // ROL zp,X
            0x2E => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::ShiftLeft(abs)], 3)) } // ROL abs
            0x3E => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::ShiftLeft(abs)], 3)) } // ROL abs,X
            // Shifts — memory (ROR)
            0x66 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::ShiftRight(zp as u16)], 2)) } // ROR zp
            0x76 => { let zp = imm(bytes, offset + 1); Some((vec![IrOp::ShiftRight(zp as u16)], 2)) } // ROR zp,X
            0x6E => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::ShiftRight(abs)], 3)) } // ROR abs
            0x7E => { let abs = addr(bytes, offset + 1, 2); Some((vec![IrOp::ShiftRight(abs)], 3)) } // ROR abs,X

            // Flag control
            0x18 => Some((vec![IrOp::SetFlag { flag: 0, value: false }], 1)), // CLC
            0x38 => Some((vec![IrOp::SetFlag { flag: 0, value: true }], 1)), // SEC
            0x58 => Some((vec![IrOp::SetFlag { flag: 1, value: false }], 1)), // CLI — clear interrupt disable
            0x78 => Some((vec![IrOp::SetFlag { flag: 1, value: true }], 1)), // SEI — set interrupt disable
            0xB8 => Some((vec![IrOp::SetFlag { flag: 2, value: false }], 1)), // CLV — clear overflow
            0xD8 => Some((vec![IrOp::SetFlag { flag: 3, value: false }], 1)), // CLD — clear decimal
            0xF8 => Some((vec![IrOp::SetFlag { flag: 3, value: true }], 1)), // SED — set decimal

            // Stack
            0x48 => Some((vec![IrOp::SetFlag { flag: 0x48, value: false }], 1)), // PHA — special: push A
            0x68 => Some((vec![IrOp::SetFlag { flag: 0x68, value: false }], 1)), // PLA — special: pull A
            0x08 => Some((vec![IrOp::SetFlag { flag: 0x08, value: false }], 1)), // PHP — special: push status
            0x28 => Some((vec![IrOp::SetFlag { flag: 0x28, value: false }], 1)), // PLP — special: pull status

            // BIT
            0x24 => {
                let zp = imm(bytes, offset + 1);
                Some((vec![IrOp::And(Operand::Zeropage(zp))], 2)) // BIT zp — reuse And for test, codegen handles specially
            }
            0x2C => {
                let abs = addr(bytes, offset + 1, 2);
                Some((vec![IrOp::And(Operand::Absolute(abs))], 3)) // BIT abs — reuse And for test
            }

            // Default: unknown opcode
            _ => {
                Some((vec![IrOp::Nop], 1))
            }
        }
    }

    /// Convert a block of IR ops into an IrBlock with proper labeling
    pub fn build_ir_block(
        &mut self,
        start_addr: u16,
        ops: Vec<IrOp>,
    ) -> IrBlock {
        IrBlock {
            start: start_addr,
            ops,
            terminator: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lift_lda_immediate() {
        let bytes = &[0xA9, 0x42];
        let ops = IrBuilder::lift_block(bytes, 0x8000);
        // 每条指令后追加 AdvanceCycles
        assert_eq!(ops.len(), 2);
        assert!(matches!(ops[0], IrOp::LoadA(Operand::Immediate(0x42))));
        assert!(matches!(ops[1], IrOp::AdvanceCycles(2)));
    }

    #[test]
    fn test_lift_jmp_absolute() {
        let bytes = &[0x4C, 0x00, 0x80];
        let ops = IrBuilder::lift_block(bytes, 0x8000);
        assert_eq!(ops.len(), 2);
        assert!(matches!(ops[0], IrOp::Jump(0x8000)));
        assert!(matches!(ops[1], IrOp::AdvanceCycles(3)));
    }

    #[test]
    fn test_lift_rts() {
        let bytes = &[0x60];
        let ops = IrBuilder::lift_block(bytes, 0x8000);
        assert_eq!(ops.len(), 2);
        assert!(matches!(ops[0], IrOp::Return));
        assert!(matches!(ops[1], IrOp::AdvanceCycles(6)));
    }

    #[test]
    fn test_lift_nop() {
        let bytes = &[0xEA];
        let ops = IrBuilder::lift_block(bytes, 0x8000);
        assert_eq!(ops.len(), 2);
        assert!(matches!(ops[0], IrOp::Nop));
        assert!(matches!(ops[1], IrOp::AdvanceCycles(2)));
    }

    #[test]
    fn test_lift_branch() {
        let bytes = &[0xD0, 0x02]; // BNE +2 at $8000 → target = $8000 + 2 + 2 = $8004
        let ops = IrBuilder::lift_block(bytes, 0x8000);
        assert_eq!(ops.len(), 2);
        assert!(matches!(ops[0], IrOp::Branch { condition: BranchCondition::Ne, target: 0x8004 }));
        assert!(matches!(ops[1], IrOp::AdvanceCycles(2)));
    }
}
