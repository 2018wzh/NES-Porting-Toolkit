# NES Porting Toolkit -- Implementation Plan

## Project Overview

The **NES Porting Toolkit** is a Rust-based framework for statically recompiling
NES/Famicom games into native executables. Instead of traditional emulation, the
toolkit analyses 6502 machine code, lifts it to an intermediate representation,
and compiles it to native code via AOT Rust codegen. The PPU and APU can operate in
compatibility mode (faithful software emulation) or be progressively replaced
with native WGPU rendering and CPAL/Kira audio.

**Tech stack:** Rust, WGPU, AOT codegen, CPAL, Kira, winit, egui.

The default game profile targets **Battle City (坦克大战)** on Mapper 0 / NROM, but
the framework is designed to be game-agnostic and extensible.

## Current Status

| Crate | Status | Description |
|---|---|---|
| `nes-core` | Functional | ROM parsing (iNES / NES 2.0), Mapper 0 / NROM, NesBus, 6502 CPU reference interpreter, PPU compatibility renderer, APU compatibility, NES controller shift-register emulation, NES system frame loop |
| `nes-profile` | Functional | `GameProfile` TOML loading/serialization, `SymbolTable` (RAM symbols, function entry points, data tables), `HookConfig` (code region annotations), profile validation |
| `nes-recompiler` | Functional | Disassembly wrapper (`disasm6502`), CFG data structures, IR6502 opcode definitions, basic code analysis, manifest generation, **Cranelift AOT codegen** (IR6502 → Cranelift IR → native .dll), deprecated Rust codegen backend |
| `nes-native-runtime` | Functional | `NesRuntime` trait, `CompatRuntime` (NesBus-backed), `PpuBridge`, `AudioBridge`, `InputBridge`, `StateBridge` |
| `nes-wgpu` | Functional | WGPU renderer with framebuffer and native tilemap/sprite modes, CHR texture atlas, WGSL shaders, egui debug overlay (CPU/PPU/RAM viewer, input mapping editor) |
| `nes-audio` | Functional | CPAL output stream, APU mixer (NES DAC formula), Kira event types (SfxId/BgmId/NativeAudioEvent) |
| `nes-input` | Functional | Backend trait, canonical gamepad state, input mapper, NES controller mapping, replay backend, hotplug manager |
| `nes-tools` | Functional | CLI with `inspect` / `run` / `trace` / `recompile` / `dump-chr` / `golden` / `input-test` subcommands |

**Overall:** Phase 1-2 (core emulation, profile system, basic tooling) is functional.
Phase 3-4 (WGPU native rendering, native audio) is in active development.

## Architecture

```
                         +---------------------------+
                         |        User ROM            |
                         +-------------+-------------+
                                       |
                                       v
+-----------------------------------------------------------------+
|                         nes-core                                  |
|  ROM Parser | Mapper | NesBus | CPU Ref | PPU Compat | APU Compat |
+-------------+---------------------------------------------------+
              |
              v
+-----------------------------------------------------------------+
|                       nes-profile                                 |
|   GameProfile | Symbols | RAM Map | Hooks | Test Config           |
+-------------+---------------------------------------------------+
              |
              v
+-----------------------------------------------------------------+
|                     nes-recompiler                                |
|   Disasm | CFG | IR6502 | Analysis | Cranelift Codegen | Manifest |
|   (deprecated: Rust Codegen)                                      |
+-------------+---------------------------------------------------+
              |
              v
+-----------------------------------------------------------------+
|                  nes-native-runtime                               |
|   NesRuntime ABI | PPU Events | Audio Events | Input Bridge       |
+-------------+---------------------------------------------------+
              |
       +------+------------+-------------+
       v      v            v             v
+-----------+ +----------+ +-----------+ +---------------------+
| nes-wgpu   | | nes-audio | | nes-input  | | nes-battle-city      |
| Renderer  | | CPAL/Kira| | Backends  | | Default Game Profile |
+-----------+ +----------+ +-----------+ +---------------------+
```

## Run Modes

| Mode | Purpose | Description |
|---|---|---|
| `compat-interpreter` | Correctness baseline | 6502 interpreter + PPU/APU compatibility layer |
| `recompiled-compat` | Recompilation validation | Statically recompiled 6502 + compatibility runtime |
| `native-port` | Native port target | Recompiled logic + WGPU rendering + native audio/input |

## Crate Dependency Graph

```
nes-core         (foundation: ROM, Mapper, Bus, CPU, PPU, APU, Controller)
  ^
  |
nes-profile      (game-specific metadata, symbols, hooks)
  ^
  |
nes-recompiler   (6502 static recompilation pipeline)
  ^
  |
nes-native-runtime (NesRuntime ABI, runtime bridges)
  ^         ^         ^
  |         |         |
nes-wgpu   nes-audio  nes-input    (frontend implementations)
              ^
              |
         nes-tools                  (CLI entry point)
```

## CLI Commands Reference

| Command | Description |
|---|---|
| `nes-port inspect --rom <FILE> [--profile <FILE>]` | Inspect ROM metadata and GameProfile |
| `nes-port run --rom <FILE> [--profile <FILE>] [--mode <MODE>]` | Run game in specified mode |
| `nes-port trace --rom <FILE> [--profile <FILE>] [--input <FILE>]` | Record CPU execution trace |
| `nes-port recompile --rom <FILE> [--profile <FILE>] [--out <DIR>]` | Static recompilation |
| `nes-port dump-chr --rom <FILE> [--out <FILE>]` | Export CHR tile atlas as PNG |
| `nes-port golden --rom <FILE> [--profile <FILE>] [--input <FILE>]` | Run golden frame tests |
| `nes-port input-test [--backend <BACKEND>] [--record <FILE>] [--mapping-wizard <FILE>]` | Test input backends |

## Build and Run

```bash
# Build everything
cargo build --release

# Run Battle City in compatibility mode
cargo run --release --bin nes-port -- run --rom roms/battle_city.nes \
    --profile profiles/battle_city/profile.toml --mode compat-interpreter

# Dump CHR tiles
cargo run --release --bin nes-port -- dump-chr --rom roms/battle_city.nes

# Inspect a ROM
cargo run --release --bin nes-port -- inspect --rom roms/battle_city.nes \
    --profile profiles/battle_city/profile.toml

# Run tests
cargo test --workspace
```

## Development Strategy

```
Correctness first
  -> Replace CPU execution (recompiler)
  -> Replace rendering (WGPU)
  -> Replace audio (CPAL / Kira)
  -> Semantic game state extraction
  -> Generalize to more NES/Famicom games
```

## Upcoming Phases

1. **Phase 3 -- AOT codegen improvements**: Extend `codegen_rust.rs` with missing opcodes and addressing modes (absolute,X/Y, indirect, zeropage indexed).
2. **Phase 4 -- WGPU native rendering polish**: Verify `TilemapPass`, `SpritePass` correctness against compat framebuffer output.
3. **Phase 5 -- Native audio**: Wire `ApuMixer` to `CpalOutput` and implement Kira native SFX/BGM hooks.
4. **Phase 6 -- Game state extraction**: Semantic hooks beyond basic RAM symbols.
