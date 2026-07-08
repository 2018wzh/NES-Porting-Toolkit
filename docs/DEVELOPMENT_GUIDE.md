# Toolkit 开发适配指南

本文档面向想用本工具链适配新 NES 游戏的开发者。读完你应该知道：从拿到一个 ROM 到跑起来，中间要经过哪些步骤，每个步骤需要你做什么。

## 整体流程

适配一个新游戏大致分六步：

1. **建 GameProfile** — 告诉工具链这个游戏的基本信息
2. **标符号表** — 把游戏里关键函数和数据的地址标出来
3. **配 hook** — 标注代码区域的性质（函数入口、数据表等）
4. **配输入映射** — 把 NES 按键映射到 PC 键盘/手柄
5. **创建游戏 crate** — 基于 `nptk` facade crate 编写游戏入口
6. **验证** — 跑 golden test 确认重编译结果正确

## 架构概览

```
games/my-game/              ← 游戏 crate（只含游戏特定逻辑）
  ├── Cargo.toml            ← 只依赖 nptk + cc
  ├── build.rs              ← AOT 编译（使用 nptk::build）
  └── src/
      ├── main.rs           ← 实现 GameHandlers trait
      ├── lib.rs            ← 导出游戏模块
      ├── game_runtime.rs   ← NES 运行时封装
      ├── game_state.rs     ← 语义化游戏状态
      └── native_hooks.rs   ← Hook 注册

crates/nptk/                ← Facade crate（统一入口）
  ├── nptk-app              ← 窗口/事件循环/帧循环框架
  ├── nptk-core             ← NES 模拟核心
  ├── nptk-native-runtime   ← 运行时 ABI
  ├── nptk-wgpu             ← WGPU 渲染器
  ├── nptk-audio            ← 音频系统
  ├── nptk-input            ← 输入系统
  └── nptk-profile          ← 游戏配置
```

游戏 crate **只需依赖 `nptk` 一个 crate**（运行时）+ `cc`（构建时链接 `.o`），所有平台细节（窗口、渲染、音频、输入、调试 UI）都由 `nptk` 封装。

## 第一步：建 GameProfile

在 `profiles/` 下新建一个目录，以游戏 ID 命名，比如 `profiles/my_game/`。在里面创建 `profile.toml`：

```toml
[game]
id = "my_game"
display_name = "My Game"
region = "US"
default_mode = "compat-interpreter"

[rom]
system = "nes"
accepted_format = ["ines", "nes20"]
mapper = 0
mapper_name = "NROM"
mirroring = "horizontal"
prg_size = 16384
chr_size = 8192
has_sram = false

[[rom.known_dump]]
name = "My Game (U)"
prg_crc32 = "00000000"    # 替换为实际 CRC
chr_crc32 = "00000000"

[cpu]
reset_vector = "auto"
nmi_vector = "auto"
irq_vector = "auto"
allow_decimal_mode = false
unknown_indirect_jump = "dispatcher"

[ppu]
initial_mode = "compat"
native_mode = "tilemap_sprite"
chr_export = true
palette_policy = "nes_palette"

[audio]
initial_mode = "apu_compat"

[input]
controller_ports = 2
default_backend_policy = "auto"
default_port_1 = "keyboard_gamepad"
```

关键字段说明：

- `mapper` — iNES mapper 编号。目前内置支持 NROM(0)、UxROM(2)、CNROM(3)。其他 mapper 需要自己实现 `MapperChip` trait（见下文"扩展 Mapper"）。
- `prg_size` / `chr_size` — 必须和 ROM 实际大小一致，否则解析会报错。
- `mirroring` — 从 ROM header 读，也可以在这里覆盖。

## 第二步：标符号表

创建 `symbols.ron`，把游戏里你关心的 RAM 地址和函数入口标出来：

```ron
(
    ram: {
        "lives":             0x0051,
        "player_x":          0x00A6,
        "player_y":          0x00A7,
        "player_direction":  0x00A8,
        "game_mode":         0x0078,
        "stage_number":      0x0085,
    },
    functions: {
        "nmi_handler":       0xFFF0,
        "reset_handler":     0xFFFC,
        "title_screen":      0xE000,
        "game_init":         0xE100,
        "player_move":       0xE200,
    },
    data: {
        "stage_data":        0xC000,
        "tank_sprites":      0xD000,
    },
)
```

