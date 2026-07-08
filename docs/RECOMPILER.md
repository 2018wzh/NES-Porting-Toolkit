# 6502 Static Recompiler Design

## Overview

The `nes-recompiler` crate transforms NES 6502 machine code into native
executables via a multi-stage pipeline:

```
ROM (PRG-ROM bytes)
    |
    v
Disassembly  (disasm6502 crate)
    |
    v
CFG Construction  (control flow graph: basic blocks + edges)
    |
    v
IR6502 Lowering  (6502 semantics to IR opcodes)
    |
    v
Cranelift AOT Codegen  (IR → Cranelift IR → native .o → .dll)
```

Two codegen backends are available:
- **`codegen_cranelift`** (default) — IR6502 → Cranelift IR → native machine code → .dll
- **`codegen_rust`** (deprecated) — IR6502 → Rust source → rustc

## Pipeline Stages

### Stage 1: ROM Extraction

The recompiler reads PRG-ROM from a parsed `NesRom` structure. In Mapper 0
(NROM), PRG-ROM occupies `$8000-$FFFF` in CPU address space (16 KiB) or
`$8000-$FFFF` with the second bank at `$C000-$FFFF` (32 KiB).

### Stage 2: Disassembly

The `disasm.rs` module wraps the `disasm6502` crate to produce a linear
disassembly of PRG-ROM bytes. Each instruction is decoded into its mnemonic,
addressing mode, operand bytes, and cycle count.

```rust
pub fn disassemble(data: &[u8], start_address: u16) -> Result<Vec<String>, String>
```

This produces a flat list of instruction strings. For the recompiler's purposes,
the structured instruction data from `disasm6502` is used directly for CFG
construction rather than parsing the strings.

### Stage 3: CFG Construction

The `cfg.rs` module builds a control flow graph from the disassembled
instructions. Each basic block is a linear sequence of instructions with a
single entry point and a single exit (branch, jump, return, or fall-through).

**Block discovery algorithm:**

1. Start from known entry points: reset vector, NMI vector, IRQ/BRK vector, and
   any explicitly annotated function entries from the GameProfile hooks.
2. For each entry point, trace forward through the instruction stream:
   - Regular instructions extend the current block.
   - Unconditional branches (`JMP`, `RTS`, `RTI`) terminate the block and create
     a successor edge (or mark the block as a leaf for `RTS`/`RTI`).
   - Conditional branches (`BCC`, `BCS`, `BEQ`, `BMI`, `BNE`, `BPL`, `BVC`,
     `BVS`) create two successors: the taken target and the fall-through address.
   - `JSR` creates a call edge to the target subroutine and a fall-through edge
     for the return point.
   - Indirect jumps (`JMP (addr)`) are deferred to the indirect jump dispatch
     strategy (see below).
3. Repeat until no new blocks are discovered.

```rust
pub struct BasicBlock {
    pub id: BlockId,
    pub start: u16,
    pub end: u16,
    pub successors: Vec<BlockId>,
    pub predecessors: Vec<BlockId>,
}

pub struct Cfg {
    pub blocks: HashMap<BlockId, BasicBlock>,
    pub entry: BlockId,
}
```

### Stage 4: IR6502 Lowering

The `ir6502.rs` module defines an intermediate representation that captures
6502 semantics in a form suitable for code generation.

#### IR Opcode Reference

