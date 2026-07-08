//! Cranelift AOT 代码生成
//!
//! 将 IR6502 中间表示编译为 Cranelift IR，再通过 cranelift-object 输出为
//! 原生目标文件 (.o)，最终链接为 .dll 动态库。
//!
//! # ABI
//!
//! 每个 6502 基本块编译为一个 `extern "C"` 函数：
//!
//! ```c
//! uint16_t block_XXXX(NesBusImpl* bus, NativeCpuState* cpu);
//! ```
//!
//! - `bus`: NES 总线指针，通过 `nes_read8`/`nes_write8` 外部函数访问内存
//! - `cpu`: CPU 状态指针，通过 load/store 直接访问字段
//! - 返回值: 下一个 PC 地址 (0 = 回退到解释器)

use crate::ir6502::{BranchCondition, IrOp, Operand};

use cranelift_codegen::Context;
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::types::*;
use cranelift_codegen::ir::{
    AbiParam, FuncRef, InstBuilder, MemFlagsData, Signature, UserFuncName, Value,
    immediates::Offset32,
};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::settings;
use cranelift_codegen::settings::Configurable;
use cranelift_control::ControlPlane;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};

use std::collections::HashMap;

mod cpu_offset {
    pub const A: i32 = 0;
    pub const X: i32 = 1;
    pub const Y: i32 = 2;
    pub const SP: i32 = 3;
    pub const CARRY: i32 = 4;
    pub const ZERO: i32 = 5;
    pub const NEGATIVE: i32 = 6;
    pub const OVERFLOW: i32 = 7;
    pub const INTERRUPT_DISABLE: i32 = 8;
}

#[derive(Debug, Clone)]
pub struct CompiledBlock {
    pub address: u16,
    pub name: String,
}

pub struct CraneliftAot {
    module: ObjectModule,
    builder_ctx: FunctionBuilderContext,
    ctx: Context,
    blocks: Vec<CompiledBlock>,
    block_names: HashMap<u16, String>,
    read8_id: cranelift_module::FuncId,
    write8_id: cranelift_module::FuncId,
    advance_id: cranelift_module::FuncId,
}

impl CraneliftAot {
    pub fn new() -> Result<Self, String> {
        let mut flag_builder = settings::builder();
        flag_builder
            .set("opt_level", "speed")
            .map_err(|e| format!("settings: {}", e))?;
        let flags = settings::Flags::new(flag_builder);

        let isa_builder = cranelift_native::builder().map_err(|e| format!("native isa: {}", e))?;
        let isa = isa_builder
            .finish(flags)
            .map_err(|e| format!("isa finish: {}", e))?;

        let obj_builder = ObjectBuilder::new(
            isa,
            "nes_recompiled",
            cranelift_module::default_libcall_names(),
        )
        .map_err(|e| format!("object builder: {}", e))?;

        let mut module = ObjectModule::new(obj_builder);

        // Declare imported functions at module level
        let mut read8_sig = Signature::new(CallConv::SystemV);
        read8_sig.params.push(AbiParam::new(I64));
        read8_sig.params.push(AbiParam::new(I16));
        read8_sig.returns.push(AbiParam::new(I8));
        let read8_id = module
            .declare_function("nes_read8", Linkage::Import, &read8_sig)
            .map_err(|e| format!("declare nes_read8: {}", e))?;

        let mut write8_sig = Signature::new(CallConv::SystemV);
        write8_sig.params.push(AbiParam::new(I64));
        write8_sig.params.push(AbiParam::new(I16));
        write8_sig.params.push(AbiParam::new(I8));
        let write8_id = module
            .declare_function("nes_write8", Linkage::Import, &write8_sig)
            .map_err(|e| format!("declare nes_write8: {}", e))?;

        // Declare nes_advance_cycles(bus: *mut NesBusImpl, cycles: u32)
        let mut advance_sig = Signature::new(CallConv::SystemV);
        advance_sig.params.push(AbiParam::new(I64));
        advance_sig.params.push(AbiParam::new(I32));
        let advance_id = module
            .declare_function("nes_advance_cycles", Linkage::Import, &advance_sig)
            .map_err(|e| format!("declare nes_advance_cycles: {}", e))?;

        Ok(CraneliftAot {
            module,
            builder_ctx: FunctionBuilderContext::new(),
            ctx: Context::new(),
            blocks: Vec::new(),
            block_names: HashMap::new(),
            read8_id,
            write8_id,
            advance_id,
        })
    }

