# FC/NES 原生移植框架实现计划

**默认实现：Battle City / 坦克大战**  
**技术栈：Rust + WGPU + 可插拔输入/音频/重编译 Runtime**  
**资料检索日期：2026-07-06**  
**文档版本：v0.2，新增 XInput / GameInput / RawInput / DirectInput 等手柄 API 输入设计**

---

## 0. 项目目标

本项目的目标不是把 FC/NES ROM 放进一个模拟器壳里运行，而是建立一个可复用的 **FC/NES 原生移植框架**：

```text
FC/NES ROM
  ↓
ROM 与 GameProfile 识别
  ↓
6502 代码分析、运行追踪、静态重编译
  ↓
NES 硬件兼容 Runtime
  ↓
WGPU 原生 Tile/Sprite 渲染
  ↓
CPAL/Kira 原生音频
  ↓
可插拔输入系统：winit / gilrs / XInput / GameInput / RawInput / HID / Web Gamepad
  ↓
Windows / Linux / macOS / WebAssembly / 其他 wgpu 支持平台
```

默认游戏 Profile 选择 **Battle City / 坦克大战**。公开资料显示，Battle City 的 NES 版本是 Mapper 0 / NROM，PRG-ROM 为 `1 x 16 KiB`，CHR-ROM 为 `1 x 8 KiB`，水平镜像，无 SRAM，因此非常适合作为第一个重编译和原生化目标。[^battle-city]

核心目标：

1. 框架通用，Battle City 只是默认实现。
2. 保留兼容运行模式，用于和原版行为对照。
3. 通过静态重编译把 6502 逻辑迁移为平台原生代码。
4. 逐步把 PPU/APU/输入替换为原生渲染、音频和输入系统。
5. 使用 Rust + WGPU 生态，尽量复用成熟库。
6. Windows 输入层新增显式 XInput、GameInput、RawInput、DirectInput/HID 支持，而不是只依赖单一跨平台库。
7. 不分发商业 ROM、CHR dump、原始音频或其他版权资源，只分发工具、Profile、符号表、补丁、重编译器和生成流程。

---

## 1. 总体架构

```text
                         ┌───────────────────────────┐
                         │        User ROM            │
                         └─────────────┬─────────────┘
                                       │
                                       ▼
┌─────────────────────────────────────────────────────────────────┐
│                         nptk-core                                  │
│  ROM Parser │ Mapper │ NesBus │ CPU Ref │ PPU Compat │ APU Compat │
└─────────────┬───────────────────────────────────────────────────┘
              │
              ▼
┌─────────────────────────────────────────────────────────────────┐
│                       nptk-profile                                 │
│   GameProfile │ Symbols │ RAM Map │ Hooks │ Test Config           │
└─────────────┬───────────────────────────────────────────────────┘
              │
              ▼
┌─────────────────────────────────────────────────────────────────┐
│                     nptk-recompiler                                │
│   Disasm │ CFG │ IR6502 │ Analysis │ Rust Codegen │ Manifest      │
└─────────────┬───────────────────────────────────────────────────┘
              │
              ▼
┌─────────────────────────────────────────────────────────────────┐
│                  nptk-native-runtime                               │
│   NesRuntime ABI │ PPU Events │ Audio Events │ Input Bridge       │
└─────────────┬───────────────────────────────────────────────────┘
              │
       ┌──────┼────────────┬─────────────┐
       ▼      ▼            ▼             ▼
┌──────────┐ ┌──────────┐ ┌───────────┐ ┌─────────────────────┐
│ nptk-wgpu  │ │ nptk-audio │ │ nptk-input  │ │ nptk-battle-city       │
│ Renderer │ │ CPAL/Kira│ │ Backends  │ │ Default Game Profile │
└──────────┘ └──────────┘ └───────────┘ └─────────────────────┘
```

框架支持三种运行模式：

| 模式 | 目的 | 说明 |
|---|---|---|
| `compat-interpreter` | 正确性基准 | 6502 interpreter + PPU/APU 兼容层 |
| `recompiled-compat` | 重编译验证 | 静态重编译 6502 代码 + 兼容 Runtime |
| `native-port` | 原生移植目标 | 重编译逻辑 + WGPU Tile/Sprite 渲染 + 原生输入/音频 |

开发策略：

```text
先保证行为正确
→ 再替换 CPU 执行方式
→ 再替换渲染
→ 再替换音频
→ 再语义化游戏状态
→ 最后泛化到更多 FC/NES 游戏
```

---

## 2. 关键资料汇总

### 2.1 NES/FC 硬件资料

NES CPU 地址空间中，`$0000-$07FF` 是 2 KiB internal RAM，`$2000-$2007` 是 PPU 寄存器，`$4000-$4017` 是 APU 与 I/O，`$8000-$FFFF` 通常由卡带 PRG-ROM 或 Mapper 提供。[^cpu-map]

PPU 寄存器不是普通内存；例如 `$2000-$2007` 有镜像和副作用，`$2005/$2006/$2007` 的 latch 与地址递增行为会直接影响画面正确性。[^ppu-registers]

PPU 地址空间中，`$0000-$1FFF` 通常是 pattern table / CHR，`$2000-$2FFF` 是 nametable，`$3F00-$3FFF` 是 palette。OAM 为 256 bytes，最多 64 个 sprite，每个 4 bytes。[^ppu-memory][^oam]

### 2.2 ROM 与 Mapper 资料

框架需要支持 iNES 与 NES 2.0。NES 2.0 的 16 字节 header 提供 mapper、submapper、PRG/CHR size、mirroring、battery、console type 等字段。[^nes20]

第一阶段只实现：

```text
iNES / NES 2.0 header
Mapper 0 / NROM
PRG-ROM 16 KiB / 32 KiB
CHR-ROM 8 KiB
水平 / 垂直镜像
无 SRAM
```

后续再扩展：

```text
CNROM
UxROM
MMC1
MMC3
CHR-RAM
Battery-backed SRAM
PAL / NTSC 差异
```

### 2.3 Battle City 默认 Profile 资料

Battle City 默认硬件信息：

```text
Mapper: 0 / NROM
PRG-ROM: 16 KiB
CHR-ROM: 8 KiB
Mirroring: Horizontal
SRAM: No
```

Data Crystal 还提供了部分 RAM map，例如 lives、stage counter、current tank state、shield status、current block type 等，可作为符号表起点，但并不代表逆向信息完整。[^battle-city-ram]

---

## 3. Rust 生态选型

### 3.1 渲染与窗口

| 领域 | 推荐库 | 说明 |
|---|---|---|
| GPU 渲染 | `wgpu` | 跨平台、safe、pure-Rust 图形 API；原生支持 Vulkan、Metal、D3D12、OpenGL，WASM 上支持 WebGPU/WebGL2。[^wgpu] |
| 窗口与事件循环 | `winit` | 跨平台窗口创建和事件循环库，负责窗口、键盘、鼠标、焦点、resize 等事件。[^winit] |
| 调试 UI | `egui-wgpu` | CPU/RAM/PPU/OAM viewer、trace viewer、input inspector、frame hash 对照。 |

WGPU 渲染层分成：

