//! mos6502 CPU 封装
//!
//! 本模块将 `mos6502` crate 的 CPU 封装为与旧代码兼容的接口。
//! `Cpu6502` 是 `mos6502::cpu::CPU<NesBusImpl, Ricoh2a03>` 的类型别名。
//!
//! 保留 `CpuFlags` 作为兼容包装，提供 `from_byte`/`to_byte`/`set_zn` 方法。

use crate::bus::NesBusImpl;
use mos6502::cpu;
use mos6502::instruction::Ricoh2a03;
use mos6502::registers::Status;

/// 重新导出 mos6502 类型，供下游 crate 使用
pub use mos6502::registers::Status as MosStatus;
pub use mos6502::instruction::Ricoh2a03 as MosRicoh2a03;

/// mos6502 CPU 类型别名 — 使用 NesBusImpl 作为内存总线，Ricoh2a03 作为 CPU 变体
pub type Cpu6502 = cpu::CPU<NesBusImpl, Ricoh2a03>;

/// CPU 标志兼容包装 — 桥接到 `mos6502::registers::Status` bitflags
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CpuFlags {
    pub carry: bool,
    pub zero: bool,
    pub interrupt_disable: bool,
    pub decimal: bool,
    pub break_flag: bool,
    pub overflow: bool,
    pub negative: bool,
}

impl CpuFlags {
    pub fn from_byte(b: u8) -> Self {
        let status = Status::from_bits_truncate(b);
        Self {
            carry: status.contains(Status::PS_CARRY),
            zero: status.contains(Status::PS_ZERO),
            interrupt_disable: status.contains(Status::PS_DISABLE_INTERRUPTS),
            decimal: status.contains(Status::PS_DECIMAL_MODE),
            break_flag: status.contains(Status::PS_BRK),
            overflow: status.contains(Status::PS_OVERFLOW),
            negative: status.contains(Status::PS_NEGATIVE),
        }
    }

    pub fn to_byte(self) -> u8 {
        let mut status = Status::empty();
        status.set(Status::PS_CARRY, self.carry);
        status.set(Status::PS_ZERO, self.zero);
        status.set(Status::PS_DISABLE_INTERRUPTS, self.interrupt_disable);
        status.set(Status::PS_DECIMAL_MODE, self.decimal);
        status.set(Status::PS_BRK, self.break_flag);
        status.set(Status::PS_OVERFLOW, self.overflow);
        status.set(Status::PS_NEGATIVE, self.negative);
        status.bits()
    }

    pub fn set_zn(&mut self, v: u8) {
        self.zero = v == 0;
        self.negative = v & 0x80 != 0;
    }
}

/// 将 `CpuFlags` 转换为 `mos6502::registers::Status`
impl From<CpuFlags> for Status {
    fn from(f: CpuFlags) -> Self {
        let mut status = Status::empty();
        status.set(Status::PS_CARRY, f.carry);
        status.set(Status::PS_ZERO, f.zero);
        status.set(Status::PS_DISABLE_INTERRUPTS, f.interrupt_disable);
        status.set(Status::PS_DECIMAL_MODE, f.decimal);
        status.set(Status::PS_BRK, f.break_flag);
        status.set(Status::PS_OVERFLOW, f.overflow);
        status.set(Status::PS_NEGATIVE, f.negative);
        status
    }
}

/// 将 `mos6502::registers::Status` 转换为 `CpuFlags`
impl From<Status> for CpuFlags {
    fn from(s: Status) -> Self {
        Self {
            carry: s.contains(Status::PS_CARRY),
            zero: s.contains(Status::PS_ZERO),
            interrupt_disable: s.contains(Status::PS_DISABLE_INTERRUPTS),
            decimal: s.contains(Status::PS_DECIMAL_MODE),
            break_flag: s.contains(Status::PS_BRK),
            overflow: s.contains(Status::PS_OVERFLOW),
            negative: s.contains(Status::PS_NEGATIVE),
        }
    }
}