    pub fn compile_block(&mut self, address: u16, ir_ops: &[IrOp]) -> Result<(), String> {
        let name = format!("block_{:04X}", address);
        let func_name = UserFuncName::user(0, self.blocks.len() as u32);

        let mut sig = Signature::new(CallConv::SystemV);
        sig.params.push(AbiParam::new(I64));
        sig.params.push(AbiParam::new(I64));
        sig.returns.push(AbiParam::new(I32));

        self.ctx.func.signature = sig.clone();
        self.ctx.func.name = func_name;

        let mut bcx = FunctionBuilder::new(&mut self.ctx.func, &mut self.builder_ctx);
        let entry_block = bcx.create_block();
        bcx.append_block_params_for_function_params(entry_block);
        bcx.switch_to_block(entry_block);
        bcx.seal_block(entry_block);

        let params = bcx.block_params(entry_block);
        let bus_ptr = params[0];
        let cpu_ptr = params[1];

        let read8_ref = self
            .module
            .declare_func_in_func(self.read8_id, &mut bcx.func);
        let write8_ref = self
            .module
            .declare_func_in_func(self.write8_id, &mut bcx.func);
        let advance_ref = self
            .module
            .declare_func_in_func(self.advance_id, &mut bcx.func);

        let mut current_addr = address;
        let mut has_unconditional_return = false;
        // total_cycles: 跟踪块内累计 CPU 周期数，初始为 0
        let zero32 = bcx.ins().iconst(I32, 0);
        let mut total_cycles = zero32;

        for op in ir_ops {
            // 在 terminal 指令之后跳过后续指令（它们位于 dead block 中）
            if has_unconditional_return {
                break;
            }
            translate_op(
                &mut bcx,
                op,
                bus_ptr,
                cpu_ptr,
                current_addr,
                read8_ref,
                write8_ref,
                advance_ref,
                &mut total_cycles,
            )?;
            match op {
                IrOp::Return | IrOp::Jump(_) | IrOp::JumpIndirect(_) | IrOp::Branch { .. } => {
                    has_unconditional_return = true;
                }
                _ => {}
            }
            current_addr = current_addr.wrapping_add(ir_op_len(op));
        }

        if !has_unconditional_return {
            // 返回 (0 << 16) | total_cycles = total_cycles
            // 高 16 位 PC=0 表示回退到解释器
            bcx.ins().return_(&[total_cycles]);
        }

        bcx.finalize();

        let func_id = self
            .module
            .declare_function(&name, Linkage::Export, &sig)
            .map_err(|e| format!("declare {}: {}", name, e))?;
        let mut ctrl_plane = ControlPlane::default();
        self.module
            .define_function_with_control_plane(func_id, &mut self.ctx, &mut ctrl_plane)
            .map_err(|e| format!("define {}: {}", name, e))?;
        self.module.clear_context(&mut self.ctx);

        self.blocks.push(CompiledBlock {
            address,
            name: name.clone(),
        });
        self.block_names.insert(address, name);

        Ok(())
    }

    pub fn finish(self) -> Result<(Vec<u8>, Vec<CompiledBlock>, HashMap<u16, String>), String> {
        let product = self.module.finish();
        let bytes = product.emit().map_err(|e| format!("emit: {}", e))?;
        Ok((bytes, self.blocks, self.block_names))
    }

    pub fn blocks(&self) -> &[CompiledBlock] {
        &self.blocks
    }
    pub fn block_names(&self) -> &HashMap<u16, String> {
        &self.block_names
    }
}

// ── External function declarations ──

// ── CPU state helpers ──

fn load_cpu_u8(bcx: &mut FunctionBuilder, cpu_ptr: Value, offset: i32) -> Value {
    let mut flags = MemFlagsData::new();
    flags.set_notrap();
    let addr = bcx.ins().iadd_imm(cpu_ptr, offset as i64);
    let off = Offset32::new(0);
    bcx.ins().load(I8, flags, addr, off)
}

fn store_cpu_u8(bcx: &mut FunctionBuilder, cpu_ptr: Value, offset: i32, val: Value) {
    let mut flags = MemFlagsData::new();
    flags.set_notrap();
    let addr = bcx.ins().iadd_imm(cpu_ptr, offset as i64);
    let off = Offset32::new(0);
    bcx.ins().store(flags, val, addr, off);
}

fn set_zn(bcx: &mut FunctionBuilder, cpu_ptr: Value, val: Value) {
    // icmp returns i8 (0 or 1), no need to extend
    let zero = bcx.ins().icmp_imm(IntCC::Equal, val, 0);
    store_cpu_u8(bcx, cpu_ptr, cpu_offset::ZERO, zero);
    let neg = bcx.ins().ushr_imm(val, 7);
    store_cpu_u8(bcx, cpu_ptr, cpu_offset::NEGATIVE, neg);
}

// ── Memory access helpers ──

fn call_read8(bcx: &mut FunctionBuilder, bus_ptr: Value, addr: u16, read8_ref: FuncRef) -> Value {
    let addr_val = bcx.ins().iconst(I16, addr as i64);
    let call_inst = bcx.ins().call(read8_ref, &[bus_ptr, addr_val]);
    let results = bcx.inst_results(call_inst);
    results[0]
}

fn call_read8_val(
    bcx: &mut FunctionBuilder,
    bus_ptr: Value,
    addr: Value,
    read8_ref: FuncRef,
) -> Value {
    let call_inst = bcx.ins().call(read8_ref, &[bus_ptr, addr]);
    let results = bcx.inst_results(call_inst);
    results[0]
}

fn call_write8(
    bcx: &mut FunctionBuilder,
    bus_ptr: Value,
    addr: u16,
    val: Value,
    write8_ref: FuncRef,
) {
    let addr_val = bcx.ins().iconst(I16, addr as i64);
    bcx.ins().call(write8_ref, &[bus_ptr, addr_val, val]);
}

fn call_write8_val(
    bcx: &mut FunctionBuilder,
    bus_ptr: Value,
    addr: Value,
    val: Value,
    write8_ref: FuncRef,
) {
    bcx.ins().call(write8_ref, &[bus_ptr, addr, val]);
}

// ── Instruction length ──