```text
NesFramebufferPass     # 兼容模式 framebuffer 上传
TilemapPass            # 原生背景 tilemap
SpritePass             # 原生 OAM sprite
PalettePass            # NES palette / shader tint
DebugOverlayPass       # egui 调试界面
PresentPass            # 最终输出
```

### 3.2 音频

| 领域 | 推荐库 | 说明 |
|---|---|---|
| 低层音频输出 | `cpal` | Rust 低层跨平台 audio I/O 库，适合输出 APU 兼容层生成的 PCM。[^cpal] |
| 游戏音频 | `kira` | 面向游戏的音频库，支持 mixer、tween、clock、spatial audio，适合原生 SFX/BGM。[^kira] |

音频路线：

```text
阶段 1: APU compat → CPAL PCM output
阶段 2: SFX hook → Kira
阶段 3: BGM hook / native sequencer → Kira
```

### 3.3 6502 / NES 相关库

| 用途 | 推荐库 | 说明 |
|---|---|---|
| 参考 CPU / 语义交叉验证 | `mos6502` 或自研 minimal CPU | 可用于 interpreter mode 与指令级测试。 |
| 6502 反汇编 | `disasm6502` | 支持 6502 binary disassembly、forbidden instructions、cycle count、寄存器访问和 flag 影响信息。[^disasm6502] |
| NES emulator 参考 | `TetaNES` / `monsoon_core` | 仅作参考或测试对照，不作为主 Runtime 强依赖。 |

### 3.4 工程基础库

| 领域 | 推荐库 | 说明 |
|---|---|---|
| CLI | `clap` | `nptk-port inspect/run/trace/recompile/dump-chr/golden` |
| 配置 | `serde` + `toml` / `ron` | `GameProfile`、symbols、hooks、tests |
| 日志 | `tracing` | 结构化日志、trace、调试 |
| 图像 | `image` | CHR 导出 PNG、golden frame、debug atlas |
| GPU POD | `bytemuck` | Vertex/uniform buffer 类型转换 |
| Benchmark | `criterion` | CPU interpreter、recompiled block、renderer benchmark |

---

## 4. Workspace 结构

```text
nptk-native/
├── Cargo.toml
├── crates/
│   ├── nptk-core/
│   │   ├── src/
│   │   │   ├── rom/
│   │   │   ├── mapper/
│   │   │   ├── bus/
│   │   │   ├── cpu_ref/
│   │   │   ├── ppu_compat/
│   │   │   ├── apu_compat/
│   │   │   ├── controller/
│   │   │   └── runtime.rs
│   │   └── Cargo.toml
│   │
│   ├── nptk-profile/
│   │   ├── src/
│   │   │   ├── profile.rs
│   │   │   ├── symbols.rs
│   │   │   ├── hooks.rs
│   │   │   └── validation.rs
│   │   └── Cargo.toml
│   │
│   ├── nptk-recompiler/
│   │   ├── src/
│   │   │   ├── disasm.rs
│   │   │   ├── cfg.rs
│   │   │   ├── ir6502.rs
│   │   │   ├── analysis.rs
│   │   │   ├── codegen_rust.rs
│   │   │   └── manifest.rs
│   │   └── Cargo.toml
│   │
│   ├── nptk-native-runtime/
│   │   ├── src/
│   │   │   ├── nes_runtime.rs
│   │   │   ├── ppu_bridge.rs
│   │   │   ├── audio_bridge.rs
│   │   │   ├── input_bridge.rs
│   │   │   ├── state_bridge.rs
│   │   │   └── save_state.rs
│   │   └── Cargo.toml
│   │
│   ├── nptk-wgpu/
│   │   ├── src/
│   │   │   ├── app.rs
│   │   │   ├── renderer.rs
│   │   │   ├── tilemap.rs
│   │   │   ├── sprite.rs
│   │   │   ├── palette.rs
│   │   │   ├── debug_ui.rs
│   │   │   └── shaders/
│   │   └── Cargo.toml
│   │
│   ├── nptk-audio/
│   │   ├── src/
│   │   │   ├── cpal_output.rs
│   │   │   ├── apu_mixer.rs
│   │   │   ├── kira_events.rs
│   │   │   └── audio_policy.rs
│   │   └── Cargo.toml
│   │
│   ├── nptk-input/
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── backend.rs
│   │   │   ├── canonical.rs
│   │   │   ├── mapper.rs
│   │   │   ├── nes_controller.rs
│   │   │   ├── replay.rs
│   │   │   ├── hotplug.rs
│   │   │   ├── backends/
│   │   │   │   ├── winit_keyboard.rs
│   │   │   │   ├── gilrs_gamepad.rs
│   │   │   │   ├── xinput_windows.rs
│   │   │   │   ├── gameinput_windows.rs
│   │   │   │   ├── rawinput_windows.rs
│   │   │   │   ├── directinput_windows.rs
│   │   │   │   ├── hidapi_generic.rs
│   │   │   │   └── web_gamepad.rs
│   │   │   └── tests/
│   │   └── Cargo.toml
│   │
│   ├── nptk-tools/
│   │   ├── src/bin/
│   │   │   ├── nptk-port.rs
│   │   │   ├── nptk-dump-chr.rs
│   │   │   ├── nptk-trace.rs
│   │   │   ├── nptk-build-profile.rs
│   │   │   ├── nptk-recompile.rs
│   │   │   └── nptk-input-test.rs
│   │   └── Cargo.toml
│   │
│   └── nptk-battle-city/
│       ├── src/
│       │   ├── lib.rs
│       │   ├── native_hooks.rs
│       │   ├── game_state.rs
│       │   └── battle_city_runtime.rs
│       └── Cargo.toml
│
├── profiles/
│   └── battle_city/
│       ├── profile.toml
│       ├── symbols.ron
│       ├── ram_map.ron
│       ├── hooks.ron
│       ├── input.ron
│       ├── tests.ron
│       └── README.md
│
├── generated/
│   └── battle_city/
│       ├── recompiled.rs
│       ├── blocks.rs
│       ├── symbols.rs
│       └── manifest.json
│
├── assets/
│   └── README.md
│
├── tests/
│   ├── cpu/
│   ├── ppu/
│   ├── apu/
│   ├── input/
│   ├── trace/
│   └── golden/
│
└── docs/
    ├── IMPLEMENTATION_PLAN.md
    ├── PROFILE_FORMAT.md
    ├── RECOMPILER.md
    ├── RUNTIME_ABI.md
    ├── INPUT_BACKENDS.md
    └── LEGAL.md
```

---

## 5. GameProfile 设计

### 5.1 Battle City Profile 示例

```toml
[game]
id = "battle_city"
display_name = "Battle City"
region = "JP"
default_mode = "native-port"

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
name = "Battle City (J)"
# CRC 仅用于识别用户本地 ROM；仓库不包含 ROM 内容。
prg_crc32 = "optional"
chr_crc32 = "optional"
combined_crc32 = "optional"

[cpu]
reset_vector = "auto"
nmi_vector = "auto"
irq_vector = "auto"
allow_decimal_mode = false
unknown_indirect_jump = "dispatcher"

[ppu]
initial_mode = "compat"
native_mode = "tilemap_sprite"
sprite_source = "oam"
background_source = "nametable"
chr_export = true
palette_policy = "nes_palette"

[audio]
initial_mode = "apu_compat"
native_sfx = "optional"
native_bgm = "optional"

[input]
controller_ports = 2
default_backend_policy = "auto"
default_port_1 = "keyboard_gamepad"
input_profile = "profiles/battle_city/input.ron"

[testing]
enable_trace_compare = true
enable_golden_frames = true
enable_input_replay = true
```

