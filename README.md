# NES Porting Toolkit (nptk)

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)

NES Porting Toolkit 是一个 Rust 编写的 NES/Famicom 游戏静态重编译框架。它不走传统模拟器逐条解释 6502 指令的路子，而是把 6502 机器码静态分析、提升为中间表示，再通过 Cranelift AOT 编译生成本地机器码，最终把 NES 游戏编译成原生可执行文件。

## 核心特性

- **静态重编译** — 一条流水线走到底：反汇编 → CFG → IR6502 → Cranelift IR → 本地机器码
- **双执行模式** — `compat-interpreter`（纯 6502 解释器，做正确性基线）和 `recompiled-compat`（Cranelift AOT 原生代码 + 解释器回退）
- **渐进式替换** — CPU 执行、PPU 渲染、APU 音频可以独立替换成原生实现，不用一次全改
- **WGPU 渲染** — 兼容模式走 framebuffer 上传，原生模式走 tilemap/sprite 直接渲染，带 WGSL shader
- **可插拔输入** — winit 键盘、gilrs 手柄、通用 HID、XInput，支持 replay 录制和回放
- **CPAL/Kira 音频** — APU 兼容混音 + PCM 输出，预留原生 SFX/BGM 事件接口
- **GameProfile 系统** — 用 TOML/RON 描述游戏的 ROM 特征、符号表、hook、输入映射和测试配置
- **egui 调试 UI** — 运行时 CPU/PPU/RAM 查看器、输入映射编辑器
- **CLI 工具链** — 7 个子命令：`inspect` / `run` / `trace` / `recompile` / `dump-chr` / `golden` / `input-test`

## 架构概览

```
                         ┌───────────────────────┐
                         │       User ROM         │
                         └──────────┬────────────┘
                                    │
                                    ▼
 ┌──────────────────────────────────────────────────────────────────┐
 │                         nptk-core                                │
 │  ROM Parser │ Mapper │ NesBus │ CPU Ref │ PPU Compat │ APU Compat│
 └──────────────────────────┬───────────────────────────────────────┘
                            │
                            ▼
 ┌──────────────────────────────────────────────────────────────────┐
 │                       nptk-profile                                │
 │   GameProfile │ Symbols │ RAM Map │ Hooks │ Test Config           │
 └──────────────────────────┬───────────────────────────────────────┘
                            │
                            ▼
 ┌──────────────────────────────────────────────────────────────────┐
 │                     nptk-recompiler                               │
 │   Disasm │ CFG │ IR6502 │ Analysis │ Cranelift Codegen │ Manifest │
 └──────────────────────────┬───────────────────────────────────────┘
                            │
                            ▼
 ┌──────────────────────────────────────────────────────────────────┐
 │                  nptk-native-runtime                              │
 │   NesRuntime ABI │ PPU Events │ Audio Events │ Input Bridge       │
 └──────────┬───────────────┬──────────────┬────────────────────────┘
            │               │              │
            ▼               ▼              ▼
 ┌──────────────┐  ┌──────────────┐  ┌──────────────┐
 │   nptk-wgpu   │  │  nptk-audio  │  │  nptk-input   │
 │   Renderer    │  │  CPAL/Kira   │  │   Backends    │
 └──────────────┘  └──────────────┘  └──────────────┘
            │               │              │
            └───────────────┼──────────────┘
                            │
                            ▼
 ┌──────────────────────────────────────────────────────────────────┐
 │                    nptk-battle-city (Game App)                    │
 │         winit + wgpu + egui + CPAL + gilrs 完整集成              │
 └──────────────────────────────────────────────────────────────────┘
```

## 快速开始

### 前置要求

- Rust 工具链（edition 2024）
- 一个合法的 NES ROM 文件（比如 Battle City）

### 构建

```bash
# 构建所有 crate
cargo build --release

# 跑所有测试
cargo test --workspace
```

### 运行 Battle City

```bash
# 兼容模式（6502 解释器 + PPU/APU 兼容层）
cargo run --release --bin nptk-port -- run \
    --rom roms/BattleCity\ \(Japan\).nes \
    --profile profiles/battle_city/profile.toml \
    --mode compat-interpreter

# 重编译模式（Cranelift AOT 原生代码 + 兼容运行时）
cargo run --release --bin nptk-port -- run \
    --rom roms/BattleCity\ \(Japan\).nes \
    --profile profiles/battle_city/profile.toml \
    --mode recompiled-compat
```

### CLI 命令

| 命令 | 作用 |
|---|---|
| `nptk-port inspect --rom <FILE>` | 检查 ROM 元数据和 GameProfile |
| `nptk-port run --rom <FILE> --profile <FILE>` | 运行游戏 |
| `nptk-port trace --rom <FILE>` | 记录 CPU 执行 trace |
| `nptk-port recompile --rom <FILE>` | 静态重编译 |
| `nptk-port dump-chr --rom <FILE>` | 导出 CHR tile atlas 为 PNG |
| `nptk-port golden --rom <FILE>` | 运行 golden frame 测试 |
| `nptk-port input-test` | 测试输入后端 |