fn ir_op_len(op: &IrOp) -> u16 {
    match op {
        IrOp::LoadA(o)
        | IrOp::LoadX(o)
        | IrOp::LoadY(o)
        | IrOp::AddWithCarry(o)
        | IrOp::SubWithCarry(o)
        | IrOp::And(o)
        | IrOp::Or(o)
        | IrOp::Xor(o)
        | IrOp::Compare(o) => match o {
            Operand::Immediate(_) => 2,
            Operand::Zeropage(_) | Operand::ZeropageX(_) | Operand::ZeropageY(_) => 2,
            Operand::Address(_)
            | Operand::Absolute(_)
            | Operand::AbsoluteX(_)
            | Operand::AbsoluteY(_) => 3,
            Operand::IndirectX(_) | Operand::IndirectY(_) => 2,
        },
        IrOp::StoreA(a) | IrOp::StoreX(a) | IrOp::StoreY(a) => {
            if *a < 0x100 {
                2
            } else {
                3
            }
        }
        IrOp::Inc(a) | IrOp::Dec(a) | IrOp::ShiftLeft(a) | IrOp::ShiftRight(a) => match *a {
            0xFFFF | 0xFFFE => 1,
            a if a < 0x100 => 2,
            _ => 3,
        },
        IrOp::Branch { .. } => 2,
        IrOp::Jump(_) | IrOp::JumpIndirect(_) | IrOp::Call(_) => 3,
        IrOp::Return | IrOp::Nop => 1,
        IrOp::SetFlag { .. } => 1,
        IrOp::AdvanceCycles(_) => 0,
    }
}

/// 打包返回值为 u32: (pc << 16) | total_cycles
fn pack_return(bcx: &mut FunctionBuilder, pc: u16, total_cycles: Value) -> Value {
    let pc_val = bcx.ins().iconst(I32, ((pc as u32) << 16) as i64);
    bcx.ins().bor(pc_val, total_cycles)
}

// ── Operand loading ──

fn load_operand(
    bcx: &mut FunctionBuilder,
    operand: &Operand,
    bus_ptr: Value,
    cpu_ptr: Value,
    read8_ref: FuncRef,
) -> Value {
    match operand {
        Operand::Immediate(v) => bcx.ins().iconst(I8, *v as i64),
        Operand::Zeropage(a) => call_read8(bcx, bus_ptr, *a as u16, read8_ref),
        Operand::ZeropageX(a) => {
            let base = bcx.ins().iconst(I16, *a as i64);
            let x = load_cpu_u8(bcx, cpu_ptr, cpu_offset::X);
            let x_16 = bcx.ins().uextend(I16, x);
            let addr = bcx.ins().iadd(base, x_16);
            call_read8_val(bcx, bus_ptr, addr, read8_ref)
        }
        Operand::ZeropageY(a) => {
            let base = bcx.ins().iconst(I16, *a as i64);
            let y = load_cpu_u8(bcx, cpu_ptr, cpu_offset::Y);
            let y_16 = bcx.ins().uextend(I16, y);
            let addr = bcx.ins().iadd(base, y_16);
            call_read8_val(bcx, bus_ptr, addr, read8_ref)
        }
        Operand::Address(a) | Operand::Absolute(a) => call_read8(bcx, bus_ptr, *a, read8_ref),
        Operand::AbsoluteX(a) => {
            let base = bcx.ins().iconst(I16, *a as i64);
            let x = load_cpu_u8(bcx, cpu_ptr, cpu_offset::X);
            let x_16 = bcx.ins().uextend(I16, x);
            let addr = bcx.ins().iadd(base, x_16);
            call_read8_val(bcx, bus_ptr, addr, read8_ref)
        }
        Operand::AbsoluteY(a) => {
            let base = bcx.ins().iconst(I16, *a as i64);
            let y = load_cpu_u8(bcx, cpu_ptr, cpu_offset::Y);
            let y_16 = bcx.ins().uextend(I16, y);
            let addr = bcx.ins().iadd(base, y_16);
            call_read8_val(bcx, bus_ptr, addr, read8_ref)
        }
        Operand::IndirectX(zp) => {
            let zp_base = bcx.ins().iconst(I16, *zp as i64);
            let x = load_cpu_u8(bcx, cpu_ptr, cpu_offset::X);
            let x_16 = bcx.ins().uextend(I16, x);
            let ptr_addr = bcx.ins().iadd(zp_base, x_16);
            let lo = call_read8_val(bcx, bus_ptr, ptr_addr, read8_ref);
            let ptr_addr_plus1 = bcx.ins().iadd_imm(ptr_addr, 1);
            let hi = call_read8_val(bcx, bus_ptr, ptr_addr_plus1, read8_ref);
            let hi_16 = bcx.ins().uextend(I16, hi);
            let lo_16 = bcx.ins().uextend(I16, lo);
            let ptr = bcx.ins().ishl_imm(hi_16, 8);
            let ptr = bcx.ins().bor(ptr, lo_16);
            call_read8_val(bcx, bus_ptr, ptr, read8_ref)
        }
        Operand::IndirectY(zp) => {
            let zp_base = bcx.ins().iconst(I16, *zp as i64);
            let lo = call_read8_val(bcx, bus_ptr, zp_base, read8_ref);
            let zp_base_plus1 = bcx.ins().iadd_imm(zp_base, 1);
            let hi = call_read8_val(bcx, bus_ptr, zp_base_plus1, read8_ref);
            let hi_16 = bcx.ins().uextend(I16, hi);
            let lo_16 = bcx.ins().uextend(I16, lo);
            let ptr = bcx.ins().ishl_imm(hi_16, 8);
            let ptr = bcx.ins().bor(ptr, lo_16);
            let y = load_cpu_u8(bcx, cpu_ptr, cpu_offset::Y);
            let y_16 = bcx.ins().uextend(I16, y);
            let addr = bcx.ins().iadd(ptr, y_16);
            call_read8_val(bcx, bus_ptr, addr, read8_ref)
        }
    }
}