### 5.2 RAM 符号表示例

```ron
(
    ram: {
        "lives": 0x0051,
        "stage_counter": 0x0085,
        "skip_current_level": 0x0080,
        "power_counter_or_enemies_killed": 0x0019,
        "power_position": 0x0086,
        "power_status": 0x0049,
        "current_tank_state": 0x00A8,
        "shield_status": 0x0089,
        "current_block_type": 0x005C,
    }
)
```

### 5.3 输入 Profile 示例

```ron
(
    ports: [
        (
            port: 1,
            sources: ["keyboard", "gamepad0"],
            mapping: {
                "nes_a":      ["keyboard:KeyZ", "gamepad:South"],
                "nes_b":      ["keyboard:KeyX", "gamepad:East"],
                "nes_start":  ["keyboard:Enter", "gamepad:Start"],
                "nes_select": ["keyboard:ShiftRight", "gamepad:Select"],
                "nes_up":     ["keyboard:ArrowUp", "gamepad:DPadUp", "gamepad:LeftStickY<-0.5"],
                "nes_down":   ["keyboard:ArrowDown", "gamepad:DPadDown", "gamepad:LeftStickY>0.5"],
                "nes_left":   ["keyboard:ArrowLeft", "gamepad:DPadLeft", "gamepad:LeftStickX<-0.5"],
                "nes_right":  ["keyboard:ArrowRight", "gamepad:DPadRight", "gamepad:LeftStickX>0.5"],
            },
            opposite_direction_policy: "neutralize",
            analog_deadzone: 0.25,
            analog_hysteresis: 0.05,
        ),
        (
            port: 2,
            sources: ["gamepad1"],
            mapping: "default_nes",
            opposite_direction_policy: "neutralize",
            analog_deadzone: 0.25,
            analog_hysteresis: 0.05,
        )
    ],

    backend_policy: (
        windows: ["gilrs_wgi", "xinput", "rawinput", "directinput", "keyboard"],
        linux:   ["gilrs_evdev", "hidapi", "keyboard"],
        macos:   ["gilrs", "hidapi", "keyboard"],
        wasm:    ["web_gamepad", "keyboard"],
    )
)
```

---

## 6. Core Runtime 设计

### 6.1 NES 总线抽象

重编译代码不能直接访问数组，必须经过总线。

```rust
pub trait NesBus {
    fn cpu_read(&mut self, addr: u16) -> u8;
    fn cpu_write(&mut self, addr: u16, value: u8);

    fn ppu_read(&mut self, addr: u16) -> u8;
    fn ppu_write(&mut self, addr: u16, value: u8);

    fn tick_cpu(&mut self, cycles: u32);
}
```

原因是 CPU 地址空间中的 PPU、APU、controller、DMA、mapper 寄存器都有副作用。`$2000-$2007`、`$4000-$4017`、`$8000-$FFFF` 不能当普通 RAM 处理。

### 6.2 Mapper 插件

```rust
pub trait Mapper {
    fn cpu_read(&mut self, addr: u16) -> Option<u8>;
    fn cpu_write(&mut self, addr: u16, value: u8) -> bool;

    fn ppu_read(&mut self, addr: u16) -> Option<u8>;
    fn ppu_write(&mut self, addr: u16, value: u8) -> bool;

    fn mirroring(&self) -> Mirroring;
    fn mapper_id(&self) -> u16;
}
```

第一阶段实现 `Mapper0Nrom`。NROM 没有 mapper register，CPU PRG-ROM 固定映射，PPU `$0000-$1FFF` 映射 CHR-ROM 或 CHR-RAM。[^nrom]

### 6.3 Runtime ABI

重编译后的 6502 代码只依赖稳定 ABI：

```rust
pub trait NesRuntime {
    fn read8(&mut self, addr: u16) -> u8;
    fn write8(&mut self, addr: u16, value: u8);

    fn advance_cpu_cycles(&mut self, cycles: u32);

    fn nmi_pending(&self) -> bool;
    fn clear_nmi(&mut self);

    fn read_controller_shift(&mut self, port: u8) -> u8;
    fn write_controller_strobe(&mut self, value: u8);

    fn ppu_events(&mut self) -> &mut dyn PpuEventSink;
    fn audio_events(&mut self) -> &mut dyn AudioEventSink;
}
```

这样同一份重编译代码可以运行在：

```text
兼容 PPU/APU
原生 WGPU renderer
headless test runner
trace recorder
debugger
```

---

## 7. 6502 静态重编译设计

### 7.1 Pipeline

```text
1. Load ROM
2. Parse iNES / NES 2.0 header
3. Instantiate Mapper
4. Resolve Reset/NMI/IRQ vectors
5. Disassemble reachable code
6. Merge static analysis + runtime trace
7. Classify code / data / jump table
8. Lift 6502 instructions to IR6502
9. Split basic blocks
10. Build CFG
11. Apply GameProfile hooks and annotations
12. Generate Rust code
13. Compile generated crate
14. Run trace comparison
15. Enable native renderer/audio/input hooks
```

### 7.2 IR6502 示例

```rust
pub enum IrOp {
    LoadA(Operand),
    LoadX(Operand),
    LoadY(Operand),

    StoreA(Address),
    StoreX(Address),
    StoreY(Address),

    AddWithCarry(Operand),
    SubWithCarry(Operand),

    And(Operand),
    Or(Operand),
    Xor(Operand),

    Branch {
        condition: BranchCondition,
        target: Label,
        fallthrough: Label,
    },

    Jump(Label),
    JumpIndirect(Address),
    Call(Label),
    Return,

    ReadBus { addr: Expr, dst: Temp },
    WriteBus { addr: Expr, value: Expr },

    SetFlags(FlagUpdate),
    AdvanceCycles(u8),
}
```

### 7.3 生成 Rust 代码示例

```rust
pub fn block_8120<R: NesRuntime>(rt: &mut R, cpu: &mut CpuState) -> BlockExit {
    let value = rt.read8(0x0051);
    cpu.a = value;
    cpu.set_zn(cpu.a);
    rt.advance_cpu_cycles(3);

    if cpu.flag_z() {
        BlockExit::Jump(Label::Block_8180)
    } else {
        BlockExit::Jump(Label::Block_8125)
    }
}
```

### 7.4 间接跳转策略

```text
优先级 1: 静态识别 jump table
优先级 2: FCEUX/Mesen trace 收集实际目标
优先级 3: runtime dispatcher fallback
```

---

## 8. PPU 与 WGPU 原生渲染

### 8.1 兼容模式

```text
PPU compat
  ↓
256x240 indexed framebuffer
  ↓
NES palette
  ↓
wgpu texture upload
  ↓
screen quad
```

