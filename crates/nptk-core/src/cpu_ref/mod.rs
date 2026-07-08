// Rust 6502 CPU implementation
// Based on: NESDev Wiki 6502 reference

use crate::bus::NesBus;

#[derive(Debug, Clone, Copy, Default)]
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
        Self {
            carry: b & 0x01 != 0,
            zero: b & 0x02 != 0,
            interrupt_disable: b & 0x04 != 0,
            decimal: b & 0x08 != 0,
            break_flag: b & 0x10 != 0,
            overflow: b & 0x40 != 0,
            negative: b & 0x80 != 0,
        }
    }
    pub fn to_byte(self) -> u8 {
        self.carry as u8
            | (self.zero as u8) << 1
            | (self.interrupt_disable as u8) << 2
            | (self.decimal as u8) << 3
            | (self.break_flag as u8) << 4
            | 0x20
            | (self.overflow as u8) << 6
            | (self.negative as u8) << 7
    }
    pub fn set_zn(&mut self, v: u8) {
        self.zero = v == 0;
        self.negative = v & 0x80 != 0;
    }
}

#[derive(Debug, Clone)]
pub struct Cpu6502 {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub sp: u8,
    pub pc: u16,
    pub status: CpuFlags,
    pub cycles: u64,
    pub nmi_pending: bool,
}

impl Cpu6502 {
    pub fn new() -> Self {
        Self {
            a: 0,
            x: 0,
            y: 0,
            sp: 0xFD,
            pc: 0,
            status: CpuFlags::default(),
            cycles: 0,
            nmi_pending: false,
        }
    }

    pub fn reset(&mut self, bus: &mut dyn NesBus) {
        let lo = bus.cpu_read(0xFFFC) as u16;
        let hi = bus.cpu_read(0xFFFD) as u16;
        self.pc = lo | (hi << 8);
        self.sp = self.sp.wrapping_sub(3);
        self.status.interrupt_disable = true;
        self.cycles = self.cycles.wrapping_add(7);
    }

    pub fn trigger_nmi(&mut self, bus: &mut dyn NesBus) {
        bus.cpu_write(0x0100 | self.sp as u16, (self.pc >> 8) as u8);
        self.sp = self.sp.wrapping_sub(1);
        bus.cpu_write(0x0100 | self.sp as u16, self.pc as u8);
        self.sp = self.sp.wrapping_sub(1);
        bus.cpu_write(0x0100 | self.sp as u16, self.status.to_byte());
        self.sp = self.sp.wrapping_sub(1);
        self.status.interrupt_disable = true;
        let lo = bus.cpu_read(0xFFFA) as u16;
        let hi = bus.cpu_read(0xFFFB) as u16;
        self.pc = lo | (hi << 8);
        self.cycles = self.cycles.wrapping_add(7);
    }

    pub fn step(&mut self, bus: &mut dyn NesBus) -> u32 {
        let opcode = bus.cpu_read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        self.exec(opcode, bus)
    }