// ── IR operation translation ──

#[allow(clippy::too_many_arguments)]
fn translate_op(
    bcx: &mut FunctionBuilder,
    op: &IrOp,
    bus_ptr: Value,
    cpu_ptr: Value,
    instr_addr: u16,
    read8_ref: FuncRef,
    write8_ref: FuncRef,
    advance_ref: FuncRef,
    total_cycles: &mut Value,
) -> Result<(), String> {
    match op {
        IrOp::Nop => Ok(()),

        IrOp::LoadA(operand) => {
            let val = load_operand(bcx, operand, bus_ptr, cpu_ptr, read8_ref);
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::A, val);
            set_zn(bcx, cpu_ptr, val);
            Ok(())
        }
        IrOp::LoadX(operand) => {
            let val = load_operand(bcx, operand, bus_ptr, cpu_ptr, read8_ref);
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::X, val);
            set_zn(bcx, cpu_ptr, val);
            Ok(())
        }
        IrOp::LoadY(operand) => {
            let val = load_operand(bcx, operand, bus_ptr, cpu_ptr, read8_ref);
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::Y, val);
            set_zn(bcx, cpu_ptr, val);
            Ok(())
        }

        IrOp::StoreA(addr) => {
            let a = load_cpu_u8(bcx, cpu_ptr, cpu_offset::A);
            call_write8(bcx, bus_ptr, *addr, a, write8_ref);
            Ok(())
        }
        IrOp::StoreX(addr) => {
            let x = load_cpu_u8(bcx, cpu_ptr, cpu_offset::X);
            call_write8(bcx, bus_ptr, *addr, x, write8_ref);
            Ok(())
        }
        IrOp::StoreY(addr) => {
            let y = load_cpu_u8(bcx, cpu_ptr, cpu_offset::Y);
            call_write8(bcx, bus_ptr, *addr, y, write8_ref);
            Ok(())
        }

        IrOp::AddWithCarry(operand) => {
            translate_adc(bcx, operand, bus_ptr, cpu_ptr, read8_ref, write8_ref)
        }
        IrOp::SubWithCarry(operand) => {
            translate_sbc(bcx, operand, bus_ptr, cpu_ptr, read8_ref, write8_ref)
        }

        IrOp::And(operand) => {
            let a = load_cpu_u8(bcx, cpu_ptr, cpu_offset::A);
            let val = load_operand(bcx, operand, bus_ptr, cpu_ptr, read8_ref);
            let result = bcx.ins().band(a, val);
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::A, result);
            set_zn(bcx, cpu_ptr, result);
            Ok(())
        }
        IrOp::Or(operand) => {
            let a = load_cpu_u8(bcx, cpu_ptr, cpu_offset::A);
            let val = load_operand(bcx, operand, bus_ptr, cpu_ptr, read8_ref);
            let result = bcx.ins().bor(a, val);
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::A, result);
            set_zn(bcx, cpu_ptr, result);
            Ok(())
        }
        IrOp::Xor(operand) => {
            let a = load_cpu_u8(bcx, cpu_ptr, cpu_offset::A);
            let val = load_operand(bcx, operand, bus_ptr, cpu_ptr, read8_ref);
            let result = bcx.ins().bxor(a, val);
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::A, result);
            set_zn(bcx, cpu_ptr, result);
            Ok(())
        }

        IrOp::Compare(operand) => {
            let a = load_cpu_u8(bcx, cpu_ptr, cpu_offset::A);
            let val = load_operand(bcx, operand, bus_ptr, cpu_ptr, read8_ref);
            let result = bcx.ins().isub(a, val);
            let ge = bcx.ins().icmp(IntCC::UnsignedGreaterThanOrEqual, a, val);
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::CARRY, ge);
            let zero = bcx.ins().icmp_imm(IntCC::Equal, result, 0);
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::ZERO, zero);
            let neg = bcx.ins().ushr_imm(result, 7);
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::NEGATIVE, neg);
            Ok(())
        }

        IrOp::Inc(addr) => {
            translate_inc_dec(bcx, bus_ptr, cpu_ptr, *addr, true, read8_ref, write8_ref)
        }
        IrOp::Dec(addr) => {
            translate_inc_dec(bcx, bus_ptr, cpu_ptr, *addr, false, read8_ref, write8_ref)
        }

        IrOp::ShiftLeft(addr) => translate_shift(
            bcx, bus_ptr, cpu_ptr, *addr, true, false, read8_ref, write8_ref,
        ),
        IrOp::ShiftRight(addr) => translate_shift(
            bcx, bus_ptr, cpu_ptr, *addr, false, false, read8_ref, write8_ref,
        ),

        IrOp::Branch { condition, target } => {
            translate_branch(bcx, cpu_ptr, *condition, *target, *total_cycles)
        }

        IrOp::Jump(target) => {
            let result = pack_return(bcx, *target, *total_cycles);
            bcx.ins().return_(&[result]);
            // Create a new unreachable block for any subsequent instructions
            let dead_block = bcx.create_block();
            bcx.switch_to_block(dead_block);
            bcx.seal_block(dead_block);
            Ok(())
        }
        IrOp::JumpIndirect(_ptr) => {
            // 间接跳转：PC=0 回退到解释器，只返回周期数
            bcx.ins().return_(&[*total_cycles]);
            let dead_block = bcx.create_block();
            bcx.switch_to_block(dead_block);
            bcx.seal_block(dead_block);
            Ok(())
        }

        IrOp::Call(target) => {
            let ret_addr = instr_addr.wrapping_add(3);
            let ret_hi = (ret_addr >> 8) as u8;
            let ret_lo = (ret_addr & 0xFF) as u8;

            let sp = load_cpu_u8(bcx, cpu_ptr, cpu_offset::SP);
            let stack_base = bcx.ins().iconst(I16, 0x0100);
            let sp_16 = bcx.ins().uextend(I16, sp);
            let push_addr_hi = bcx.ins().iadd(stack_base, sp_16);
            let ret_hi_val = bcx.ins().iconst(I8, ret_hi as i64);
            call_write8_val(bcx, bus_ptr, push_addr_hi, ret_hi_val, write8_ref);

            let sp_minus_1 = bcx.ins().iadd_imm(sp, -1);
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::SP, sp_minus_1);

            let sp2 = load_cpu_u8(bcx, cpu_ptr, cpu_offset::SP);
            let sp2_16 = bcx.ins().uextend(I16, sp2);
            let push_addr_lo = bcx.ins().iadd(stack_base, sp2_16);
            let ret_lo_val = bcx.ins().iconst(I8, ret_lo as i64);
            call_write8_val(bcx, bus_ptr, push_addr_lo, ret_lo_val, write8_ref);

            let sp_minus_2 = bcx.ins().iadd_imm(sp2, -1);
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::SP, sp_minus_2);

            let result = pack_return(bcx, *target, *total_cycles);
            bcx.ins().return_(&[result]);
            let dead_block = bcx.create_block();
            bcx.switch_to_block(dead_block);
            bcx.seal_block(dead_block);
            Ok(())
        }
        IrOp::Return => {
            // RTS: PC=0 回退到解释器，只返回周期数
            bcx.ins().return_(&[*total_cycles]);
            let dead_block = bcx.create_block();
            bcx.switch_to_block(dead_block);
            bcx.seal_block(dead_block);
            Ok(())
        }

        IrOp::SetFlag { flag, value } => translate_set_flag(
            bcx,
            cpu_ptr,
            bus_ptr,
            *flag,
            *value,
            read8_ref,
            write8_ref,
            total_cycles,
        ),

        IrOp::AdvanceCycles(n) => {
            let cycles_val = bcx.ins().iconst(I32, *n as i64);
            bcx.ins().call(advance_ref, &[bus_ptr, cycles_val]);
            // 累加到 total_cycles
            let n32 = bcx.ins().iconst(I32, *n as i64);
            *total_cycles = bcx.ins().iadd(*total_cycles, n32);
            Ok(())
        }
    }
}

