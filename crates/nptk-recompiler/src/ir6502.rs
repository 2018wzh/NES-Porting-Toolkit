//! IR6502 中间表示

/// IR 标签
pub type Label = u16;

/// 转发条件
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchCondition {
    Eq, Ne, Cs, Cc, Mi, Pl, Vs, Vc,
}

/// 寻址操作数
#[derive(Debug, Clone)]
pub enum Operand {
    Immediate(u8),
    Address(u16),
    Zeropage(u8),
    ZeropageX(u8),
    ZeropageY(u8),
    Absolute(u16),
    AbsoluteX(u16),
    AbsoluteY(u16),
    IndirectX(u8),
    IndirectY(u8),
}

/// IR 指令
#[derive(Debug, Clone)]
pub enum IrOp {
    Nop,
    LoadA(Operand),
    LoadX(Operand),
    LoadY(Operand),
    StoreA(u16),
    StoreX(u16),
    StoreY(u16),
    AddWithCarry(Operand),
    SubWithCarry(Operand),
    And(Operand),
    Or(Operand),
    Xor(Operand),
    Compare(Operand),
    Inc(u16),
    Dec(u16),
    ShiftLeft(u16),
    ShiftRight(u16),
    Branch { condition: BranchCondition, target: Label },
    Jump(Label),
    JumpIndirect(u16),
    Call(Label),
    Return,
    SetFlag { flag: u8, value: bool },
    AdvanceCycles(u8),
}

/// 基本块 IR
#[derive(Debug, Clone)]
pub struct IrBlock {
    pub start: u16,
    pub ops: Vec<IrOp>,
    pub terminator: Option<IrOp>,
}