    #[allow(unused_assignments)]
    fn exec(&mut self, op: u8, bus: &mut dyn NesBus) -> u32 {
        let mut pc = self.pc;
        let mut sp = self.sp;

        macro_rules! imm {
            () => {{
                let v = bus.cpu_read(pc);
                pc = pc.wrapping_add(1);
                v
            }};
        }
        macro_rules! abs {
            () => {{
                let lo = bus.cpu_read(pc) as u16;
                let hi = bus.cpu_read(pc.wrapping_add(1)) as u16;
                pc = pc.wrapping_add(2);
                lo | (hi << 8)
            }};
        }
        macro_rules! abx {
            () => {{
                let b = abs!();
                b.wrapping_add(self.x as u16)
            }};
        }
        macro_rules! aby {
            () => {{
                let b = abs!();
                b.wrapping_add(self.y as u16)
            }};
        }
        macro_rules! zp {
            () => {{
                let a = bus.cpu_read(pc) as u16;
                pc = pc.wrapping_add(1);
                a
            }};
        }
        macro_rules! zpx {
            () => {{
                let a = bus.cpu_read(pc).wrapping_add(self.x) as u16;
                pc = pc.wrapping_add(1);
                a
            }};
        }
        macro_rules! zpy {
            () => {{
                let a = bus.cpu_read(pc).wrapping_add(self.y) as u16;
                pc = pc.wrapping_add(1);
                a
            }};
        }
        macro_rules! izx {
            () => {{
                let z = bus.cpu_read(pc).wrapping_add(self.x);
                pc = pc.wrapping_add(1);
                let lo = bus.cpu_read(z as u16) as u16;
                let hi = bus.cpu_read(z.wrapping_add(1) as u16) as u16;
                lo | (hi << 8)
            }};
        }
        macro_rules! izy {
            () => {{
                let z = bus.cpu_read(pc);
                pc = pc.wrapping_add(1);
                let lo = bus.cpu_read(z as u16) as u16;
                let hi = bus.cpu_read(z.wrapping_add(1) as u16) as u16;
                (lo | (hi << 8)).wrapping_add(self.y as u16)
            }};
        }
        macro_rules! ind {
            () => {{
                let lo = bus.cpu_read(pc) as u16;
                let hi = bus.cpu_read(pc.wrapping_add(1)) as u16;
                pc = pc.wrapping_add(2);
                let ptr = lo | (hi << 8);
                let alo = bus.cpu_read(ptr) as u16;
                let ahi = bus.cpu_read(ptr.wrapping_add(1)) as u16;
                alo | (ahi << 8)
            }};
        }

        let result = match op {
            0x00 => {
                pc = pc.wrapping_add(1);
                bus.cpu_write(0x0100 | sp as u16, (pc >> 8) as u8);
                sp = sp.wrapping_sub(1);
                bus.cpu_write(0x0100 | sp as u16, pc as u8);
                sp = sp.wrapping_sub(1);
                bus.cpu_write(0x0100 | sp as u16, self.status.to_byte() | 0x10);
                sp = sp.wrapping_sub(1);
                self.status.interrupt_disable = true;
                let lo = bus.cpu_read(0xFFFE) as u16;
                let hi = bus.cpu_read(0xFFFF) as u16;
                self.pc = lo | (hi << 8);
                7
            }
            0x01 => {
                let a = izx!();
                self.a |= bus.cpu_read(a);
                self.status.set_zn(self.a);
                6
            }
            0x05 => {
                let a = zp!();
                self.a |= bus.cpu_read(a);
                self.status.set_zn(self.a);
                3
            }
            0x06 => {
                let a = zp!();
                let v = bus.cpu_read(a);
                bus.cpu_write(a, v << 1);
                self.status.carry = v & 0x80 != 0;
                self.status.set_zn(v << 1);
                5
            }
            0x08 => {
                bus.cpu_write(0x0100 | sp as u16, self.status.to_byte() | 0x10);
                sp = sp.wrapping_sub(1);
                3
            }
            0x09 => {
                self.a |= imm!();
                self.status.set_zn(self.a);
                2
            }
            0x0A => {
                self.status.carry = self.a & 0x80 != 0;
                self.a <<= 1;
                self.status.set_zn(self.a);
                2
            }
            0x0D => {
                let a = abs!();
                self.a |= bus.cpu_read(a);
                self.status.set_zn(self.a);
                4
            }
            0x0E => {
                let a = abs!();
                let v = bus.cpu_read(a);
                bus.cpu_write(a, v << 1);
                self.status.carry = v & 0x80 != 0;
                self.status.set_zn(v << 1);
                6
            }

            // LDA
            0xA9 => {
                self.a = imm!();
                self.status.set_zn(self.a);
                2
            }
            0xA5 => {
                let a = zp!();
                self.a = bus.cpu_read(a);
                self.status.set_zn(self.a);
                3
            }
            0xB5 => {
                let a = zpx!();
                self.a = bus.cpu_read(a);
                self.status.set_zn(self.a);
                4
            }
            0xAD => {
                let a = abs!();
                self.a = bus.cpu_read(a);
                self.status.set_zn(self.a);
                4
            }
            0xBD => {
                let a = abx!();
                self.a = bus.cpu_read(a);
                self.status.set_zn(self.a);
                4
            }
            0xB9 => {
                let a = aby!();
                self.a = bus.cpu_read(a);
                self.status.set_zn(self.a);
                4
            }
            0xA1 => {
                let a = izx!();
                self.a = bus.cpu_read(a);
                self.status.set_zn(self.a);
                6
            }
            0xB1 => {
                let a = izy!();
                self.a = bus.cpu_read(a);
                self.status.set_zn(self.a);
                5
            }

            // STA
            0x85 => {
                let a = zp!();
                bus.cpu_write(a, self.a);
                3
            }
            0x95 => {
                let a = zpx!();
                bus.cpu_write(a, self.a);
                4
            }
            0x8D => {
                let a = abs!();
                bus.cpu_write(a, self.a);
                4
            }
            0x9D => {
                let a = abx!();
                bus.cpu_write(a, self.a);
                5
            }
            0x99 => {
                let a = aby!();
                bus.cpu_write(a, self.a);
                5
            }
            0x81 => {
                let a = izx!();
                bus.cpu_write(a, self.a);
                6
            }
            0x91 => {
                let a = izy!();
                bus.cpu_write(a, self.a);
                6
            }

            // LDX
            0xA2 => {
                self.x = imm!();
                self.status.set_zn(self.x);
                2
            }
            0xA6 => {
                let a = zp!();
                self.x = bus.cpu_read(a);
                self.status.set_zn(self.x);
                3
            }
            0xB6 => {
                let a = zpy!();
                self.x = bus.cpu_read(a);
                self.status.set_zn(self.x);
                4
            }
            0xAE => {
                let a = abs!();
                self.x = bus.cpu_read(a);
                self.status.set_zn(self.x);
                4
            }
            0xBE => {
                let a = aby!();
                self.x = bus.cpu_read(a);
                self.status.set_zn(self.x);
                4
            }

            // STX
            0x86 => {
                let a = zp!();
                bus.cpu_write(a, self.x);
                3
            }
            0x96 => {
                let a = zpy!();
                bus.cpu_write(a, self.x);
                4
            }
            0x8E => {
                let a = abs!();
                bus.cpu_write(a, self.x);
                4
            }

            // LDY
            0xA0 => {
                self.y = imm!();
                self.status.set_zn(self.y);
                2
            }
            0xA4 => {
                let a = zp!();
                self.y = bus.cpu_read(a);
                self.status.set_zn(self.y);
                3
            }
            0xB4 => {
                let a = zpx!();
                self.y = bus.cpu_read(a);
                self.status.set_zn(self.y);
                4
            }
            0xAC => {
                let a = abs!();
                self.y = bus.cpu_read(a);
                self.status.set_zn(self.y);
                4
            }
            0xBC => {
                let a = abx!();
                self.y = bus.cpu_read(a);
                self.status.set_zn(self.y);
                4
            }

            // STY
            0x84 => {
                let a = zp!();
                bus.cpu_write(a, self.y);
                3
            }
            0x94 => {
                let a = zpx!();
                bus.cpu_write(a, self.y);
                4
            }
            0x8C => {
                let a = abs!();
                bus.cpu_write(a, self.y);
                4
            }

            // Transfers
            0xAA => {
                self.x = self.a;
                self.status.set_zn(self.x);
                2
            }
            0x8A => {
                self.a = self.x;
                self.status.set_zn(self.a);
                2
            }
            0xA8 => {
                self.y = self.a;
                self.status.set_zn(self.y);
                2
            }
            0x98 => {
                self.a = self.y;
                self.status.set_zn(self.a);
                2
            }
            0xBA => {
                self.x = sp;
                self.status.set_zn(self.x);
                2
            }
            0x9A => {
                sp = self.x;
                2
            }

            // Stack
            0x48 => {
                bus.cpu_write(0x0100 | sp as u16, self.a);
                sp = sp.wrapping_sub(1);
                3
            }
            0x68 => {
                sp = sp.wrapping_add(1);
                self.a = bus.cpu_read(0x0100 | sp as u16);
                self.status.set_zn(self.a);
                4
            }
            0x28 => {
                sp = sp.wrapping_add(1);
                self.status = CpuFlags::from_byte(bus.cpu_read(0x0100 | sp as u16));
                4
            }

            // ADC
            0x69 => {
                let v = imm!();
                let r = self.a as u16 + v as u16 + self.status.carry as u16;
                self.status.carry = r > 0xFF;
                let r8 = r as u8;
                self.status.overflow = ((self.a ^ r8) & (v ^ r8) & 0x80) != 0;
                self.a = r8;
                self.status.set_zn(self.a);
                2
            }
            0x65 => {
                let a = zp!();
                let v = bus.cpu_read(a);
                let r = self.a as u16 + v as u16 + self.status.carry as u16;
                self.status.carry = r > 0xFF;
                let r8 = r as u8;
                self.status.overflow = ((self.a ^ r8) & (v ^ r8) & 0x80) != 0;
                self.a = r8;
                self.status.set_zn(self.a);
                3
            }
            0x75 => {
                let a = zpx!();
                let v = bus.cpu_read(a);
                let r = self.a as u16 + v as u16 + self.status.carry as u16;
                self.status.carry = r > 0xFF;
                let r8 = r as u8;
                self.status.overflow = ((self.a ^ r8) & (v ^ r8) & 0x80) != 0;
                self.a = r8;
                self.status.set_zn(self.a);
                4
            }
            0x6D => {
                let a = abs!();
                let v = bus.cpu_read(a);
                let r = self.a as u16 + v as u16 + self.status.carry as u16;
                self.status.carry = r > 0xFF;
                let r8 = r as u8;
                self.status.overflow = ((self.a ^ r8) & (v ^ r8) & 0x80) != 0;
                self.a = r8;
                self.status.set_zn(self.a);
                4
            }
            0x7D => {
                let a = abx!();
                let v = bus.cpu_read(a);
                let r = self.a as u16 + v as u16 + self.status.carry as u16;
                self.status.carry = r > 0xFF;
                let r8 = r as u8;
                self.status.overflow = ((self.a ^ r8) & (v ^ r8) & 0x80) != 0;
                self.a = r8;
                self.status.set_zn(self.a);
                4
            }
            0x79 => {
                let a = aby!();
                let v = bus.cpu_read(a);
                let r = self.a as u16 + v as u16 + self.status.carry as u16;
                self.status.carry = r > 0xFF;
                let r8 = r as u8;
                self.status.overflow = ((self.a ^ r8) & (v ^ r8) & 0x80) != 0;
                self.a = r8;
                self.status.set_zn(self.a);
                4
            }
            0x61 => {
                let a = izx!();
                let v = bus.cpu_read(a);
                let r = self.a as u16 + v as u16 + self.status.carry as u16;
                self.status.carry = r > 0xFF;
                let r8 = r as u8;
                self.status.overflow = ((self.a ^ r8) & (v ^ r8) & 0x80) != 0;
                self.a = r8;
                self.status.set_zn(self.a);
                6
            }
            0x71 => {
                let a = izy!();
                let v = bus.cpu_read(a);
                let r = self.a as u16 + v as u16 + self.status.carry as u16;
                self.status.carry = r > 0xFF;
                let r8 = r as u8;
                self.status.overflow = ((self.a ^ r8) & (v ^ r8) & 0x80) != 0;
                self.a = r8;
                self.status.set_zn(self.a);
                5
            }

            // SBC
            0xE9 => {
                let v = imm!();
                let r = self.a as i16 - v as i16 - (1 - self.status.carry as i16);
                self.status.carry = r >= 0;
                let r8 = r as u8;
                self.status.overflow = ((self.a as i16 ^ r) & ((-(v as i16)) ^ r) & 0x80) != 0;
                self.a = r8;
                self.status.set_zn(self.a);
                2
            }
            0xE5 => {
                let a = zp!();
                let v = bus.cpu_read(a);
                let r = self.a as i16 - v as i16 - (1 - self.status.carry as i16);
                self.status.carry = r >= 0;
                let r8 = r as u8;
                self.status.overflow = ((self.a as i16 ^ r) & ((-(v as i16)) ^ r) & 0x80) != 0;
                self.a = r8;
                self.status.set_zn(self.a);
                3
            }
            0xF5 => {
                let a = zpx!();
                let v = bus.cpu_read(a);
                let r = self.a as i16 - v as i16 - (1 - self.status.carry as i16);
                self.status.carry = r >= 0;
                let r8 = r as u8;
                self.status.overflow = ((self.a as i16 ^ r) & ((-(v as i16)) ^ r) & 0x80) != 0;
                self.a = r8;
                self.status.set_zn(self.a);
                4
            }
            0xED => {
                let a = abs!();
                let v = bus.cpu_read(a);
                let r = self.a as i16 - v as i16 - (1 - self.status.carry as i16);
                self.status.carry = r >= 0;
                let r8 = r as u8;
                self.status.overflow = ((self.a as i16 ^ r) & ((-(v as i16)) ^ r) & 0x80) != 0;
                self.a = r8;
                self.status.set_zn(self.a);
                4
            }
            0xFD => {
                let a = abx!();
                let v = bus.cpu_read(a);
                let r = self.a as i16 - v as i16 - (1 - self.status.carry as i16);
                self.status.carry = r >= 0;
                let r8 = r as u8;
                self.status.overflow = ((self.a as i16 ^ r) & ((-(v as i16)) ^ r) & 0x80) != 0;
                self.a = r8;
                self.status.set_zn(self.a);
                4
            }
            0xF9 => {
                let a = aby!();
                let v = bus.cpu_read(a);
                let r = self.a as i16 - v as i16 - (1 - self.status.carry as i16);
                self.status.carry = r >= 0;
                let r8 = r as u8;
                self.status.overflow = ((self.a as i16 ^ r) & ((-(v as i16)) ^ r) & 0x80) != 0;
                self.a = r8;
                self.status.set_zn(self.a);
                4
            }
            0xE1 => {
                let a = izx!();
                let v = bus.cpu_read(a);
                let r = self.a as i16 - v as i16 - (1 - self.status.carry as i16);
                self.status.carry = r >= 0;
                let r8 = r as u8;
                self.status.overflow = ((self.a as i16 ^ r) & ((-(v as i16)) ^ r) & 0x80) != 0;
                self.a = r8;
                self.status.set_zn(self.a);
                6
            }
            0xF1 => {
                let a = izy!();
                let v = bus.cpu_read(a);
                let r = self.a as i16 - v as i16 - (1 - self.status.carry as i16);
                self.status.carry = r >= 0;
                let r8 = r as u8;
                self.status.overflow = ((self.a as i16 ^ r) & ((-(v as i16)) ^ r) & 0x80) != 0;
                self.a = r8;
                self.status.set_zn(self.a);
                5
            }

            // AND
            0x29 => {
                self.a &= imm!();
                self.status.set_zn(self.a);
                2
            }
            0x25 => {
                let a = zp!();
                self.a &= bus.cpu_read(a);
                self.status.set_zn(self.a);
                3
            }
            0x35 => {
                let a = zpx!();
                self.a &= bus.cpu_read(a);
                self.status.set_zn(self.a);
                4
            }
            0x2D => {
                let a = abs!();
                self.a &= bus.cpu_read(a);
                self.status.set_zn(self.a);
                4
            }
            0x3D => {
                let a = abx!();
                self.a &= bus.cpu_read(a);
                self.status.set_zn(self.a);
                4
            }
            0x39 => {
                let a = aby!();
                self.a &= bus.cpu_read(a);
                self.status.set_zn(self.a);
                4
            }
            0x21 => {
                let a = izx!();
                self.a &= bus.cpu_read(a);
                self.status.set_zn(self.a);
                6
            }
            0x31 => {
                let a = izy!();
                self.a &= bus.cpu_read(a);
                self.status.set_zn(self.a);
                5
            }

            // EOR
            0x49 => {
                self.a ^= imm!();
                self.status.set_zn(self.a);
                2
            }
            0x45 => {
                let a = zp!();
                self.a ^= bus.cpu_read(a);
                self.status.set_zn(self.a);
                3
            }
            0x55 => {
                let a = zpx!();
                self.a ^= bus.cpu_read(a);
                self.status.set_zn(self.a);
                4
            }
            0x4D => {
                let a = abs!();
                self.a ^= bus.cpu_read(a);
                self.status.set_zn(self.a);
                4
            }
            0x5D => {
                let a = abx!();
                self.a ^= bus.cpu_read(a);
                self.status.set_zn(self.a);
                4
            }
            0x59 => {
                let a = aby!();
                self.a ^= bus.cpu_read(a);
                self.status.set_zn(self.a);
                4
            }
            0x41 => {
                let a = izx!();
                self.a ^= bus.cpu_read(a);
                self.status.set_zn(self.a);
                6
            }
            0x51 => {
                let a = izy!();
                self.a ^= bus.cpu_read(a);
                self.status.set_zn(self.a);
                5
            }

            // CMP
            0xC9 => {
                let v = imm!();
                self.status.carry = self.a >= v;
                self.status.set_zn(self.a.wrapping_sub(v));
                2
            }
            0xC5 => {
                let a = zp!();
                let v = bus.cpu_read(a);
                self.status.carry = self.a >= v;
                self.status.set_zn(self.a.wrapping_sub(v));
                3
            }
            0xD5 => {
                let a = zpx!();
                let v = bus.cpu_read(a);
                self.status.carry = self.a >= v;
                self.status.set_zn(self.a.wrapping_sub(v));
                4
            }
            0xCD => {
                let a = abs!();
                let v = bus.cpu_read(a);
                self.status.carry = self.a >= v;
                self.status.set_zn(self.a.wrapping_sub(v));
                4
            }
            0xDD => {
                let a = abx!();
                let v = bus.cpu_read(a);
                self.status.carry = self.a >= v;
                self.status.set_zn(self.a.wrapping_sub(v));
                4
            }
            0xD9 => {
                let a = aby!();
                let v = bus.cpu_read(a);
                self.status.carry = self.a >= v;
                self.status.set_zn(self.a.wrapping_sub(v));
                4
            }
            0xC1 => {
                let a = izx!();
                let v = bus.cpu_read(a);
                self.status.carry = self.a >= v;
                self.status.set_zn(self.a.wrapping_sub(v));
                6
            }
            0xD1 => {
                let a = izy!();
                let v = bus.cpu_read(a);
                self.status.carry = self.a >= v;
                self.status.set_zn(self.a.wrapping_sub(v));
                5
            }

            // CPX
            0xE0 => {
                let v = imm!();
                self.status.carry = self.x >= v;
                self.status.set_zn(self.x.wrapping_sub(v));
                2
            }
            0xE4 => {
                let a = zp!();
                let v = bus.cpu_read(a);
                self.status.carry = self.x >= v;
                self.status.set_zn(self.x.wrapping_sub(v));
                3
            }
            0xEC => {
                let a = abs!();
                let v = bus.cpu_read(a);
                self.status.carry = self.x >= v;
                self.status.set_zn(self.x.wrapping_sub(v));
                4
            }

            // CPY
            0xC0 => {
                let v = imm!();
                self.status.carry = self.y >= v;
                self.status.set_zn(self.y.wrapping_sub(v));
                2
            }
            0xC4 => {
                let a = zp!();
                let v = bus.cpu_read(a);
                self.status.carry = self.y >= v;
                self.status.set_zn(self.y.wrapping_sub(v));
                3
            }
            0xCC => {
                let a = abs!();
                let v = bus.cpu_read(a);
                self.status.carry = self.y >= v;
                self.status.set_zn(self.y.wrapping_sub(v));
                4
            }

            // INC
            0xE6 => {
                let a = zp!();
                let v = bus.cpu_read(a).wrapping_add(1);
                bus.cpu_write(a, v);
                self.status.set_zn(v);
                5
            }
            0xF6 => {
                let a = zpx!();
                let v = bus.cpu_read(a).wrapping_add(1);
                bus.cpu_write(a, v);
                self.status.set_zn(v);
                6
            }
            0xEE => {
                let a = abs!();
                let v = bus.cpu_read(a).wrapping_add(1);
                bus.cpu_write(a, v);
                self.status.set_zn(v);
                6
            }
            0xFE => {
                let a = abx!();
                let v = bus.cpu_read(a).wrapping_add(1);
                bus.cpu_write(a, v);
                self.status.set_zn(v);
                7
            }

            // DEC
            0xC6 => {
                let a = zp!();
                let v = bus.cpu_read(a).wrapping_sub(1);
                bus.cpu_write(a, v);
                self.status.set_zn(v);
                5
            }
            0xD6 => {
                let a = zpx!();
                let v = bus.cpu_read(a).wrapping_sub(1);
                bus.cpu_write(a, v);
                self.status.set_zn(v);
                6
            }
            0xCE => {
                let a = abs!();
                let v = bus.cpu_read(a).wrapping_sub(1);
                bus.cpu_write(a, v);
                self.status.set_zn(v);
                6
            }
            0xDE => {
                let a = abx!();
                let v = bus.cpu_read(a).wrapping_sub(1);
                bus.cpu_write(a, v);
                self.status.set_zn(v);
                7
            }

            // INX/INY/DEX/DEY
            0xE8 => {
                self.x = self.x.wrapping_add(1);
                self.status.set_zn(self.x);
                2
            }
            0xC8 => {
                self.y = self.y.wrapping_add(1);
                self.status.set_zn(self.y);
                2
            }
            0xCA => {
                self.x = self.x.wrapping_sub(1);
                self.status.set_zn(self.x);
                2
            }
            0x88 => {
                self.y = self.y.wrapping_sub(1);
                self.status.set_zn(self.y);
                2
            }

            // LSR
            0x4A => {
                self.status.carry = self.a & 1 != 0;
                self.a >>= 1;
                self.status.set_zn(self.a);
                2
            }
            0x46 => {
                let a = zp!();
                let v = bus.cpu_read(a);
                bus.cpu_write(a, v >> 1);
                self.status.carry = v & 1 != 0;
                self.status.set_zn(v >> 1);
                5
            }
            0x56 => {
                let a = zpx!();
                let v = bus.cpu_read(a);
                bus.cpu_write(a, v >> 1);
                self.status.carry = v & 1 != 0;
                self.status.set_zn(v >> 1);
                6
            }
            0x4E => {
                let a = abs!();
                let v = bus.cpu_read(a);
                bus.cpu_write(a, v >> 1);
                self.status.carry = v & 1 != 0;
                self.status.set_zn(v >> 1);
                6
            }
            0x5E => {
                let a = abx!();
                let v = bus.cpu_read(a);
                bus.cpu_write(a, v >> 1);
                self.status.carry = v & 1 != 0;
                self.status.set_zn(v >> 1);
                7
            }

            // ROL
            0x2A => {
                let c = self.status.carry as u8;
                self.status.carry = self.a & 0x80 != 0;
                self.a = (self.a << 1) | c;
                self.status.set_zn(self.a);
                2
            }
            0x26 => {
                let a = zp!();
                let v = bus.cpu_read(a);
                let c = self.status.carry as u8;
                self.status.carry = v & 0x80 != 0;
                let r = (v << 1) | c;
                bus.cpu_write(a, r);
                self.status.set_zn(r);
                5
            }
            0x36 => {
                let a = zpx!();
                let v = bus.cpu_read(a);
                let c = self.status.carry as u8;
                self.status.carry = v & 0x80 != 0;
                let r = (v << 1) | c;
                bus.cpu_write(a, r);
                self.status.set_zn(r);
                6
            }
            0x2E => {
                let a = abs!();
                let v = bus.cpu_read(a);
                let c = self.status.carry as u8;
                self.status.carry = v & 0x80 != 0;
                let r = (v << 1) | c;
                bus.cpu_write(a, r);
                self.status.set_zn(r);
                6
            }
            0x3E => {
                let a = abx!();
                let v = bus.cpu_read(a);
                let c = self.status.carry as u8;
                self.status.carry = v & 0x80 != 0;
                let r = (v << 1) | c;
                bus.cpu_write(a, r);
                self.status.set_zn(r);
                7
            }

            // ROR
            0x6A => {
                let c = (self.status.carry as u8) << 7;
                self.status.carry = self.a & 1 != 0;
                self.a = (self.a >> 1) | c;
                self.status.set_zn(self.a);
                2
            }
            0x66 => {
                let a = zp!();
                let v = bus.cpu_read(a);
                let c = (self.status.carry as u8) << 7;
                self.status.carry = v & 1 != 0;
                let r = (v >> 1) | c;
                bus.cpu_write(a, r);
                self.status.set_zn(r);
                5
            }
            0x76 => {
                let a = zpx!();
                let v = bus.cpu_read(a);
                let c = (self.status.carry as u8) << 7;
                self.status.carry = v & 1 != 0;
                let r = (v >> 1) | c;
                bus.cpu_write(a, r);
                self.status.set_zn(r);
                6
            }
            0x6E => {
                let a = abs!();
                let v = bus.cpu_read(a);
                let c = (self.status.carry as u8) << 7;
                self.status.carry = v & 1 != 0;
                let r = (v >> 1) | c;
                bus.cpu_write(a, r);
                self.status.set_zn(r);
                6
            }
            0x7E => {
                let a = abx!();
                let v = bus.cpu_read(a);
                let c = (self.status.carry as u8) << 7;
                self.status.carry = v & 1 != 0;
                let r = (v >> 1) | c;
                bus.cpu_write(a, r);
                self.status.set_zn(r);
                7
            }

            // BIT
            0x24 => {
                let a = zp!();
                let v = bus.cpu_read(a);
                self.status.zero = self.a & v == 0;
                self.status.overflow = v & 0x40 != 0;
                self.status.negative = v & 0x80 != 0;
                3
            }
            0x2C => {
                let a = abs!();
                let v = bus.cpu_read(a);
                self.status.zero = self.a & v == 0;
                self.status.overflow = v & 0x40 != 0;
                self.status.negative = v & 0x80 != 0;
                4
            }

            // JMP
            0x4C => {
                pc = abs!();
                3
            }
            0x6C => {
                pc = ind!();
                5
            }

            // JSR/RTS/RTI
            0x20 => {
                let target = abs!();
                let npc = pc.wrapping_sub(1);
                bus.cpu_write(0x0100 | sp as u16, (npc >> 8) as u8);
                sp = sp.wrapping_sub(1);
                bus.cpu_write(0x0100 | sp as u16, npc as u8);
                sp = sp.wrapping_sub(1);
                pc = target;
                6
            }
            0x60 => {
                sp = sp.wrapping_add(1);
                let lo = bus.cpu_read(0x0100 | sp as u16) as u16;
                sp = sp.wrapping_add(1);
                let hi = bus.cpu_read(0x0100 | sp as u16) as u16;
                pc = (lo | (hi << 8)).wrapping_add(1);
                6
            }
            0x40 => {
                sp = sp.wrapping_add(1);
                self.status = CpuFlags::from_byte(bus.cpu_read(0x0100 | sp as u16));
                sp = sp.wrapping_add(1);
                let lo = bus.cpu_read(0x0100 | sp as u16) as u16;
                sp = sp.wrapping_add(1);
                let hi = bus.cpu_read(0x0100 | sp as u16) as u16;
                pc = lo | (hi << 8);
                6
            }

            // Branch
            0x10 => {
                let offset = bus.cpu_read(pc) as i8;
                pc = pc.wrapping_add(1);
                if !self.status.negative {
                    pc = pc.wrapping_add(offset as u16);
                    3
                } else {
                    2
                }
            }
            0x30 => {
                let offset = bus.cpu_read(pc) as i8;
                pc = pc.wrapping_add(1);
                if self.status.negative {
                    pc = pc.wrapping_add(offset as u16);
                    3
                } else {
                    2
                }
            }
            0x50 => {
                let offset = bus.cpu_read(pc) as i8;
                pc = pc.wrapping_add(1);
                if self.status.overflow {
                    pc = pc.wrapping_add(offset as u16);
                    3
                } else {
                    2
                }
            }
            0x70 => {
                let offset = bus.cpu_read(pc) as i8;
                pc = pc.wrapping_add(1);
                if !self.status.overflow {
                    pc = pc.wrapping_add(offset as u16);
                    3
                } else {
                    2
                }
            }
            0x90 => {
                let offset = bus.cpu_read(pc) as i8;
                pc = pc.wrapping_add(1);
                if !self.status.carry {
                    pc = pc.wrapping_add(offset as u16);
                    3
                } else {
                    2
                }
            }
            0xB0 => {
                let offset = bus.cpu_read(pc) as i8;
                pc = pc.wrapping_add(1);
                if self.status.carry {
                    pc = pc.wrapping_add(offset as u16);
                    3
                } else {
                    2
                }
            }
            0xD0 => {
                let offset = bus.cpu_read(pc) as i8;
                pc = pc.wrapping_add(1);
                if !self.status.zero {
                    pc = pc.wrapping_add(offset as u16);
                    3
                } else {
                    2
                }
            }
            0xF0 => {
                let offset = bus.cpu_read(pc) as i8;
                pc = pc.wrapping_add(1);
                if self.status.zero {
                    pc = pc.wrapping_add(offset as u16);
                    3
                } else {
                    2
                }
            }

            // Clear/Set flags
            0x18 => {
                self.status.carry = false;
                2
            }
            0x38 => {
                self.status.carry = true;
                2
            }
            0x58 => {
                self.status.interrupt_disable = false;
                2
            }
            0x78 => {
                self.status.interrupt_disable = true;
                2
            }
            0xB8 => {
                self.status.overflow = false;
                2
            }
            0xD8 => {
                self.status.decimal = false;
                2
            }
            0xF8 => {
                self.status.decimal = true;
                2
            }
            0xEA => 2, // NOP

            _ => 2,
        };
        self.pc = pc;
        self.sp = sp;
        result
    }
}