| IR Opcode | Description |
|---|---|
| `Nop` | No operation |
| `LoadA(Operand)` | Load accumulator from operand |
| `LoadX(Operand)` | Load X register from operand |
| `LoadY(Operand)` | Load Y register from operand |
| `StoreA(u16)` | Store accumulator to address |
| `StoreX(u16)` | Store X register to address |
| `StoreY(u16)` | Store Y register to address |
| `AddWithCarry(Operand)` | A = A + operand + carry |
| `SubWithCarry(Operand)` | A = A - operand - (1 - carry) |
| `And(Operand)` | A = A & operand |
| `Or(Operand)` | A = A \| operand |
| `Xor(Operand)` | A = A ^ operand |
| `Compare(Operand)` | Compare A with operand (sets flags) |
| `Inc(u16)` | Increment memory at address |
| `Dec(u16)` | Decrement memory at address |
| `ShiftLeft(u16)` | Arithmetic shift left (ASL) at address |
| `ShiftRight(u16)` | Logical shift right (LSR) at address |
| `Branch { condition, target }` | Conditional branch to label |
| `Jump(Label)` | Unconditional jump to label |
| `JumpIndirect(u16)` | Indirect jump via address |
| `Call(Label)` | Subroutine call to label |
| `Return` | Return from subroutine |
| `SetFlag { flag, value }` | Set a CPU status flag directly |
| `AdvanceCycles(u8)` | Advance CPU cycle counter (for MMIO side effects) |

#### Addressing Modes

6502 addressing modes are resolved during IR lowering and translated into
concrete `Operand` variants:

| Operand Variant | 6502 Addressing Mode | Example |
|---|---|---|
| `Immediate(u8)` | Immediate (`#$nn`) | `LDA #$42` |
| `Address(u16)` | Implied by opcode | Accumulator ops |
| `Zeropage(u8)` | Zero Page (`$nn`) | `LDA $50` |
| `ZeropageX(u8)` | Zero Page,X (`$nn,X`) | `LDA $50,X` |
| `ZeropageY(u8)` | Zero Page,Y (`$nn,Y`) | `LDX $50,Y` |
| `Absolute(u16)` | Absolute (`$nnnn`) | `LDA $2000` |
| `AbsoluteX(u16)` | Absolute,X (`$nnnn,X`) | `LDA $8000,X` |
| `AbsoluteY(u16)` | Absolute,Y (`$nnnn,Y`) | `LDA $8000,Y` |
| `IndirectX(u8)` | (Zero Page,X) (`($nn,X)`) | `LDA ($50,X)` |
| `IndirectY(u8)` | (Zero Page),Y (`($nn),Y`) | `LDA ($50),Y` |

#### Handling Strategy

The recompiler resolves each addressing mode at IR lowering time:

- **Absolute, Zero Page, Immediate**: Trivial -- the operand is statically known.
- **Indexed modes (X, Y)**: The index register value is loaded at runtime and
  added to the base address. Overflow/wrapping follows 6502 semantics (no page
  crossing penalty in IR, but cycle accounting is preserved).
- **Indirect modes**: The effective address is computed via two zero-page reads.
  The generated code emits loads from the NesRuntime (which wraps the NesBus) rather than
  raw memory access, to preserve MMIO side effects.
- **Page-crossing cycle penalty**: Tracked via `AdvanceCycles` but otherwise
  transparent to the generated code.

### Stage 5: Cranelift AOT Codegen (Default)

The `codegen_cranelift.rs` module performs ahead-of-time compilation of 6502
machine code into native machine code via Cranelift IR, outputting a `.o`
object file that can be linked into a `.dll` dynamic library.

```rust
pub struct CraneliftAot {
    module: ObjectModule,
    builder_ctx: FunctionBuilderContext,
    ctx: Context,
    blocks: Vec<CompiledBlock>,
    block_names: HashMap<u16, String>,
}
```

**ABI:**

Each 6502 basic block is compiled as an `extern "C"` function:

```c
uint16_t block_XXXX(NesBusImpl* bus, NativeCpuState* cpu);
```

- `bus`: NES 总线指针，通过 `nes_read8`/`nes_write8` 外部函数访问内存
- `cpu`: CPU 状态指针，通过 load/store 直接访问字段
- 返回值: 下一个 PC 地址 (0 = 回退到解释器)

**Memory access:** Cranelift-generated code calls `nes_read8`/`nes_write8`
via `extern "C"` function calls, avoiding vtable overhead.