// ── ADC ──

fn translate_adc(
    bcx: &mut FunctionBuilder,
    operand: &Operand,
    bus_ptr: Value,
    cpu_ptr: Value,
    read8_ref: FuncRef,
    _write8_ref: FuncRef,
) -> Result<(), String> {
    let a = load_cpu_u8(bcx, cpu_ptr, cpu_offset::A);
    let val = load_operand(bcx, operand, bus_ptr, cpu_ptr, read8_ref);
    let carry = load_cpu_u8(bcx, cpu_ptr, cpu_offset::CARRY);

    let a_16 = bcx.ins().uextend(I16, a);
    let val_16 = bcx.ins().uextend(I16, val);
    let carry_16 = bcx.ins().uextend(I16, carry);
    let r_16 = bcx.ins().iadd(a_16, val_16);
    let r_16 = bcx.ins().iadd(r_16, carry_16);

    let carry_set = bcx.ins().icmp_imm(IntCC::UnsignedGreaterThan, r_16, 0xFF);
    store_cpu_u8(bcx, cpu_ptr, cpu_offset::CARRY, carry_set);

    let r8 = bcx.ins().ireduce(I8, r_16);

    let a_xor_r = bcx.ins().bxor(a, r8);
    let val_xor_r = bcx.ins().bxor(val, r8);
    let ovf_tmp = bcx.ins().band(a_xor_r, val_xor_r);
    let ovf_bit = bcx.ins().ushr_imm(ovf_tmp, 7);
    store_cpu_u8(bcx, cpu_ptr, cpu_offset::OVERFLOW, ovf_bit);

    store_cpu_u8(bcx, cpu_ptr, cpu_offset::A, r8);
    set_zn(bcx, cpu_ptr, r8);
    Ok(())
}

// ── SBC ──