#[cfg(test)]
mod tests {
    use crate::bus::NesBusImpl;
    use crate::cpu_ref::Cpu6502;

    /// Build a minimal NROM ROM with the given PRG data at $8000
    fn make_test_rom(prg_data: &[u8]) -> NesBusImpl {
        make_test_rom_vec(prg_data, 0x8000, 0x8000)
    }

    /// Build a minimal NROM ROM with custom reset and NMI vectors
    fn make_test_rom_vec(prg_data: &[u8], reset_vec: u16, nmi_vec: u16) -> NesBusImpl {
        let mut data = vec![0u8; 16 + 16384 + 8192]; // header + 16KB PRG + 8KB CHR
        data[0..4].copy_from_slice(b"NES\x1a");
        data[4] = 1; // 1 * 16KB PRG
        data[5] = 1; // 1 * 8KB CHR
        // Copy test program to PRG
        let prg_start = 0x10; // after 16-byte header
        let copy_len = prg_data.len().min(16384);
        data[prg_start..prg_start + copy_len].copy_from_slice(prg_data);
        // Set vectors at the last 6 bytes of 16KB PRG (offset 0x3FFA..0x3FFF)
        let vec_base = prg_start + 0x3FFA;
        data[vec_base] = nmi_vec as u8;
        data[vec_base + 1] = (nmi_vec >> 8) as u8;
        data[vec_base + 2] = reset_vec as u8;
        data[vec_base + 3] = (reset_vec >> 8) as u8;
        let rom = crate::rom::parse_rom(&data).unwrap();
        // 优先使用 linkme 注册的 mapper，回退到内置 NROM
        let mapper = crate::mapper::create_mapper(0, &rom)
            .unwrap_or_else(|| crate::mapper::registry::builtin_nrom(&rom));
        let cartridge = crate::mapper::Cartridge::new_simple(
            crate::mapper::CartridgeMetadata {
                mapper_id: 0,
                submapper_id: 0,
                prg_rom_size: 1,
                chr_rom_size: 1,
                has_sram: false,
                has_trainer: false,
                battery_backed: false,
            },
            rom.prg_rom.clone(),
            crate::mapper::ChrStorage::Rom(rom.chr_rom.clone().unwrap_or_default()),
            mapper,
        );
        NesBusImpl::new(cartridge)
    }