符号表的作用：重编译器在分析代码时会参考这些标注，帮你理解游戏逻辑。调试 UI 的 RAM 查看器也会用这些名字显示变量。

怎么找这些地址？常用的方法：

- 用 FCEUX 或 Mesen 的调试器，在内存查看窗口里观察数值变化
- 搜已知的初始值（比如 3 条命就搜 `0x03`）
- 参考网上已有的 RAM map 文档

## 第三步：配 hook

创建 `hooks.ron`，标注代码区域的性质：

```ron
(
    hooks: [
        (
            address: 0xE000,
            name: "title_screen",
            hook_type: NamedFunction,
            size: Some(256),
            comment: Some("Title screen entry point"),
        ),
        (
            address: 0xC000,
            name: "stage_data",
            hook_type: DataTable,
            size: Some(2048),
            comment: Some("Stage layout data"),
        ),
    ],
)
```

`hook_type` 支持的值：

- `NamedFunction` — 函数入口，重编译器会从这里开始发现基本块
- `DataTable` — 数据区，跳过反汇编
- `InlineData` — 代码中间的内联数据（比如查表）
- `Ignore` — 跳过此区域

## 第四步：配输入映射

创建 `input.ron`，把 NES 按键映射到 PC 输入设备：

```ron
(
    ports: [
        (
            port: 0,
            backend: "keyboard_gamepad",
            mapping: {
                Up:    "KeyW",
                Down:  "KeyS",
                Left:  "KeyA",
                Right: "KeyD",
                Select:"KeyShiftLeft",
                Start: "KeyEnter",
                B:     "KeyJ",
                A:     "KeyK",
            },
        ),
    ],
)
```

按键名参考 `winit::keyboard::KeyCode` 的枚举值。

## 第五步：创建游戏 crate

在 `games/` 下新建一个目录，以游戏 ID 命名，比如 `games/my_game/`。

### Cargo.toml

```toml
[package]
name = "my-game"
version.workspace = true
edition.workspace = true

[[bin]]
name = "my-game"
path = "src/main.rs"

[lib]
name = "my_game"
path = "src/lib.rs"

[dependencies]
nptk = { path = "../../crates/nptk" }

[build-dependencies]
cc.workspace = true
nptk = { path = "../../crates/nptk", features = ["build"] }
```

### build.rs — AOT 编译

```rust
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 读取并解析 ROM（编译时嵌入）
    let rom_data = std::fs::read("roms/my_game.nes")?;
    let rom = nptk::rom::parse_rom(&rom_data)
        .map_err(|e| format!("ROM parse error: {}", e))?;

    // 从向量表开始 BFS 发现基本块
    let prg = &rom.prg_rom;
    let end = prg.len();
    let reset_vec = prg[end - 4] as u16 | ((prg[end - 3] as u16) << 8);
    let nmi_vec = prg[end - 6] as u16 | ((prg[end - 5] as u16) << 8);
    let irq_vec = prg[end - 2] as u16 | ((prg[end - 1] as u16) << 8);

    // ... BFS 发现基本块逻辑 ...

    // 使用 Cranelift AOT 编译
    let mut aot = nptk::build::CraneliftAot::new()
        .map_err(|e| format!("CraneliftAot::new: {}", e))?;
    for &(addr, ref bytes) in &block_data {
        let ir_ops = nptk::build::IrBuilder::lift_block(bytes, addr);
        aot.compile_block(addr, &ir_ops)?;
    }

    let (obj_bytes, compiled_blocks, block_names) = aot.finish()?;

    // 写入 .o 并通过 cc crate 链接
    let out_dir = std::env::var("OUT_DIR")?;
    let target_dir = Path::new(&out_dir)
        .parent().and_then(|p| p.parent())
        .ok_or("Cannot determine target directory")?;
    let obj_path = target_dir.join("nes_blocks.o");
    std::fs::write(&obj_path, &obj_bytes)?;
    cc::Build::new().object(&obj_path).compile("nes_blocks");

    // 生成 Rust 绑定代码
    let bindings = generate_bindings(&compiled_blocks);
    std::fs::write(Path::new(&out_dir).join("nes_blocks.rs"), &bindings)?;
    Ok(())
}
```

### src/main.rs — 游戏入口