用于：

```text
trace 对照
golden frame
debugging
行为锁定
```

### 8.2 原生 Tile/Sprite 模式

```text
CHR-ROM
  → Texture Atlas

Nametable
  → Tilemap instances

Attribute Table
  → Palette region / material index

OAM
  → Sprite instances

PPU Scroll
  → Camera / viewport offset

Palette RAM
  → Uniform / storage buffer
```

WGPU instance 数据：

```rust
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TileInstance {
    pub pos: [f32; 2],
    pub tile_id: u32,
    pub palette_id: u32,
    pub flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SpriteInstance {
    pub pos: [f32; 2],
    pub tile_id: u32,
    pub palette_id: u32,
    pub priority: u32,
    pub flip_x: u32,
    pub flip_y: u32,
}
```

Battle City 的画面结构是固定屏幕 tile 地图 + sprite 坦克/子弹/爆炸，很适合优先原生化。

---

## 9. APU 与原生音频

### 9.1 APU 兼容层

```text
CPU writes $4000-$4017
  ↓
APU compat
  ↓
PCM ring buffer
  ↓
CPAL output stream
```

APU 必须随 CPU cycle 推进，而不是只在“播放声音”时更新，因为 frame counter、envelope、length counter、sweep、IRQ 等都与时序有关。[^apu-frame]

### 9.2 原生音频事件层

```rust
pub enum NativeAudioEvent {
    PlaySfx { id: SfxId },
    StopSfx { id: SfxId },
    PlayBgm { id: BgmId },
    StopBgm,
    SetVolume { bus: AudioBus, value: f32 },
}
```

Battle City 建议：

```text
阶段 1: 保留 APU BGM/SFX，确保声音接近原版
阶段 2: explosion/fire/powerup 等 SFX hook 到 Kira
阶段 3: 可选重写 BGM sequencer 或播放重制音频
```

---

## 10. 输入系统：新增 XInput / GameInput / RawInput / DirectInput 等 API

### 10.1 输入系统目标

输入层需要满足三个方向：

1. **可玩性**：键盘、Xbox 手柄、DualShock/DualSense、Switch Pro、通用 HID 手柄尽量开箱即用。
2. **确定性**：测试、trace replay、TAS-like replay 必须以帧为单位复现输入。
3. **原生性**：Windows 上除了跨平台 gilrs，还要提供明确的 XInput / GameInput / RawInput / DirectInput 后端。

### 10.2 输入分层

```text
OS / API backend
  ├─ winit keyboard
  ├─ gilrs gamepad
  ├─ Windows XInput
  ├─ Windows GameInput
  ├─ Windows RawInput
  ├─ Windows DirectInput legacy
  ├─ hidapi generic HID
  └─ Web Gamepad API
        ↓
RawInputEvent / RawGamepadState
        ↓
CanonicalGamepadState
        ↓
InputMapper
        ↓
NesControllerState
        ↓
NES controller shift register
        ↓
CPU reads $4016 / $4017
```