**Build integration:** The `games/battle-city/build.rs` script automatically:
1. Reads the ROM file
2. Discovers basic blocks via BFS
3. Compiles each block through Cranelift
4. Outputs a `.dll` to `target/{profile}/`
5. Generates Rust bindings for runtime loading

### Stage 5 (alt): AOT Rust Codegen (Deprecated)

The `codegen_rust.rs` module is the original AOT backend that generates Rust
source code instead of Cranelift IR. It is kept as a reference implementation.

```rust
pub struct RustCodegen {
    blocks: Vec<GeneratedBlock>,
    next_id: u32,
}
```

### Runtime Loading

The `RecompiledRuntime` in `nes-native-runtime` supports two dispatch tables:
- `dispatch: HashMap<u16, NativeBlockFn>` — Rust ABI blocks (from `codegen_rust`)
- `cabi_dispatch: HashMap<u16, CAbiBlockFn>` — C ABI blocks (from `codegen_cranelift`)

C ABI blocks use raw pointers (`*mut NesBusImpl`, `*mut NativeCpuState`)
instead of trait objects, allowing Cranelift-generated code to call into
the NES bus directly.

### Indirect Jump Dispatch Strategy

Indirect jumps (`JMP (addr)`, `RTS` pulling a computed PC from stack) are the
primary challenge in static recompilation of 6502 code. The 6502 has no
distinction between code and data pointers, and indirect jump targets often
depend on runtime state (game mode, menu selection, etc.).

**Strategy: Hybrid dispatcher**

1. **Statically known targets**: When static analysis can determine all possible
   targets (e.g., a jump table with known entries from a `JumpTable` hook), the
   indirect jump is compiled as a switch/case over the address.
2. **Runtime dispatcher**: For unresolved indirect jumps, the generated code
   returns 0, causing the runtime dispatcher to fall back to the interpreter
   for that block.
3. **Profile-guided optimisation** (future): At runtime, track which indirect
   jump targets are hot and recompile with those targets inlined as a
   fast-path switch.

The `unknown_indirect_jump` field in `[cpu]` controls behaviour for jumps
that cannot be resolved statically:

- `"dispatcher"`: Generate a runtime dispatcher call (default for recompilation).
- `"trap"`: Abort with an error (useful for debugging / verifying coverage).

## IR6502 Block Structure

```rust
pub struct IrBlock {
    pub start: u16,                    // 6502 start address
    pub ops: Vec<IrOp>,                // body instructions
    pub terminator: Option<IrOp>,      // block terminator (branch/jump/return)
}
```

A block's `terminator` is one of: `Branch`, `Jump`, `JumpIndirect`, `Call`, or
`Return`. Blocks that fall through to the next block in address order have
`termininator: None` and rely on the linker to chain them.

## Manifest Output

The recompiler produces a `Manifest` describing the generated code:

```rust
pub struct Manifest {
    pub blocks: Vec<ManifestBlock>,
}

pub struct ManifestBlock {
    pub address: u16,
    pub cycles: u32,
}
```

## Current Limitations

1. **Mapper 0 only**: Only NROM is supported. Other mappers (MMC1, MMC3, etc.)
   require mapper-specific bus implementations.
2. **Static analysis is minimal**: The `analysis.rs` module uses simplistic
   heuristics. Full control flow recovery with indirect jump resolution
   requires more sophisticated analysis (value-set analysis, abstract
   interpretation).
3. **No self-modifying code support**: The recompiler assumes PRG-ROM is
   read-only. Games that write to PRG-ROM (mapper-controlled bankswitching
   excepted) are not supported.

## See Also

- [RUNTIME_ABI.md](RUNTIME_ABI.md) -- the ABI between recompiled code and the runtime
- [PROFILE_FORMAT.md](PROFILE_FORMAT.md) -- hook and symbol configuration
- [IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md) -- overall project status