    // ─── Test 1: CPU reset ─────
    #[test]
    fn test_cpu_reset() {
        // Minimal program: just BRK
        let prg = [0x00u8; 6];
        let mut bus = make_test_rom(&prg);
        let mut cpu = Cpu6502::new();
        cpu.reset(&mut bus);
        // RESET vector at $FFFC = $8000
        assert_eq!(cpu.pc, 0x8000);
        assert_eq!(cpu.sp, 0xFA); // 0xFD - 3
        assert!(cpu.status.interrupt_disable);
    }

    // ─── Test 2: LDA immediate ─────
    #[test]
    fn test_lda_immediate() {
        let prg = [0xA9, 0x42, 0x00]; // LDA #$42, BRK
        let mut bus = make_test_rom(&prg);
        let mut cpu = Cpu6502::new();
        cpu.reset(&mut bus);
        cpu.step(&mut bus); // LDA #$42
        assert_eq!(cpu.a, 0x42);
        assert!(!cpu.status.zero);
        assert!(!cpu.status.negative);
    }

    // ─── Test 3: LDA zero flag ─────
    #[test]
    fn test_lda_zero_flag() {
        let prg = [0xA9, 0x00, 0x00]; // LDA #$00, BRK
        let mut bus = make_test_rom(&prg);
        let mut cpu = Cpu6502::new();
        cpu.reset(&mut bus);
        cpu.step(&mut bus); // LDA #$00
        assert_eq!(cpu.a, 0x00);
        assert!(cpu.status.zero);
    }

