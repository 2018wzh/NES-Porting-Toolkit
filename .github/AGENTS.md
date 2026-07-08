# NES Porting Toolkit — Agent Guidelines

## 项目简介

NES 游戏静态重编译框架。将 6502 机器码通过 Cranelift AOT 编译为原生代码，逐步用 WGPU/CPAL/原生输入替换 PPU/APU/控制器，生成独立原生可执行文件。

## 构建与测试

```sh
cargo build                    # 编译所有 workspace crate
cargo build --release          # Release 构建（含 Cranelift AOT）
cargo test --workspace         # 运行所有测试（当前 149 个）
cargo clippy                   # Lint 检查
NES_AOT=0 cargo build          # 跳过 Cranelift AOT 编译，加快迭代
```

运行 Battle City:
```sh
cargo run --release --bin nes-port -- run --rom roms/BattleCity.nes --profile profiles/battle_city/profile.toml --mode compat-interpreter
```

## 架构概览

| Crate | 职责 |
|---|---|
| `nptk-core` | NES 核心仿真：ROM、Mapper0、6502 CPU、PPU、APU、Bus、System |
| `nptk-profile` | 游戏配置：TOML profile、符号表、钩子、校验 |
| `nptk-recompiler` | 静态重编译：反汇编 → CFG → IR6502 → Cranelift AOT |
| `nptk-native-runtime` | 原生运行时 ABI：`NesRuntime` trait、桥接层 |
| `nptk-wgpu` | WGPU 渲染器（Framebuffer + Native tilemap/sprite） |
| `nptk-audio` | 音频系统：CPAL 输出、APU 混音、Kira 引擎 |
| `nptk-input` | 可插拔输入：`InputBackend` trait、回放、热插拔 |
| `nptk-tools` | CLI 工具（7 个子命令） |
| `games/battle-city` | 默认游戏 GUI 应用（winit + wgpu + egui） |

三种运行模式：`compat-interpreter` | `recompiled-compat` | `native-port`

详见 [docs/IMPLEMENTATION_PLAN.md](docs/IMPLEMENTATION_PLAN.md) 和 [docs/plan.md](docs/plan.md)。

## 编码规范

- **Rust edition 2024**，需要 Rust 1.85+
- **Crate 命名**: `nptk-*` 前缀
- **注释**: 中文为主（`///` 文档注释、`//!` crate 级注释）
- **错误处理**: `Box<dyn std::error::Error>`（CLI 主函数），`Result<_, String>` 或 `Option<T>`（内部函数）。不使用 `thiserror`/`anyhow`
- **`unsafe`**: 仅限 ABI 边界（Cranelift 机器码调用、WGPU surface 生命周期）。不引入新的 `unsafe`
- **`// ponytail:` 标记**: 表示"已知临时方案"，是作者留下的 TODO
- **`#[allow(dead_code)]`**: APU/PPU 中有部分未使用字段，是部分实现的遗留

## 关键约定

1. **`NesRuntime` trait 是核心 ABI** — 修改需同步更新 `CompatRuntime`、`RecompiledRuntime` 和 Cranelift codegen
2. **`build.rs` 自动 AOT 编译** — 读取 ROM → 发现基本块 → Cranelift 编译 → `.o` → `.a` 静态链接。ROM 不存在时静默跳过（仅 warning）
3. **输入链路**: 物理设备 → `InputBackend` → `RawGamepadState` → `InputMapper` → `CanonicalGamepadState` → `canonical_to_nes_port()` → `NesControllerState` → `NesControllerPort` → Bus
4. **PPU 渲染双路径**: `WgpuRenderer` 支持 `Framebuffer`（上传兼容帧缓冲）和 `Native`（tilemap + sprite 实例化）两种模式
5. **测试 ROM 是合成的** — 单元测试使用 `make_rom()` 构造最小 ROM，不依赖外部文件

## 文档索引

| 文档 | 内容 |
|---|---|
| [docs/RECOMPILER.md](docs/RECOMPILER.md) | 重编译器 Pipeline、IR 操作码、ABI 规范 |
| [docs/RUNTIME_ABI.md](docs/RUNTIME_ABI.md) | `NesRuntime` trait 完整规范 |
| [docs/PROFILE_FORMAT.md](docs/PROFILE_FORMAT.md) | GameProfile TOML/RON 格式 |
| [docs/INPUT_BACKENDS.md](docs/INPUT_BACKENDS.md) | 输入系统架构与后端实现 |
| [docs/mapper.md](docs/mapper.md) | 卡带芯片接口设计（中文） |
| [docs/LEGAL.md](docs/LEGAL.md) | 许可证与法律信息 |