```rust
use nptk::prelude::*;
use nptk::{ExecMode, FrameContext, GameHandlers, NesApp};
use nptk::runtime::{AudioEventSink, PpuEventSink, RecompiledRuntime};
use nptk::debug::DebugData;
use nptk::render::RenderMode;

// ROM 嵌入（编译时嵌入二进制）
const ROM_DATA: &[u8] = include_bytes!("../../roms/my_game.nes");

struct MyGame {
    system: NesSystem,
    recompiled: Option<RecompiledRuntime>,
    exec_mode: ExecMode,
}

impl MyGame {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let rom = nptk::rom::parse_rom(ROM_DATA)?;
        let mapper = nptk::mapper::create_mapper(rom.header.mapper_id, &rom)
            .ok_or_else(|| format!("Mapper {} not supported", rom.header.mapper_id))?;
        let cartridge = nptk::mapper::Cartridge::new_simple(
            nptk::mapper::CartridgeMetadata { /* ... */ },
            rom.prg_rom.clone(),
            nptk::mapper::ChrStorage::Rom(rom.chr_rom.unwrap_or_default()),
            mapper,
        );
        let system = NesSystem::from_cartridge(cartridge);
        Ok(MyGame { system, recompiled: None, exec_mode: ExecMode::Interpreter })
    }
}

impl GameHandlers for MyGame {
    fn window_title(&self) -> &str { "My Game — NES Port" }

    fn run_frame(&mut self, ctx: &mut FrameContext) {
        // 首次帧初始化重编译运行时
        if self.recompiled.is_none() {
            let rom = nptk::rom::parse_rom(ROM_DATA).unwrap();
            // ... 创建 RecompiledRuntime 并注册 AOT 块 ...
        }

        // 同步输入
        self.system.bus.controller[0].set_current(*ctx.input_state);

        // 执行帧
        let fb = match self.exec_mode {
            ExecMode::Recompiled => { /* ... */ }
            ExecMode::Interpreter => *self.system.run_frame(),
        };
        *ctx.framebuffer = fb;

        // 音频混音
        if let Some(ref tx) = *ctx.audio_tx {
            let apu = &self.system.bus.apu;
            ctx.apu_mixer.mix(
                nptk::system::CPU_CYCLES_PER_FRAME,
                apu.pulse1_output(), apu.pulse2_output(),
                apu.triangle_output(), apu.noise_output(),
            );
            for s in ctx.apu_mixer.drain_samples() {
                let _ = tx.send(s);
            }
        }

        // 调试数据
        if *ctx.show_debug {
            ctx.debug_overlay.update_nes_state(DebugData {
                cpu_a: self.system.cpu.a,
                // ... 其他字段 ...
                ram: Some(*self.system.ram()),
            });
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    NesApp::new(MyGame::new()?).run()
}
```

### 关键 API 一览

所有游戏移植需要的 API 都通过 `nptk` crate 暴露：

| 导入路径 | 类型/函数 | 用途 |
|---|---|---|
| `nptk::prelude::*` | — | 一键导入最常用类型 |
| `nptk::NesApp` | 结构体 | 应用框架，封装窗口/事件循环 |
| `nptk::GameHandlers` | trait | 游戏需实现的回调（`run_frame`） |
| `nptk::FrameContext` | 结构体 | 每帧上下文（输入、帧缓冲、音频、调试） |
| `nptk::ExecMode` | 枚举 | `Interpreter` / `Recompiled` |
| `nptk::rom::parse_rom` | 函数 | 解析 iNES/NES2.0 ROM |
| `nptk::system::NesSystem` | 结构体 | NES 系统（帧循环、CPU/PPU/APU 同步） |
| `nptk::bus::NesBusImpl` | 结构体 | NES 总线实现 |
| `nptk::controller::NesControllerState` | 结构体 | NES 控制器状态（8 按钮） |
| `nptk::mapper::Cartridge` | 结构体 | 卡带容器 |
| `nptk::runtime::RecompiledRuntime` | 结构体 | 重编译运行时 |
| `nptk::runtime::CAbiBlockFn` | 类型别名 | AOT 编译块函数指针 |
| `nptk::runtime::NativeCpuState` | 结构体 | 原生 CPU 状态 |
| `nptk::debug::DebugData` | 结构体 | 调试数据快照 |
| `nptk::render::RenderMode` | 枚举 | `Framebuffer` / `Native` |
| `nptk::audio::ApuMixer` | 结构体 | APU 音频混音器 |
| `nptk::profile::CodeHook` | 结构体 | 代码钩子定义 |
| `nptk::build::CraneliftAot` | 结构体 | AOT 编译器（用于 build.rs） |
| `nptk::build::IrBuilder` | 结构体 | IR 构建器（用于 build.rs） |