    // ─── Test 4: LDA negative flag ─────
    #[test]
    fn test_lda_negative_flag() {
        let prg = [0xA9, 0x80, 0x00]; // LDA #$80, BRK
        let mut bus = make_test_rom(&prg);
        let mut cpu = Cpu6502::new();
        cpu.reset(&mut bus);
        cpu.step(&mut bus); // LDA #$80
        assert_eq!(cpu.a, 0x80);
        assert!(cpu.status.negative);
    }

    // ─── Test 5: STA absolute ─────
    #[test]
    fn test_sta_absolute() {
        // LDA #$42, STA $0050, BRK
        let prg = [0xA9, 0x42, 0x8D, 0x50, 0x00, 0x00];
        let mut bus = make_test_rom(&prg);
        let mut cpu = Cpu6502::new();
        cpu.reset(&mut bus);
        cpu.step(&mut bus); // LDA #$42
        assert_eq!(cpu.a, 0x42);
        cpu.step(&mut bus); // STA $0050
        assert_eq!(bus.ram[0x50], 0x42);
    }

    // ─── Test 6: ADC basic ─────
    #[test]
    fn test_adc_basic() {
        // LDA #$10, ADC #$20, BRK
        let prg = [0xA9, 0x10, 0x69, 0x20, 0x00];
        let mut bus = make_test_rom(&prg);
        let mut cpu = Cpu6502::new();
        cpu.reset(&mut bus);
        cpu.step(&mut bus); // LDA #$10
        assert_eq!(cpu.a, 0x10);
        cpu.step(&mut bus); // ADC #$20
        assert_eq!(cpu.a, 0x30);
        assert!(!cpu.status.carry);
        assert!(!cpu.status.zero);
        assert!(!cpu.status.negative);
    }