### 10.3 核心数据结构

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputBackendKind {
    WinitKeyboard,
    Gilrs,
    WindowsXInput,
    WindowsGameInput,
    WindowsRawInput,
    WindowsDirectInput,
    HidApi,
    WebGamepad,
    Replay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhysicalDeviceId {
    pub backend: InputBackendKind,
    pub local_id: u64,
}

#[derive(Debug, Clone)]
pub struct RawGamepadState {
    pub device_id: PhysicalDeviceId,
    pub name: String,
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    pub buttons: Vec<bool>,
    pub axes: Vec<f32>,
    pub hats: Vec<HatState>,
    pub timestamp_ns: u64,
}

#[derive(Debug, Clone, Default)]
pub struct CanonicalGamepadState {
    pub south: bool,
    pub east: bool,
    pub west: bool,
    pub north: bool,
    pub left_shoulder: bool,
    pub right_shoulder: bool,
    pub left_trigger: f32,
    pub right_trigger: f32,
    pub select: bool,
    pub start: bool,
    pub guide: bool,
    pub left_stick_button: bool,
    pub right_stick_button: bool,
    pub dpad_up: bool,
    pub dpad_down: bool,
    pub dpad_left: bool,
    pub dpad_right: bool,
    pub left_stick: [f32; 2],
    pub right_stick: [f32; 2],
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NesControllerState {
    pub a: bool,
    pub b: bool,
    pub select: bool,
    pub start: bool,
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
}
```

NES controller shift 顺序：

```text
A, B, Select, Start, Up, Down, Left, Right
```

### 10.4 InputBackend trait

```rust
pub trait InputBackend {
    fn kind(&self) -> InputBackendKind;

    fn poll(&mut self, now_ns: u64, sink: &mut dyn InputEventSink);

    fn connected_devices(&self) -> Vec<InputDeviceInfo>;

    fn set_rumble(&mut self, device: PhysicalDeviceId, low: f32, high: f32) -> Result<(), InputError> {
        let _ = (device, low, high);
        Err(InputError::Unsupported)
    }
}
```

### 10.5 默认后端优先级

#### Windows desktop

```text
1. gilrs_wgi
2. xinput_explicit
3. rawinput
4. directinput_legacy
5. hidapi_generic
6. winit_keyboard
```

说明：

- `gilrs` 作为默认跨平台 gamepad 层，Windows 默认使用 Windows Gaming Input，也可以通过 feature 切换到 XInput。gilrs 文档显示其支持输入、热插拔、force feedback；支持 SDL-compatible controller mappings；Windows 上默认启用 `wgi`，也提供 `xinput` feature。[^gilrs]
- `xinput_explicit` 用于最常见的 Xbox/XInput 手柄路径，也用于没有焦点窗口或需要直接轮询 0..3 号控制器时。
- `rawinput` 用于获取 generic HID gamepad / joystick，并支持自定义映射。
- `directinput_legacy` 用于旧设备，但必须避免把 XInput 设备通过 DirectInput 重复枚举。Microsoft 文档说明 XInput 设备会同时表现为 XInput 和 DirectInput 设备，支持 side-by-side 时需要过滤重复设备。[^xinput-directinput]

#### Linux / BSD

```text
1. gilrs_evdev
2. hidapi_generic
3. winit_keyboard
```

gilrs 在 Linux/BSD 上通过 evdev 直接读写 `/dev/input/event*`，必要时需要 udev 权限配置。[^gilrs]

#### macOS

```text
1. gilrs
2. hidapi_generic
3. winit_keyboard
```

#### WebAssembly

```text
1. web_gamepad
2. winit_keyboard / browser keyboard events
```

Web 平台使用浏览器 Gamepad API 作为主要手柄来源。MDN 文档说明 Gamepad API 用于访问 gamepads 和其他 game controllers。[^web-gamepad]

### 10.6 Windows XInputBackend

#### 适用场景

```text
Xbox 360 / Xbox One / Xbox Series 控制器
大量支持 XInput 的第三方手柄
需要低复杂度、稳定映射、rumble 支持的 Windows desktop build
```

#### 实现选择

方案 A：使用 `rusty_xinput`

- 优点：动态加载 XInput DLL，安全封装，轮询 0..3 控制器，支持 rumble。
- 注意：文档建议调用动态加载函数，并准备在 XInput 不可用时 fallback 到 keyboard/mouse。[^rusty-xinput]

方案 B：直接用 `windows` crate

- 优点：统一调用 Windows API，可与 RawInput、GameInput、WinRT 输入放在同一 Windows 后端 crate 中。
- Microsoft 文档说明 Rust for Windows 可通过 `windows` crate 调用 Windows API。[^windows-crate]

建议：

```text
MVP: 使用 rusty_xinput 快速落地
长期: 提供 windows-crate 实现，作为 feature = "windows-native-input"
```

#### XInputBackend 伪代码

```rust
pub struct XInputBackend {
    handle: Option<rusty_xinput::XInputHandle>,
    slots: [Option<XInputDeviceState>; 4],
}

impl InputBackend for XInputBackend {
    fn kind(&self) -> InputBackendKind {
        InputBackendKind::WindowsXInput
    }

    fn poll(&mut self, now_ns: u64, sink: &mut dyn InputEventSink) {
        let Some(handle) = &self.handle else { return; };

        for slot in 0..4u32 {
            match handle.get_state(slot) {
                Ok(state) => {
                    let raw = convert_xinput_state(slot, state, now_ns);
                    sink.on_raw_gamepad(raw);
                }
                Err(_) => {
                    sink.on_device_maybe_disconnected(InputBackendKind::WindowsXInput, slot as u64);
                }
            }
        }
    }

    fn set_rumble(&mut self, device: PhysicalDeviceId, low: f32, high: f32) -> Result<(), InputError> {
        let Some(handle) = &self.handle else { return Err(InputError::BackendUnavailable); };
        let slot = device.local_id as u32;
        handle.set_state(slot, scale_rumble(low), scale_rumble(high))?;
        Ok(())
    }
}
```

#### XInput 映射到 CanonicalGamepadState

```text
XINPUT_GAMEPAD_A              → south
XINPUT_GAMEPAD_B              → east
XINPUT_GAMEPAD_X              → west
XINPUT_GAMEPAD_Y              → north
XINPUT_GAMEPAD_LEFT_SHOULDER  → left_shoulder
XINPUT_GAMEPAD_RIGHT_SHOULDER → right_shoulder
XINPUT_GAMEPAD_BACK           → select
XINPUT_GAMEPAD_START          → start
XINPUT_GAMEPAD_DPAD_UP        → dpad_up
XINPUT_GAMEPAD_DPAD_DOWN      → dpad_down
XINPUT_GAMEPAD_DPAD_LEFT      → dpad_left
XINPUT_GAMEPAD_DPAD_RIGHT     → dpad_right
sThumbLX/sThumbLY             → left_stick
sThumbRX/sThumbRY             → right_stick
bLeftTrigger                  → left_trigger
bRightTrigger                 → right_trigger
```

NES 默认映射：

```text
NES A      ← south
NES B      ← east
Start      ← start
Select     ← select
D-pad      ← dpad 或 left stick digitalized
```

#### XInput 限制

- 经典 XInput 模型按 slot 轮询，通常最多 4 个控制器。
- 主要面向 XUSB/XInput 控制器，不覆盖所有 legacy DirectInput/HID 设备。
- 对非 Xbox 设备支持取决于驱动或第三方 wrapper。

### 10.7 Windows GameInputBackend

Microsoft 把 GameInput 定位为下一代输入 API。其文档说明 GameInput 在 PC 上通过 NuGet 可用，支持 Windows 10 19H1 及之后版本；它用统一模型暴露 keyboards、mice、gamepads、其他 game controllers，并被描述为 XInput、DirectInput、Raw Input、HID、WinRT APIs 的功能超集。[^gameinput]

#### 适用场景

```text
Windows 10 19H1+
希望统一处理 gamepad / keyboard / mouse / raw devices
希望使用 polling 或 callback
希望获得 haptics / sensors / force feedback / lower-level device access
未来 Xbox/GDK 目标
```

#### 集成策略

GameInput 的分发、头文件、链接方式与普通 crates.io Rust 依赖不完全一样，因此建议作为 **optional feature**：

```toml
[features]
default = ["gilrs", "winit-keyboard"]
windows-xinput = ["dep:rusty_xinput"]
windows-native-input = ["dep:windows"]
windows-gameinput = ["windows-native-input"]
```

#### GameInputBackend 设计

```rust
pub struct GameInputBackend {
    // 通过 windows crate / bindgen / GDK import 初始化
    // 具体类型由 feature gate 隐藏
    inner: GameInputInner,
}

impl InputBackend for GameInputBackend {
    fn kind(&self) -> InputBackendKind {
        InputBackendKind::WindowsGameInput
    }

    fn poll(&mut self, now_ns: u64, sink: &mut dyn InputEventSink) {
        // 读取 gamepad reading / keyboard reading / raw device reading
        // 转为 RawGamepadState 或 RawKeyboardState
        // 再进入 canonical mapping
    }
}
```

#### 推荐落地顺序

```text
MVP: 先不强依赖 GameInput
M4/M5: Windows build 先用 gilrs + explicit XInput
M6: 增加 RawInput fallback
M7: 增加 optional GameInput backend
M8: 对比延迟、热插拔、rumble、focus 行为
```

### 10.8 Windows RawInputBackend

Raw Input 适合作为 generic HID gamepad / joystick fallback。Microsoft 示例显示，可以注册 HID usage page `0x01` 下的 gamepad usage `0x05` 和 joystick usage `0x04`，通过 `WM_INPUT` 和 `GetRawInputData` 读取原始输入。[^rawinput-using]

#### 适用场景

```text
非 XInput 手柄
旧式 USB joystick
街机摇杆 / 自定义控制器
需要自定义 mapping 的 HID 设备
需要避免 gilrs 覆盖不足的设备
```

#### 与 winit 的关系

RawInputBackend 需要 Windows HWND 与消息处理。实现上有两种方式：

1. 使用 `winit` 暴露的 Windows 平台扩展拿到 HWND，并在自定义 window proc 中处理 `WM_INPUT`。
2. 创建隐藏 helper window，只处理 raw input。

推荐：

```text
native desktop app: 绑定主 winit window HWND
headless/debug tool: 可选 hidden window
```

#### RawInputBackend 伪代码

```rust
#[cfg(target_os = "windows")]
pub struct RawInputBackend {
    hwnd: windows::Win32::Foundation::HWND,
    devices: HashMap<RawDeviceHandle, RawDeviceInfo>,
    mappings: MappingDatabase,
}

impl RawInputBackend {
    pub fn register(hwnd: HWND) -> Result<Self, InputError> {
        // RegisterRawInputDevices:
        // UsagePage = 0x01, Usage = 0x05: gamepad
        // UsagePage = 0x01, Usage = 0x04: joystick
        todo!()
    }

    pub fn handle_wm_input(&mut self, lparam: LPARAM, now_ns: u64, sink: &mut dyn InputEventSink) {
        // GetRawInputData
        // Parse HID report
        // Convert to RawGamepadState
    }
}
```

### 10.9 DirectInputBackend legacy

Microsoft DirectInput 文档说明 DirectInput 可用于 joystick 或其他 game controller，但不推荐用于 keyboard/mouse；现代 Windows Store apps 不支持 DirectInput。[^directinput]

#### 使用策略

```text
默认不启用
仅作为 legacy controller fallback
必须和 XInput side-by-side 去重
必须要求用户手动启用或通过 Profile 启用
```

去重规则：

```text
如果一个设备已被 XInputBackend 识别为 XUSB / XInput controller，
则 DirectInputBackend 不应再次上报同一物理设备。
```

这点很重要，因为 Microsoft 文档说明 XInput 设备会同时显示为 XInput 和 DirectInput 设备，如果 side-by-side 支持，就需要识别并过滤重复枚举项。[^xinput-directinput]

### 10.10 gilrs 作为默认跨平台层

gilrs 的优势：

```text
统一 gamepad layout
SDL-compatible mappings
hotplugging
force feedback / rumble
power information
Linux/BSD evdev
Windows WGI 或 XInput feature
WASM 支持
```

gilrs 文档列出支持输入、热插拔和 force feedback；Windows 上默认使用 Windows Gaming Input，如果需要 XInput，可以关闭默认 feature 并启用 `xinput` feature。[^gilrs]

推荐 `Cargo.toml`：

```toml
[dependencies]
gilrs = { version = "0.11", default-features = true, optional = true }
rusty_xinput = { version = "1", optional = true }
hidapi = { version = "2", optional = true }
windows = { version = "0.62", optional = true, features = [
    "Win32_Foundation",
    "Win32_UI_Input",
    "Win32_UI_Input_XboxController",
    "Win32_UI_Input_KeyboardAndMouse",
    "Win32_UI_WindowsAndMessaging",
    "Win32_Devices_HumanInterfaceDevice",
] }
```

如果明确想让 gilrs 使用 XInput，而不是默认 WGI：

```toml
[target.'cfg(windows)'.dependencies]
gilrs = { version = "0.11", default-features = false, features = ["xinput"] }
```

如果使用 gilrs 默认 WGI：

```toml
[target.'cfg(windows)'.dependencies]
gilrs = { version = "0.11", default-features = true }
```

### 10.11 SDL_GameControllerDB / mapping database

SDL_GameControllerDB 是社区维护的手柄映射数据库，可用于 SDL2/SDL3 Game Controller/Gamepad 功能。[^sdl-db] gilrs 也使用 SDL-compatible controller mappings，并支持 `SDL_GAMECONTROLLERCONFIG` 环境变量。[^gilrs]

框架应维护一个 mapping database 层：

```rust
pub struct MappingDatabase {
    pub built_in: Vec<ControllerMapping>,
    pub user_overrides: Vec<ControllerMapping>,
    pub sdl_gamecontrollerdb: Option<SdlControllerDb>,
}
```

加载顺序：

```text
1. 用户 Profile mapping
2. 用户配置目录 mapping
3. SDL_GAMECONTROLLERCONFIG 环境变量
4. bundled gamecontrollerdb.txt
5. backend-provided canonical layout
6. fallback learning wizard
```

### 10.12 NES 输入确定性

NES 游戏逻辑需要确定输入采样点。推荐规则：

```text
每帧 NMI 前 poll native input
在 NES controller strobe 写入时 latch NesControllerState
CPU 后续读 $4016/$4017 时读 shift register
```

实现：

```rust
pub struct NesControllerPort {
    current: NesControllerState,
    latched: u8,
    shift_index: u8,
    strobe: bool,
}

impl NesControllerPort {
    pub fn set_current(&mut self, state: NesControllerState) {
        self.current = sanitize_opposites(state);
    }

    pub fn write_strobe(&mut self, value: u8) {
        let new_strobe = value & 1 != 0;
        self.strobe = new_strobe;
        if new_strobe {
            self.latched = encode_nes_controller(self.current);
            self.shift_index = 0;
        }
    }

    pub fn read(&mut self) -> u8 {
        if self.strobe {
            return encode_nes_controller(self.current) & 1;
        }
        let bit = (self.latched >> self.shift_index.min(7)) & 1;
        self.shift_index = self.shift_index.saturating_add(1);
        bit
    }
}
```

### 10.13 相反方向策略

为了避免现代键盘/手柄同时输入 `Left+Right` 或 `Up+Down` 导致原版代码出现不可预期行为，默认策略：

```text
left + right → neutral
up + down    → neutral
```

可配置：

```ron
opposite_direction_policy: "neutralize" # neutralize | last_input_wins | allow
```

### 10.14 输入 Replay 格式

```ron
(
    version: 1,
    fps: 60,
    frames: [
        (frame: 0, port1: ["start"]),
        (frame: 120, port1: ["up"]),
        (frame: 121, port1: ["up", "a"]),
        (frame: 122, port1: ["a"]),
    ]
)
```

ReplayBackend：

```rust
pub struct ReplayBackend {
    replay: InputReplay,
    current_frame: u64,
}

impl ReplayBackend {
    pub fn state_for_frame(&self, frame: u64, port: u8) -> NesControllerState {
        // deterministic lookup
        todo!()
    }
}
```

### 10.15 输入调试工具

新增 CLI：

```bash
nptk-input-test --backend auto
nptk-input-test --backend gilrs
nptk-input-test --backend xinput
nptk-input-test --backend rawinput
nptk-input-test --backend gameinput
nptk-input-test --record input.ron
nptk-input-test --mapping-wizard profiles/battle_city/input.ron
```

Debug UI：

```text
Connected devices
Backend kind
Raw buttons / axes
Canonical gamepad state
NES port 1 / port 2 state
Controller shift register
Rumble test
Input latency estimate
Device duplicate detection
```

---

## 11. Battle City 默认实现路线

### 阶段 A：ROM 识别与资源导出

目标：

```text
读取用户提供的 Battle City ROM
验证 iNES/NES2 header
验证 mapper/prg/chr/mirroring
可选验证 CRC
导出 CHR atlas
生成 palette preview
```

CLI：

```bash
nptk-port inspect --profile profiles/battle_city/profile.toml --rom ./BattleCity.nes
nptk-dump-chr --rom ./BattleCity.nes --out ./target/battle_city/chr.png
```

### 阶段 B：兼容运行

目标：

```text
Mapper0 可用
6502 reference interpreter 可用
PPU compat 至少能渲染背景与 sprite
Controller port 1/2 可用
Battle City 可进入标题画面
```

### 阶段 C：Trace 与 Golden Test

目标：

```text
记录 CPU trace
记录每帧 RAM/OAM/PPU 摘要
记录 input replay
记录 framebuffer hash
建立 golden tests
```

### 阶段 D：静态重编译 MVP

目标：

```text
读取 PRG-ROM
识别 Reset/NMI/IRQ 入口
生成 CFG
生成 Rust basic blocks
支持 JSR/RTS/JMP/branch
支持 Zero Page/Absolute/Indexed/Indirect addressing
支持 NMI
支持 unknown indirect jump dispatcher
```

验收：

```text
recompiled-compat 与 compat-interpreter 在同一 input replay 下：
- 每帧 PC/A/X/Y/P/SP 可对照
- RAM snapshot hash 一致
- OAM hash 一致
- framebuffer hash 接近或一致
```

### 阶段 E：Battle City 原生渲染

目标：

```text
CHR-ROM → texture atlas
Nametable → WGPU tilemap
OAM → WGPU sprites
Palette RAM → shader palette
HUD / stage / lives 正确
坦克、子弹、爆炸 sprite 正确
```

### 阶段 F：Battle City 输入完整化

目标：

```text
Keyboard 可玩
gilrs gamepad 可玩
Windows XInput 可玩
Windows RawInput fallback 可识别 generic HID controller
Input replay 可复现
Debug UI 可查看 NES port state
```

测试矩阵：

| 设备 | Windows gilrs WGI | Windows XInput | RawInput | Linux gilrs evdev | Web Gamepad |
|---|---:|---:|---:|---:|---:|
| Xbox Series Controller | 必测 | 必测 | 可选 | 必测 | 可选 |
| Xbox 360 Controller | 必测 | 必测 | 可选 | 必测 | 可选 |
| DualShock 4 | 必测 | 可选 | 必测 | 必测 | 可选 |
| DualSense | 必测 | 可选 | 必测 | 必测 | 可选 |
| Switch Pro Controller | 必测 | 可选 | 必测 | 必测 | 可选 |
| 通用 USB HID 手柄 | 可选 | 不适用 | 必测 | 必测 | 可选 |
| 键盘 | 必测 | 不适用 | 不适用 | 必测 | 必测 |

### 阶段 G：语义化状态

目标：

```text
把关键 RAM 地址映射成 GameState 字段
把关键函数地址命名
把关卡、敌人、玩家、子弹、道具状态可视化
```

初始状态：

```rust
pub struct BattleCityStateView<'a> {
    pub lives: u8,
    pub stage_counter: u8,
    pub current_tank_state: u8,
    pub shield_status: u8,
    pub current_block_type: u8,
    pub raw_ram: &'a [u8; 0x800],
}
```

---

## 12. CLI 设计

主命令：

```bash
nptk-port inspect --rom game.nes --profile profiles/battle_city/profile.toml
nptk-port run --rom game.nes --profile profiles/battle_city/profile.toml --mode compat-interpreter
nptk-port run --rom game.nes --profile profiles/battle_city/profile.toml --mode recompiled-compat
nptk-port run --rom game.nes --profile profiles/battle_city/profile.toml --mode native-port

nptk-port trace --rom game.nes --profile profiles/battle_city/profile.toml --input replay.ron
nptk-port recompile --rom game.nes --profile profiles/battle_city/profile.toml --out generated/battle_city
nptk-port dump-chr --rom game.nes --out target/chr.png
nptk-port golden --rom game.nes --profile profiles/battle_city/profile.toml --input replay.ron

nptk-input-test --backend auto
nptk-input-test --backend gilrs
nptk-input-test --backend xinput
nptk-input-test --backend rawinput
nptk-input-test --record replay.ron
nptk-input-test --mapping-wizard profiles/battle_city/input.ron
```

---

## 13. 测试计划

### 13.1 CPU 测试

```text
nestest
寻址模式测试
flag 行为测试
NMI/IRQ/BRK/RTI 测试
stack/JSR/RTS 测试
illegal opcode 策略测试
```

NESdev 汇总了大量 emulator tests，`nestest` 是 CPU 测试常见起点。[^emulator-tests]

### 13.2 PPU 测试

```text
PPU register latch
$2005/$2006/$2007 行为
nametable mirroring
palette mirroring
OAM DMA
sprite priority
sprite flip
sprite overflow 标志策略
```

### 13.3 APU 测试

```text
pulse/noise/triangle channel smoke test
frame counter timing
length counter / envelope
PCM ring buffer underrun
CPAL latency
```

### 13.4 Recompiler 测试

```text
每条 6502 指令 IR lift snapshot
basic block codegen snapshot
CFG snapshot
unknown indirect jump dispatcher test
interpreter vs recompiled register trace
interpreter vs recompiled RAM hash
```

### 13.5 输入测试

```text
Keyboard mapping test
XInput slot 0..3 poll test
XInput rumble test
XInput disconnect/reconnect test
GameInput optional smoke test
RawInput WM_INPUT parse test
DirectInput legacy enumeration test
Duplicate device filtering test
SDL mapping database test
Analog deadzone/hysteresis test
Opposite direction policy test
Input replay deterministic test
Controller strobe/shift register test
```

### 13.6 Battle City Golden Test

```text
标题画面 hash
开始游戏后第 N 帧 RAM hash
玩家移动 replay
玩家射击 replay
敌人生成 replay
砖块破坏 replay
道具出现 replay
Game Over replay
Stage clear replay
```

---

## 14. 里程碑

### M0：工程骨架

交付物：

```text
Rust workspace
clap CLI
tracing 日志
profile loader
ROM loader
wgpu window skeleton
nptk-input crate skeleton
CI 基础测试
```

### M1：ROM / Mapper / CPU Reference

交付物：

```text
iNES / NES 2.0 parser
Mapper0 / NROM
6502 reference interpreter
NesBus
Controller shift register
Battle City inspect 成功
```

### M2：输入系统 MVP

交付物：

```text
winit keyboard backend
gilrs backend
NES port mapping
Input replay
nptk-input-test
Battle City keyboard/gamepad 可操作标题菜单
```

### M3：PPU Compat 与基础画面

交付物：

```text
PPU register bridge
nametable/palette/OAM 基础支持
framebuffer renderer
CHR dump
Battle City 标题画面
```

### M4：Windows XInput 后端

交付物：

```text
rusty_xinput 或 windows-crate XInputBackend
slot 0..3 轮询
rumble
hotplug/disconnect 检测
XInput → Canonical → NES mapping
Windows XInput golden input test
```

### M5：Trace 与 Debugger

交付物：

```text
CPU trace
RAM/OAM/PPU snapshot
frame hash
input replay
input inspector
eGUI debug overlay
FCEUX/Mesen trace 对照导入
```

### M6：Static Recompiler MVP

交付物：

```text
6502 disasm
CFG builder
IR6502
Rust codegen
generated crate
Battle City recompiled-compat 跑通标题画面
```

### M7：RawInput / HID fallback

交付物：

```text
Windows RawInputBackend
HID gamepad/joystick registration
custom mapping wizard
SDL_GameControllerDB integration
XInput/RawInput duplicate filtering
```

### M8：Battle City 可玩

交付物：

```text
Battle City recompiled-compat 可完整游玩
Keyboard/gilrs/XInput 输入正确
音频 compat 输出
Golden tests 覆盖主要玩法
```

### M9：原生渲染

交付物：

```text
CHR atlas renderer
tilemap renderer
sprite renderer
palette shader
Battle City native-port 可玩
compat/native 画面对照工具
```

### M10：GameInput optional backend

交付物：

```text
Windows GameInputBackend optional feature
NuGet/GDK 集成文档
polling/callback smoke test
与 gilrs/XInput/RawInput 行为对照
```

### M11：原生音频与语义化状态

交付物：

```text
Kira SFX hook
GameState view
RAM symbol inspector
function symbol map
Battle City debug tools
```

### M12：框架泛化验证

交付物：

```text
第二个 NROM homebrew / test ROM Profile
不改框架代码即可接入
Profile 格式文档
Runtime ABI 文档
Input backends 文档
Recompiler 文档
```

---

## 15. 风险与处理策略

| 风险 | 处理 |
|---|---|
| 静态分析误把数据当代码 | 静态分析 + runtime trace + Profile annotations |
| 间接跳转目标不完整 | jump table 识别 + trace 收集 + dispatcher fallback |
| PPU 时序不准 | 保留 compat mode；以 golden frame 和 PPU tests 锁定 |
| 原生渲染和 PPU 输出不一致 | native renderer 与 compat framebuffer 并排对照 |
| APU 音频失真或延迟 | 先 CPAL 输出 APU PCM，再逐步 Kira 原生音频 |
| Windows 输入设备重复上报 | XInput / DirectInput / RawInput device de-duplication |
| GameInput 集成复杂 | 作为 optional feature，不阻塞 MVP |
| RawInput mapping 复杂 | SDL_GameControllerDB + mapping wizard + 用户覆盖配置 |
| gilrs WGI 需要焦点窗口 | 提供 explicit XInput fallback；文档说明 backend 差异 |
| 依赖许可证不合适 | 主链路只选 permissive 依赖；非商业许可证库只作参考 |
| 框架变成 Battle City 专用 | Profile 驱动；第二个 NROM 测试 ROM 作为泛化验收 |
| 分发版权资源风险 | 不提交 ROM/CHR/音频 dump；只提交工具、Profile、符号和补丁 |

---

## 16. Definition of Done

第一版完成标准：

```text
1. 用户提供 Battle City ROM 后，CLI 能识别 ROM 并验证 Profile。
2. compat-interpreter 模式可运行 Battle City。
3. recompiled-compat 模式可运行 Battle City，且关键 replay 的 RAM/OAM/frame hash 与基准一致。
4. native-port 模式使用 WGPU tilemap/sprite 渲染，而不是只上传模拟器 framebuffer。
5. 输入支持 keyboard + gilrs gamepad + Windows XInput。
6. 输入 replay 可确定性复现。
7. Windows RawInput fallback 至少可识别 generic HID gamepad/joystick。
8. 音频至少支持 APU compat PCM 输出。
9. Battle City 的 lives、stage、tank state、shield、block type 等关键 RAM 状态可在 debug UI 中查看。
10. 所有生成代码可复现。
11. 仓库不包含商业 ROM 或原始版权资源。
12. 第二个 Mapper0 测试 ROM 可通过新增 Profile 接入，证明框架不是 Battle City 专用。
```

---

## 17. 推荐开发顺序

```text
1. nptk-core: ROM parser + Mapper0 + NesBus
2. nptk-profile: GameProfile + Battle City profile
3. nptk-input: backend trait + keyboard + NES controller shift register
4. nptk-tools: inspect / dump-chr / input-test
5. nptk-core: CPU reference interpreter
6. nptk-wgpu: framebuffer renderer
7. nptk-input: gilrs backend + mapping + replay
8. nptk-core: PPU compat 最小实现
9. nptk-input: Windows XInputBackend
10. nptk-tools: trace / golden
11. nptk-recompiler: disasm + CFG + IR6502
12. nptk-recompiler: Rust codegen
13. nptk-native-runtime: Runtime ABI
14. nptk-input: RawInput/HID fallback + duplicate filtering
15. nptk-battle-city: 默认 Profile hooks
16. nptk-wgpu: tilemap/sprite native renderer
17. nptk-audio: CPAL APU output
18. nptk-audio: Kira native SFX hooks
19. nptk-input: optional GameInput backend
20. docs: Profile / Runtime / Recompiler / Input Backends 文档
```

---

## 18. 附录：资料来源

[^battle-city]: Data Crystal, “Battle City (NES)”, mapper、PRG/CHR、mirroring、SRAM 信息：<https://datacrystal.tcrf.net/wiki/Battle_City_%28NES%29>

[^battle-city-ram]: Data Crystal, “Battle City (NES)/RAM map”：<https://datacrystal.tcrf.net/wiki/Battle_City_%28NES%29/RAM_map>

[^cpu-map]: NESdev Wiki, “CPU memory map”：<https://www.nesdev.org/wiki/CPU_memory_map>

[^ppu-registers]: NESdev Wiki, “PPU registers”：<https://www.nesdev.org/wiki/PPU_registers>

[^ppu-memory]: NESdev Wiki, “PPU memory map”：<https://www.nesdev.org/wiki/PPU_memory_map>

[^oam]: NESdev Wiki, “PPU OAM”：<https://www.nesdev.org/wiki/PPU_OAM>

[^nes20]: NESdev Wiki, “NES 2.0”：<https://www.nesdev.org/wiki/NES_2.0>

[^nrom]: NESdev Wiki, “NROM”：<https://www.nesdev.org/wiki/NROM>

[^apu-frame]: NESdev Wiki, “APU Frame Counter”：<https://www.nesdev.org/wiki/APU_Frame_Counter>

[^emulator-tests]: NESdev Wiki, “Emulator tests”：<https://www.nesdev.org/wiki/Emulator_tests>

[^wgpu]: docs.rs, “wgpu”：<https://docs.rs/wgpu/>

[^winit]: docs.rs, “winit”：<https://docs.rs/winit/>

[^cpal]: docs.rs, “cpal”：<https://docs.rs/cpal/>

[^kira]: docs.rs, “kira”：<https://docs.rs/kira/>

[^disasm6502]: docs.rs, “disasm6502”：<https://docs.rs/disasm6502>

[^gilrs]: docs.rs, “gilrs”：<https://docs.rs/gilrs/>

[^rusty-xinput]: docs.rs, “rusty_xinput”：<https://docs.rs/rusty-xinput>

[^windows-crate]: Microsoft Learn, “Rust for Windows, and the windows crate”：<https://learn.microsoft.com/en-us/windows/dev-environment/rust/rust-for-windows>

[^gameinput]: Microsoft Learn, “GameInput introduction”：<https://learn.microsoft.com/en-us/gaming/gdk/docs/features/common/input/overviews/input-overview>

[^xinput-directinput]: Microsoft Learn, “Comparison of XInput and DirectInput features”：<https://learn.microsoft.com/en-us/windows/win32/xinput/xinput-and-directinput>

[^rawinput-using]: Microsoft Learn, “Using Raw Input”：<https://learn.microsoft.com/en-us/windows/win32/inputdev/using-raw-input>

[^directinput]: Microsoft Learn, “DirectInput”：<https://learn.microsoft.com/en-us/previous-versions/windows/desktop/ee416842%28v%3Dvs.85%29>

[^sdl-db]: GitHub, “SDL_GameControllerDB”：<https://github.com/mdqinc/SDL_GameControllerDB>

[^web-gamepad]: MDN Web Docs, “Using the Gamepad API”：<https://developer.mozilla.org/en-US/docs/Web/API/Gamepad_API/Using_the_Gamepad_API>