### 运行 GUI 应用

```bash
cargo run --release --bin battle-city
```

## 运行模式

| 模式 | 用途 | 说明 |
|---|---|---|
| `compat-interpreter` | 正确性基线 | 6502 解释器 + PPU/APU 兼容层 |
| `recompiled-compat` | 重编译验证 | 静态重编译 6502 + 兼容运行时 |
| `native-port` | 原生移植目标 | 重编译逻辑 + WGPU 渲染 + 原生音频/输入 |

## 项目结构

```
├── Cargo.toml                  # 工作空间根
├── crates/
│   ├── nptk-core/              # NES 核心：ROM、Mapper、Bus、CPU、PPU、APU
│   ├── nptk-profile/           # GameProfile 定义、符号表、hook 配置
│   ├── nptk-recompiler/        # 6502 静态重编译流水线（Cranelift AOT）
│   ├── nptk-native-runtime/    # 原生运行时 ABI 与桥接层
│   ├── nptk-wgpu/              # WGPU 渲染器（framebuffer + native tilemap/sprite）
│   ├── nptk-audio/             # 音频系统（CPAL PCM + Kira 原生）
│   ├── nptk-input/             # 可插拔输入系统（键盘、手柄、HID、replay）
│   ├── nptk-tools/             # CLI 工具（nptk-port 二进制入口）
│   └── nptk-mapper/            # Mapper 聚合 crate（重导出 + linkme 自动注册）
├── mappers/
│   ├── nrom/                   # NROM (Mapper 0) 实现
│   ├── uxrom/                  # UxROM (Mapper 2) 实现
│   └── cnrom/                  # CNROM (Mapper 3) 实现
├── games/
│   └── battle-city/            # Battle City 完整 GUI 应用
├── profiles/
│   ├── battle_city/            # Battle City GameProfile
│   └── donkey_kong/            # Donkey Kong GameProfile（示例）
├── roms/                       # ROM 文件目录（用户自行提供）
└── docs/                       # 详细文档
    ├── DEVELOPMENT_GUIDE.md    # 开发适配指南（适配新游戏）
    ├── IMPLEMENTATION_PLAN.md  # 实现计划与状态
    ├── PROFILE_FORMAT.md       # GameProfile 格式参考
    ├── RUNTIME_ABI.md          # NesRuntime ABI 规范
    ├── RECOMPILER.md           # 重编译器设计
    ├── INPUT_BACKENDS.md       # 输入后端文档
    ├── MAPPER_IMPLEMENTATION.md# Mapper 实现指南
    └── LEGAL.md                # 许可与法律信息
```

## 技术栈

| 领域 | 技术 |
|---|---|
| 语言 | Rust (edition 2024) |
| AOT 编译 | Cranelift（IR → 本地机器码 → 静态链接） |
| 渲染 | WGPU + WGSL shader |
| 窗口/事件 | winit |
| 调试 UI | egui |
| 音频输出 | CPAL (PCM) / Kira（原生音频事件） |
| 输入 | winit 键盘、gilrs 手柄、HIDAPI、XInput |
| 序列化 | TOML / RON / serde |

## 当前状态

所有核心 crate 功能完备，149 个测试全部通过。Cranelift AOT 编译流水线已集成到 `build.rs`，构建时自动把 6502 基本块编译成本地机器码，静态链接到最终二进制。

| Crate | 状态 | 说明 |
|---|---|---|
| `nptk-core` | ✅ 功能完备 | ROM 解析、Mapper 0/2/3、Bus、6502 CPU、PPU、APU、控制器 |
| `nptk-profile` | ✅ 功能完备 | GameProfile 加载/序列化、符号表、hook、验证 |
| `nptk-recompiler` | ✅ 功能完备 | 反汇编、CFG、IR6502、分析、Cranelift AOT codegen、manifest |
| `nptk-native-runtime` | ✅ 功能完备 | NesRuntime ABI、PPU/Audio/Input/State 桥接 |
| `nptk-wgpu` | ✅ 功能完备 | Framebuffer + native tilemap/sprite 渲染、调色板、调试 UI |
| `nptk-audio` | ✅ 功能完备 | CPAL 输出、APU 混音、Kira 引擎骨架 |
| `nptk-input` | ✅ 功能完备 | 多后端、canonical 状态、replay、热插拔 |
| `nptk-tools` | ✅ 功能完备 | 7 子命令 CLI、配置管理 |
| `nptk-battle-city` | ✅ 功能完备 | 完整 GUI 应用（winit + wgpu + egui） |

---

# 法律与许可

## 项目许可

本项目采用 **MIT** 或 **Apache License 2.0** 双许可，详见 [`docs/LEGAL.md`](docs/LEGAL.md)。

## ROM 版权

**本项目不分发任何商业 ROM、CHR 转储、原始音频或其他受版权保护的游戏资产。** 用户必须自行提供合法获取的 ROM 文件。详见 [`docs/LEGAL.md`](docs/LEGAL.md)。