fn translate_sbc(
    bcx: &mut FunctionBuilder,
    operand: &Operand,
    bus_ptr: Value,
    cpu_ptr: Value,
    read8_ref: FuncRef,
    _write8_ref: FuncRef,
) -> Result<(), String> {
    let a = load_cpu_u8(bcx, cpu_ptr, cpu_offset::A);
    let val = load_operand(bcx, operand, bus_ptr, cpu_ptr, read8_ref);
    let carry = load_cpu_u8(bcx, cpu_ptr, cpu_offset::CARRY);

    let a_16 = bcx.ins().uextend(I16, a);
    let val_16 = bcx.ins().uextend(I16, val);
    let carry_16 = bcx.ins().uextend(I16, carry);

    let r_16 = bcx.ins().isub(a_16, val_16);
    let one = bcx.ins().iconst(I16, 1);
    let r_16 = bcx.ins().isub(r_16, one);
    let r_16 = bcx.ins().iadd(r_16, carry_16);

    let carry_set = bcx.ins().icmp_imm(IntCC::SignedGreaterThanOrEqual, r_16, 0);
    store_cpu_u8(bcx, cpu_ptr, cpu_offset::CARRY, carry_set);

    let r8 = bcx.ins().ireduce(I8, r_16);

    let not_val = bcx.ins().bnot(val);
    let neg_val = bcx.ins().iadd_imm(not_val, 1);
    let a_xor_r = bcx.ins().bxor(a, r8);
    let nv_xor_r = bcx.ins().bxor(neg_val, r8);
    let ovf_tmp = bcx.ins().band(a_xor_r, nv_xor_r);
    let ovf_bit = bcx.ins().ushr_imm(ovf_tmp, 7);
    store_cpu_u8(bcx, cpu_ptr, cpu_offset::OVERFLOW, ovf_bit);

    store_cpu_u8(bcx, cpu_ptr, cpu_offset::A, r8);
    set_zn(bcx, cpu_ptr, r8);
    Ok(())
}

// ── INC/DEC ──

fn translate_inc_dec(
    bcx: &mut FunctionBuilder,
    bus_ptr: Value,
    cpu_ptr: Value,
    addr: u16,
    is_inc: bool,
    read8_ref: FuncRef,
    write8_ref: FuncRef,
) -> Result<(), String> {
    match addr {
        0xFFFF => {
            let reg = load_cpu_u8(bcx, cpu_ptr, cpu_offset::X);
            let result = if is_inc {
                bcx.ins().iadd_imm(reg, 1)
            } else {
                bcx.ins().iadd_imm(reg, -1)
            };
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::X, result);
            set_zn(bcx, cpu_ptr, result);
        }
        0xFFFE => {
            let reg = load_cpu_u8(bcx, cpu_ptr, cpu_offset::Y);
            let result = if is_inc {
                bcx.ins().iadd_imm(reg, 1)
            } else {
                bcx.ins().iadd_imm(reg, -1)
            };
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::Y, result);
            set_zn(bcx, cpu_ptr, result);
        }
        _ => {
            let v = call_read8(bcx, bus_ptr, addr, read8_ref);
            let result = if is_inc {
                bcx.ins().iadd_imm(v, 1)
            } else {
                bcx.ins().iadd_imm(v, -1)
            };
            call_write8(bcx, bus_ptr, addr, result, write8_ref);
            set_zn(bcx, cpu_ptr, result);
        }
    }
    Ok(())
}

// ── Shift ──

fn translate_shift(
    bcx: &mut FunctionBuilder,
    bus_ptr: Value,
    cpu_ptr: Value,
    addr: u16,
    is_left: bool,
    _is_rotate: bool,
    read8_ref: FuncRef,
    write8_ref: FuncRef,
) -> Result<(), String> {
    match addr {
        0xFFFF => {
            let a = load_cpu_u8(bcx, cpu_ptr, cpu_offset::A);
            if is_left {
                let carry = bcx.ins().ushr_imm(a, 7);
                store_cpu_u8(bcx, cpu_ptr, cpu_offset::CARRY, carry);
                let result = bcx.ins().ishl_imm(a, 1);
                store_cpu_u8(bcx, cpu_ptr, cpu_offset::A, result);
                set_zn(bcx, cpu_ptr, result);
            } else {
                let carry = bcx.ins().band_imm(a, 1);
                store_cpu_u8(bcx, cpu_ptr, cpu_offset::CARRY, carry);
                let result = bcx.ins().ushr_imm(a, 1);
                store_cpu_u8(bcx, cpu_ptr, cpu_offset::A, result);
                set_zn(bcx, cpu_ptr, result);
            }
        }
        0xFFFE => {
            let a = load_cpu_u8(bcx, cpu_ptr, cpu_offset::A);
            let carry = load_cpu_u8(bcx, cpu_ptr, cpu_offset::CARRY);
            if is_left {
                let new_carry = bcx.ins().ushr_imm(a, 7);
                store_cpu_u8(bcx, cpu_ptr, cpu_offset::CARRY, new_carry);
                let shifted = bcx.ins().ishl_imm(a, 1);
                let result = bcx.ins().bor(shifted, carry);
                store_cpu_u8(bcx, cpu_ptr, cpu_offset::A, result);
                set_zn(bcx, cpu_ptr, result);
            } else {
                let new_carry = bcx.ins().band_imm(a, 1);
                store_cpu_u8(bcx, cpu_ptr, cpu_offset::CARRY, new_carry);
                let shifted = bcx.ins().ushr_imm(a, 1);
                let carry_7 = bcx.ins().ishl_imm(carry, 7);
                let result = bcx.ins().bor(shifted, carry_7);
                store_cpu_u8(bcx, cpu_ptr, cpu_offset::A, result);
                set_zn(bcx, cpu_ptr, result);
            }
        }
        _ => {
            let v = call_read8(bcx, bus_ptr, addr, read8_ref);
            if is_left {
                let carry = bcx.ins().ushr_imm(v, 7);
                store_cpu_u8(bcx, cpu_ptr, cpu_offset::CARRY, carry);
                let result = bcx.ins().ishl_imm(v, 1);
                call_write8(bcx, bus_ptr, addr, result, write8_ref);
                set_zn(bcx, cpu_ptr, result);
            } else {
                let carry = bcx.ins().band_imm(v, 1);
                store_cpu_u8(bcx, cpu_ptr, cpu_offset::CARRY, carry);
                let result = bcx.ins().ushr_imm(v, 1);
                call_write8(bcx, bus_ptr, addr, result, write8_ref);
                set_zn(bcx, cpu_ptr, result);
            }
        }
    }
    Ok(())
}