    // ─── Test 7: ADC with carry ─────
    #[test]
    fn test_adc_with_carry() {
        // LDA #$FF, ADC #$01 (C=0 → overflow), BRK
        let prg = [0xA9, 0xFF, 0x69, 0x01, 0x00];
        let mut bus = make_test_rom(&prg);
        let mut cpu = Cpu6502::new();
        cpu.reset(&mut bus);
        cpu.step(&mut bus); // LDA #$FF
        assert_eq!(cpu.a, 0xFF);
        cpu.step(&mut bus); // ADC #$01
        assert_eq!(cpu.a, 0x00);
        assert!(cpu.status.carry); // 0xFF + 0x01 = 0x100 → carry
        assert!(cpu.status.zero); // result is 0
    }

    // ─── Test 8: SBC basic ─────
    #[test]
    fn test_sbc_basic() {
        // LDA #$30, SEC, SBC #$10, BRK
        let prg = [0xA9, 0x30, 0x38, 0xE9, 0x10, 0x00];
        let mut bus = make_test_rom(&prg);
        let mut cpu = Cpu6502::new();
        cpu.reset(&mut bus);
        cpu.step(&mut bus); // LDA #$30
        assert_eq!(cpu.a, 0x30);
        cpu.step(&mut bus); // SEC (C = 1)
        assert!(cpu.status.carry);
        cpu.step(&mut bus); // SBC #$10 → A = 0x30 - 0x10 = 0x20
        assert_eq!(cpu.a, 0x20);
        assert!(cpu.status.carry); // borrow = 0
        assert!(!cpu.status.zero);
    }