## 第六步：验证

创建 `tests.ron`，定义 golden test：

```ron
(
    frames: [
        (
            name: "title_screen",
            frame_count: 60,
            hash: "sha256:...",
        ),
    ],
)
```

然后运行：

```bash
cargo run --release --bin nptk-port -- golden \
    --rom roms/my_game.nes \
    --profile profiles/my_game/profile.toml
```

这会跑指定帧数，计算 framebuffer 的 hash，和 `tests.ron` 里的值比对。第一次跑的时候先不填 hash，让工具输出实际 hash 值，确认画面正确后再写进去。

## 扩展 Mapper

如果游戏用的 mapper 不在内置列表里（NROM/UxROM/CNROM 之外），需要自己实现。

新建一个 crate，比如 `mappers/mmc1/`，实现 `MapperChip` trait：

```rust
use nptk::mapper::{MapperChip, MapperContext};
use std::cell::RefCell;
use std::rc::Rc;

pub struct Mmc1 {
    // 你的状态
}

impl MapperChip for Mmc1 {
    fn mapper_id(&self) -> u16 { 1 }
    fn name(&self) -> &'static str { "MMC1" }

    fn cpu_read(&mut self, ctx: &Rc<RefCell<MapperContext>>, addr: u16) -> Option<u8> {
        // CPU 地址空间读取逻辑
    }

    fn cpu_write(&mut self, ctx: &Rc<RefCell<MapperContext>>, addr: u16, value: u8) -> bool {
        // CPU 地址空间写入逻辑（bank 切换等）
    }

    fn ppu_read(&mut self, ctx: &Rc<RefCell<MapperContext>>, addr: u16) -> Option<u8> {
        // PPU 地址空间读取（CHR bank 切换）
    }

    fn ppu_write(&mut self, ctx: &Rc<RefCell<MapperContext>>, addr: u16, value: u8) -> bool {
        // PPU 地址空间写入
    }

    fn mirroring(&self) -> nptk::rom::Mirroring {
        nptk::rom::Mirroring::Horizontal
    }
}
```

然后用 `linkme` 注册到全局 mapper registry：

```rust
use nptk::mapper::registry::MAPPER_REGISTRY;

#[linkme::distributed_slice(MAPPER_REGISTRY)]
static REGISTER_MMC1: fn() -> Box<dyn MapperChip> = || Box::new(Mmc1::new());
```

最后在 `Cargo.toml` 里把新 crate 加入工作空间，`nptk-mapper` 会自动聚合。

## 写原生 hook（进阶）

如果只是把游戏跑起来，到上一步就够了。但如果想真正做"移植"——用原生代码替换游戏的部分逻辑——就需要写 hook。

hook 在 `hooks.ron` 里声明，然后在 Rust 代码里实现对应的替换函数。运行时，当 CPU 执行到 hook 标注的地址时，框架会调用你的 Rust 函数而不是执行原始 6502 代码。

典型场景：

- 用 WGPU 原生绘制替换游戏的软件渲染
- 用 Kira 播放原生音效替换 APU 音频
- 截取游戏状态（比如读取 `lives` 地址来显示在自定义 HUD 上）

具体实现参考 `games/battle-city/src/` 里的例子。

## 调试技巧

- 先用 `compat-interpreter` 模式跑，确认游戏能正常运行
- 切到 `recompiled-compat` 模式，看重编译有没有问题
- 用 `nptk-port trace` 记录 CPU trace，和已知正确的 trace 比对
- 打开 egui 调试面板（GUI 应用默认带），查看 CPU/PPU/RAM 状态
- 如果重编译后画面异常，检查 hook 标注是否准确，特别是 `DataTable` 有没有被误标为代码

## 开发策略

```
正确性优先
  → 替换 CPU 执行（重编译器）
  → 替换渲染（WGPU）
  → 替换音频（CPAL / Kira）
  → 语义游戏状态提取
  → 推广到更多 NES/Famicom 游戏
```