// ── Branch ──

/// 分支指令翻译。
///
/// taken 路径：return (target << 16) | total_cycles
/// fallthrough 路径：return 0（PC=0 回退到解释器），周期数由后续块累加
///
/// 注意：Branch 被视为 terminal 指令，调用者应在遇到 Branch 后停止添加指令。
fn translate_branch(
    bcx: &mut FunctionBuilder,
    cpu_ptr: Value,
    condition: BranchCondition,
    target: u16,
    total_cycles: Value,
) -> Result<(), String> {
    let cond_val = match condition {
        BranchCondition::Eq => load_cpu_u8(bcx, cpu_ptr, cpu_offset::ZERO),
        BranchCondition::Ne => {
            let z = load_cpu_u8(bcx, cpu_ptr, cpu_offset::ZERO);
            let one = bcx.ins().iconst(I8, 1);
            bcx.ins().bxor(z, one)
        }
        BranchCondition::Cs => load_cpu_u8(bcx, cpu_ptr, cpu_offset::CARRY),
        BranchCondition::Cc => {
            let c = load_cpu_u8(bcx, cpu_ptr, cpu_offset::CARRY);
            let one = bcx.ins().iconst(I8, 1);
            bcx.ins().bxor(c, one)
        }
        BranchCondition::Mi => load_cpu_u8(bcx, cpu_ptr, cpu_offset::NEGATIVE),
        BranchCondition::Pl => {
            let n = load_cpu_u8(bcx, cpu_ptr, cpu_offset::NEGATIVE);
            let one = bcx.ins().iconst(I8, 1);
            bcx.ins().bxor(n, one)
        }
        BranchCondition::Vs => load_cpu_u8(bcx, cpu_ptr, cpu_offset::OVERFLOW),
        BranchCondition::Vc => {
            let v = load_cpu_u8(bcx, cpu_ptr, cpu_offset::OVERFLOW);
            let one = bcx.ins().iconst(I8, 1);
            bcx.ins().bxor(v, one)
        }
    };

    let cond_bool = bcx.ins().icmp_imm(IntCC::NotEqual, cond_val, 0);

    let taken_block = bcx.create_block();
    let fallthrough_block = bcx.create_block();

    bcx.ins()
        .brif(cond_bool, taken_block, &[], fallthrough_block, &[]);

    bcx.switch_to_block(taken_block);
    bcx.seal_block(taken_block);
    let taken_result = pack_return(bcx, target, total_cycles);
    bcx.ins().return_(&[taken_result]);

    bcx.switch_to_block(fallthrough_block);
    bcx.seal_block(fallthrough_block);
    // fallthrough: 返回 0 PC（由后续块处理），周期数由后续块累加
    let zero_ret = bcx.ins().iconst(I32, 0);
    bcx.ins().return_(&[zero_ret]);

    Ok(())
}

// ── Set flag / register transfer ──