    // ─── Test 9: JMP absolute ─────
    #[test]
    fn test_jmp_absolute() {
        // JMP $8005, NOP, BRK (target is BRK at $8005)
        let prg = [0x4C, 0x05, 0x80, 0xEA, 0x00];
        let mut bus = make_test_rom(&prg);
        let mut cpu = Cpu6502::new();
        cpu.reset(&mut bus);
        cpu.step(&mut bus); // JMP $8005
        assert_eq!(cpu.pc, 0x8005);
    }

    // ─── Test 10: JSR / RTS ─────
    #[test]
    fn test_jsr_rts() {
        // $8000: JSR $8006
        // $8003: BRK          ← landing after RTS
        // $8006: RTS
        let prg = [
            0x20, 0x06, 0x80, // JSR $8006
            0x00, // BRK ($8003)
            0x00, // padding ($8004)
            0x00, // padding ($8005)
            0x60, // RTS  ($8006)
        ];
        let mut bus = make_test_rom(&prg);
        let mut cpu = Cpu6502::new();
        cpu.reset(&mut bus);

        let sp_before = cpu.sp;
        cpu.step(&mut bus); // JSR → PC = $8006
        assert_eq!(cpu.pc, 0x8006);
        assert_eq!(cpu.sp, sp_before.wrapping_sub(2)); // pushed 2 bytes (return addr)
        cpu.step(&mut bus); // RTS → PC = $8003
        assert_eq!(cpu.pc, 0x8003);
        assert_eq!(cpu.sp, sp_before); // SP restored
    }

    // ─── Test 11: Branch taken ─────
    #[test]
    fn test_branch_taken() {
        // LDA #$01 (Z=0), BNE +$02 (taken), BRK, NOP, BRK (landing)
        let prg = [
            0xA9, 0x01, // LDA #$01  ($8000)
            0xD0, 0x02, // BNE +$02  ($8002)
            0x00, // BRK ($8004) — skipped
            0xEA, // NOP ($8005) — skipped
            0x00, // BRK ($8006) — landing
        ];
        let mut bus = make_test_rom(&prg);
        let mut cpu = Cpu6502::new();
        cpu.reset(&mut bus);
        cpu.step(&mut bus); // LDA #$01 → Z=0
        cpu.step(&mut bus); // BNE +$02 → taken, PC lands on BRK at $8006
        assert_eq!(cpu.pc, 0x8006);
    }

    // ─── Test 12: Branch not taken ─────
    #[test]
    fn test_branch_not_taken() {
        // LDA #$00 (Z=1), BNE +$02 (not taken), BRK
        let prg = [
            0xA9, 0x00, // LDA #$00  ($8000)
            0xD0, 0x02, // BNE +$02  ($8002)
            0x00, // BRK ($8004) — landing (branch not taken)
            0xEA, // NOP ($8005)
            0x00, // BRK ($8006)
        ];
        let mut bus = make_test_rom(&prg);
        let mut cpu = Cpu6502::new();
        cpu.reset(&mut bus);
        cpu.step(&mut bus); // LDA #$00 → Z=1
        cpu.step(&mut bus); // BNE +$02 → NOT taken, PC = $8004
        assert_eq!(cpu.pc, 0x8004);
    }

    // ─── Test 13: PHA / PLA ─────
    #[test]
    fn test_pha_pla() {
        // LDA #$55, PHA, LDA #$00, PLA, BRK
        let prg = [
            0xA9, 0x55, // LDA #$55
            0x48, // PHA
            0xA9, 0x00, // LDA #$00
            0x68, // PLA
            0x00, // BRK
        ];
        let mut bus = make_test_rom(&prg);
        let mut cpu = Cpu6502::new();
        cpu.reset(&mut bus);
        cpu.step(&mut bus); // LDA #$55
        assert_eq!(cpu.a, 0x55);
        cpu.step(&mut bus); // PHA
        cpu.step(&mut bus); // LDA #$00
        assert_eq!(cpu.a, 0x00);
        cpu.step(&mut bus); // PLA → A restored to 0x55
        assert_eq!(cpu.a, 0x55);
        assert!(!cpu.status.zero);
    }

    // ─── Test 14: INX / DEX ─────
    #[test]
    fn test_inx_dex() {
        // INX, INX, DEX, DEX, BRK
        let prg = [0xE8, 0xE8, 0xCA, 0xCA, 0x00];
        let mut bus = make_test_rom(&prg);
        let mut cpu = Cpu6502::new();
        cpu.reset(&mut bus);
        cpu.step(&mut bus); // INX → X=1
        assert_eq!(cpu.x, 1);
        cpu.step(&mut bus); // INX → X=2
        assert_eq!(cpu.x, 2);
        cpu.step(&mut bus); // DEX → X=1
        assert_eq!(cpu.x, 1);
        assert!(!cpu.status.zero);
        cpu.step(&mut bus); // DEX → X=0
        assert_eq!(cpu.x, 0);
        assert!(cpu.status.zero);
    }

