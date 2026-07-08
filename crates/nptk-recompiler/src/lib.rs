//! nptk-recompiler: 6502 静态重编译器
//!
//! Pipeline: disasm → CFG → IR6502 → Cranelift AOT codegen → .dll
//!
//! Two execution modes:
//! - compat-interpreter (6502 emulation)
//! - recompiled-compat (Cranelift AOT native code + interpreter fallback)
//!
//! Single codegen backend:
//! - `codegen` — IR6502 → Cranelift IR → native

pub mod analysis;
pub mod aot_link;
pub mod cfg;
pub mod codegen;
pub mod disasm;
pub mod ir6502;
pub mod ir_builder;
pub mod manifest;