fn translate_set_flag(
    bcx: &mut FunctionBuilder,
    cpu_ptr: Value,
    bus_ptr: Value,
    flag: u8,
    value: bool,
    read8_ref: FuncRef,
    write8_ref: FuncRef,
    total_cycles: &mut Value,
) -> Result<(), String> {
    let _ = value;
    match flag {
        0xAA => {
            let a = load_cpu_u8(bcx, cpu_ptr, cpu_offset::A);
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::X, a);
            set_zn(bcx, cpu_ptr, a);
        }
        0x8A => {
            let x = load_cpu_u8(bcx, cpu_ptr, cpu_offset::X);
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::A, x);
            set_zn(bcx, cpu_ptr, x);
        }
        0xA8 => {
            let a = load_cpu_u8(bcx, cpu_ptr, cpu_offset::A);
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::Y, a);
            set_zn(bcx, cpu_ptr, a);
        }
        0x98 => {
            let y = load_cpu_u8(bcx, cpu_ptr, cpu_offset::Y);
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::A, y);
            set_zn(bcx, cpu_ptr, y);
        }
        0xBA => {
            let sp = load_cpu_u8(bcx, cpu_ptr, cpu_offset::SP);
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::X, sp);
            set_zn(bcx, cpu_ptr, sp);
        }
        0x9A => {
            let x = load_cpu_u8(bcx, cpu_ptr, cpu_offset::X);
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::SP, x);
        }
        0x48 => {
            let a = load_cpu_u8(bcx, cpu_ptr, cpu_offset::A);
            let sp = load_cpu_u8(bcx, cpu_ptr, cpu_offset::SP);
            let stack_base = bcx.ins().iconst(I16, 0x0100);
            let sp_16 = bcx.ins().uextend(I16, sp);
            let push_addr = bcx.ins().iadd(stack_base, sp_16);
            call_write8_val(bcx, bus_ptr, push_addr, a, write8_ref);
            let sp_minus_1 = bcx.ins().iadd_imm(sp, -1);
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::SP, sp_minus_1);
        }
        0x68 => {
            let sp = load_cpu_u8(bcx, cpu_ptr, cpu_offset::SP);
            let sp_plus_1 = bcx.ins().iadd_imm(sp, 1);
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::SP, sp_plus_1);
            let stack_base = bcx.ins().iconst(I16, 0x0100);
            let sp2_16 = bcx.ins().uextend(I16, sp_plus_1);
            let pop_addr = bcx.ins().iadd(stack_base, sp2_16);
            let val = call_read8_val(bcx, bus_ptr, pop_addr, read8_ref);
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::A, val);
            set_zn(bcx, cpu_ptr, val);
        }
        0x08 => {
            let carry = load_cpu_u8(bcx, cpu_ptr, cpu_offset::CARRY);
            let zero = load_cpu_u8(bcx, cpu_ptr, cpu_offset::ZERO);
            let overflow = load_cpu_u8(bcx, cpu_ptr, cpu_offset::OVERFLOW);
            let negative = load_cpu_u8(bcx, cpu_ptr, cpu_offset::NEGATIVE);
            let z1 = bcx.ins().ishl_imm(zero, 1);
            let p = bcx.ins().bor(carry, z1);
            let b20 = bcx.ins().iconst(I8, 0x20);
            let p = bcx.ins().bor(p, b20);
            let o6 = bcx.ins().ishl_imm(overflow, 6);
            let p = bcx.ins().bor(p, o6);
            let n7 = bcx.ins().ishl_imm(negative, 7);
            let p = bcx.ins().bor(p, n7);
            let sp = load_cpu_u8(bcx, cpu_ptr, cpu_offset::SP);
            let stack_base = bcx.ins().iconst(I16, 0x0100);
            let sp_16 = bcx.ins().uextend(I16, sp);
            let push_addr = bcx.ins().iadd(stack_base, sp_16);
            call_write8_val(bcx, bus_ptr, push_addr, p, write8_ref);
            let sp_minus_1 = bcx.ins().iadd_imm(sp, -1);
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::SP, sp_minus_1);
        }
        0x28 => {
            let sp = load_cpu_u8(bcx, cpu_ptr, cpu_offset::SP);
            let sp_plus_1 = bcx.ins().iadd_imm(sp, 1);
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::SP, sp_plus_1);
            let stack_base = bcx.ins().iconst(I16, 0x0100);
            let sp2_16 = bcx.ins().uextend(I16, sp_plus_1);
            let pop_addr = bcx.ins().iadd(stack_base, sp2_16);
            let p = call_read8_val(bcx, bus_ptr, pop_addr, read8_ref);
            let carry = bcx.ins().band_imm(p, 1);
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::CARRY, carry);
            let p2 = bcx.ins().band_imm(p, 2);
            let zero = bcx.ins().ushr_imm(p2, 1);
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::ZERO, zero);
            let p40 = bcx.ins().band_imm(p, 0x40);
            let overflow = bcx.ins().ushr_imm(p40, 6);
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::OVERFLOW, overflow);
            let p80 = bcx.ins().band_imm(p, 0x80);
            let negative = bcx.ins().ushr_imm(p80, 7);
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::NEGATIVE, negative);
        }
        0xFF => {
            // BRK: 返回 0 PC（回退到解释器），携带周期数
            bcx.ins().return_(&[*total_cycles]);
            let dead_block = bcx.create_block();
            bcx.switch_to_block(dead_block);
            bcx.seal_block(dead_block);
        }
        0 => {
            let val = if value {
                bcx.ins().iconst(I8, 1)
            } else {
                bcx.ins().iconst(I8, 0)
            };
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::CARRY, val);
        }
        1 => {
            let val = if value {
                bcx.ins().iconst(I8, 1)
            } else {
                bcx.ins().iconst(I8, 0)
            };
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::INTERRUPT_DISABLE, val);
        }
        2 => {
            let val = if value {
                bcx.ins().iconst(I8, 1)
            } else {
                bcx.ins().iconst(I8, 0)
            };
            store_cpu_u8(bcx, cpu_ptr, cpu_offset::OVERFLOW, val);
        }
        3 => {}
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir_builder::IrBuilder;

    #[test]
    fn test_compile_lda_sta_rts() {
        let mut aot = CraneliftAot::new().unwrap();
        let ir_ops = IrBuilder::lift_block(&[0xA9, 0x42, 0x85, 0x50, 0x60], 0x8000);
        assert_eq!(aot.blocks().len(), 0);
        aot.compile_block(0x8000, &ir_ops).unwrap();
        assert_eq!(aot.blocks().len(), 1);
        let (obj, _, _) = aot.finish().unwrap();
        assert!(!obj.is_empty());
    }

    #[test]
    fn test_compile_jmp() {
        let mut aot = CraneliftAot::new().unwrap();
        let ir_ops = IrBuilder::lift_block(&[0x4C, 0x00, 0x80], 0x8000);
        aot.compile_block(0x8000, &ir_ops).unwrap();
        let (obj, _, _) = aot.finish().unwrap();
        assert!(!obj.is_empty());
    }

    #[test]
    fn test_compile_multiple_blocks() {
        let mut aot = CraneliftAot::new().unwrap();
        let ir_ops1 = IrBuilder::lift_block(&[0xA9, 0x01, 0x4C, 0x10, 0x80], 0x8000);
        aot.compile_block(0x8000, &ir_ops1).unwrap();
        let ir_ops2 = IrBuilder::lift_block(&[0x85, 0x50, 0x60], 0x8010);
        aot.compile_block(0x8010, &ir_ops2).unwrap();
        assert_eq!(aot.blocks().len(), 2);
        let (obj, _, _) = aot.finish().unwrap();
        assert!(!obj.is_empty());
    }
}