    // ─── Test 15: AND / EOR / ORA ─────
    #[test]
    fn test_and_eor_ora() {
        // LDA #$FF, AND #$0F, EOR #$F0, ORA #$0F, BRK
        let prg = [
            0xA9, 0xFF, // LDA #$FF
            0x29, 0x0F, // AND #$0F → A = 0x0F
            0x49, 0xF0, // EOR #$F0 → A = 0xFF
            0x09, 0x0F, // ORA #$0F → A = 0xFF
            0x00, // BRK
        ];
        let mut bus = make_test_rom(&prg);
        let mut cpu = Cpu6502::new();
        cpu.reset(&mut bus);
        cpu.step(&mut bus); // LDA #$FF
        assert_eq!(cpu.a, 0xFF);
        cpu.step(&mut bus); // AND #$0F
        assert_eq!(cpu.a, 0x0F);
        assert!(!cpu.status.zero);
        assert!(!cpu.status.negative);
        cpu.step(&mut bus); // EOR #$F0
        assert_eq!(cpu.a, 0xFF);
        assert!(cpu.status.negative); // bit 7 set
        cpu.step(&mut bus); // ORA #$0F
        assert_eq!(cpu.a, 0xFF);
    }

    // ─── Test 16: CMP ─────
    #[test]
    fn test_cmp() {
        // LDA #$42, CMP #$42 (equal), CMP #$10 (greater), CMP #$80 (less), BRK
        let prg = [
            0xA9, 0x42, // LDA #$42
            0xC9, 0x42, // CMP #$42  → equal
            0xC9, 0x10, // CMP #$10  → greater
            0xC9, 0x80, // CMP #$80  → less
            0x00, // BRK
        ];
        let mut bus = make_test_rom(&prg);
        let mut cpu = Cpu6502::new();
        cpu.reset(&mut bus);
        cpu.step(&mut bus); // LDA #$42
        assert_eq!(cpu.a, 0x42);
        cpu.step(&mut bus); // CMP #$42: equal
        assert!(cpu.status.zero);
        assert!(cpu.status.carry); // A >= M
        cpu.step(&mut bus); // CMP #$10: greater
        assert!(!cpu.status.zero);
        assert!(cpu.status.carry); // A >= M
        cpu.step(&mut bus); // CMP #$80: less
        assert!(!cpu.status.zero);
        assert!(!cpu.status.carry); // A < M
    }

    // ─── Test 17: ASL / LSR ─────
    #[test]
    fn test_asl_lsr() {
        // LDA #$81, ASL A, LSR A, BRK
        let prg = [0xA9, 0x81, 0x0A, 0x4A, 0x00];
        let mut bus = make_test_rom(&prg);
        let mut cpu = Cpu6502::new();
        cpu.reset(&mut bus);
        cpu.step(&mut bus); // LDA #$81 → A = 0x81
        assert_eq!(cpu.a, 0x81);
        cpu.step(&mut bus); // ASL A → A = 0x02, C = 1
        assert_eq!(cpu.a, 0x02);
        assert!(cpu.status.carry);
        cpu.step(&mut bus); // LSR A → A = 0x01, C = 0
        assert_eq!(cpu.a, 0x01);
        assert!(!cpu.status.carry);
    }

    // ─── Test 18: BIT ─────
    #[test]
    fn test_bit() {
        // Store $C0 at ZP $10 via direct RAM write
        // Test: LDA #$00, BIT $10 → Z=1 (A & $C0 = 0), N=1, V=1
        //        LDA #$80, BIT $10 → Z=0, N=1, V=1
        let prg = [
            0xA9, 0x00, // LDA #$00  ($8000)
            0x24, 0x10, // BIT $10   ($8002)
            0xA9, 0x80, // LDA #$80  ($8004)
            0x24, 0x10, // BIT $10   ($8006)
            0x00, // BRK       ($8008)
        ];
        let mut bus = make_test_rom(&prg);
        let mut cpu = Cpu6502::new();
        // Write $C0 to ZP $10 (bits 7 and 6 set)
        bus.ram[0x10] = 0xC0;
        cpu.reset(&mut bus);
        cpu.step(&mut bus); // LDA #$00
        assert_eq!(cpu.a, 0x00);
        cpu.step(&mut bus); // BIT $10: A & $C0 = 0 → Z=1; N=1 (bit 7); V=1 (bit 6)
        assert!(cpu.status.zero);
        assert!(cpu.status.negative);
        assert!(cpu.status.overflow);
        cpu.step(&mut bus); // LDA #$80
        assert_eq!(cpu.a, 0x80);
        cpu.step(&mut bus); // BIT $10: A & $C0 = $80 → Z=0
        assert!(!cpu.status.zero);
        assert!(cpu.status.negative);
        assert!(cpu.status.overflow);
    }

    // ─── Test 19: NMI ─────
    #[test]
    fn test_nmi() {
        // NMI handler at $8100, main program at $8000
        // Set NMI vector to $8100
        let prg = [0x00]; // BRK at $8000 (main)
        // NMI handler is empty (just BRK) at $8100
        let mut bus = make_test_rom_vec(&prg, 0x8000, 0x8100);
        let mut cpu = Cpu6502::new();
        cpu.reset(&mut bus);
        assert_eq!(cpu.pc, 0x8000);

        let sp_before = cpu.sp;
        cpu.trigger_nmi(&mut bus);
        // PC jumps to NMI vector
        assert_eq!(cpu.pc, 0x8100);
        // SP decremented by 3 (pushed PCL, PCH, status)
        assert_eq!(cpu.sp, sp_before.wrapping_sub(3));
        assert!(cpu.status.interrupt_disable);
    }

    // ─── Test 20: Full roundtrip ─────
    #[test]
    fn test_roundtrip() {
        // Loop: LDA #$42, STA $0050, LDA $0050, CMP #$42, BEQ +$02, JMP $8000
        // The BEQ skips 2 bytes to a JMP that loops back
        let prg = [
            0xA9, 0x42, // $8000: LDA #$42
            0x8D, 0x50, 0x00, // $8002: STA $0050
            0xAD, 0x50, 0x00, // $8005: LDA $0050
            0xC9, 0x42, // $8008: CMP #$42
            0xF0, 0x02, // $800A: BEQ +$02 → $800E
            0xEA, // $800C: NOP (skipped by BEQ)
            0xEA, // $800D: NOP (skipped by BEQ)
            0x4C, 0x00, 0x80, // $800E: JMP $8000
        ];
        let mut bus = make_test_rom(&prg);
        let mut cpu = Cpu6502::new();
        cpu.reset(&mut bus);

        // Run 100 steps
        for _ in 0..100 {
            cpu.step(&mut bus);
        }

        // After 100 steps, state should be consistent
        assert_eq!(cpu.a, 0x42);
        assert_eq!(bus.ram[0x50], 0x42);
        assert!(cpu.status.zero); // CMP #$42: equal
        assert!(cpu.status.carry); // CMP #$42: A >= M
    }
}
