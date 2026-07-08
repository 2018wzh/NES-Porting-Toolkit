# FC/NES 重编译器中的卡带芯片接口设计

面向项目：**FC/NES Native Port Framework / Rust + WGPU 原生移植框架**  
默认目标游戏：**Battle City / 坦克大战，Mapper 0 / NROM**  
文档目标：设计一套可扩展的卡带芯片接口，使重编译后的 6502 代码可以统一访问不同 FC/NES 卡带内置芯片，包括 NROM、UxROM、CNROM、MMC1、MMC3、MMC5、VRC 系列、Namco 163、Sunsoft 5B、FDS 等。

---

## 1. 设计背景

FC/NES 的卡带不是一块单纯的 ROM。很多卡带内部包含额外电路或 ASIC，通常称为 **Mapper** 或卡带芯片。这些芯片可能提供：

- PRG-ROM bank switching
- CHR-ROM / CHR-RAM bank switching
- PRG-RAM / SRAM
- Nametable mirroring 控制
- Scanline / IRQ 计数器
- 扩展音频
- 扩展 VRAM / ExRAM
- 乘法器等辅助寄存器
- FDS 磁碟状态与音频

因此，在重编译器中不应把卡带当成固定 ROM 数组，而应该把它建模为挂在 CPU 总线和 PPU 总线上的可编程外设。

---

## 2. 核心原则

### 2.1 重编译代码不直接感知具体 Mapper

重编译后的 6502 代码不应该生成针对 MMC1、MMC3、VRC6 等芯片的特殊逻辑。

不推荐：

```rust
if mapper_id == 4 {
    mmc3_write(addr, value);
}
```

推荐：

```rust
rt.cpu_write(0x8000, cpu.a);
let value = rt.cpu_read(0xC000);
```

具体的 Mapper 行为由运行时的 `Cartridge` 和 `MapperChip` 实现完成。

---

### 2.2 Mapper 属于 Cartridge，不属于 CPU 或 PPU

卡带芯片同时影响 CPU 访问 PRG 区域和 PPU 访问 CHR 区域，因此 Mapper 应挂在 `Cartridge` 层。

```text
Recompiled 6502 Code
    ↓ read8/write8/tick
RuntimeBus
    ↓
Cartridge
    ↓
MapperChip
    ↓
PRG-ROM / CHR-ROM / CHR-RAM / PRG-RAM / IRQ / Expansion Audio
```

---

### 2.3 Mapper 必须同时处理 CPU 总线和 PPU 总线

CPU 侧典型范围：

```text
$4020-$5FFF   扩展寄存器 / 卡带扩展区域
$6000-$7FFF   PRG-RAM / SRAM / Mapper RAM
$8000-$FFFF   PRG-ROM / Mapper 寄存器
```

PPU 侧典型范围：

```text
$0000-$1FFF   CHR-ROM / CHR-RAM / CHR bank
$2000-$2FFF   Nametable / Mirroring / Mapper-controlled VRAM
```

CPU 写 Mapper 寄存器可能影响后续 PPU 读到的 CHR bank，因此 Mapper 状态必须在 CPU/PPU 间共享。

---

## 3. 总体架构

```text
┌────────────────────────────┐
│ Recompiled 6502 Blocks      │
│ read8 / write8 / tick       │
└──────────────┬─────────────┘
               │
               ▼
┌────────────────────────────┐
│ RuntimeAbi                  │
│ cpu_read / cpu_write        │
│ advance_cpu_cycles          │
│ poll_irq / poll_nmi         │
└──────────────┬─────────────┘
               │
               ▼
┌────────────────────────────┐
│ NesRuntime / Bus            │
│ CPU RAM / PPU / APU / IO    │
└──────────────┬─────────────┘
               │
               ▼
┌────────────────────────────┐
│ Cartridge                   │
│ PRG / CHR / PRG-RAM         │
└──────────────┬─────────────┘
               │
               ▼
┌────────────────────────────┐
│ MapperChip                  │
│ cpu_read / cpu_write        │
│ ppu_read / ppu_write        │
│ cpu_tick / ppu_tick         │
│ irq / mirroring / audio     │
└────────────────────────────┘
```

---

## 4. 运行时 ABI 设计

重编译器只生成对 `RuntimeAbi` 的调用。

```rust
pub trait RuntimeAbi {
    fn cpu_read(&mut self, addr: u16) -> u8;
    fn cpu_write(&mut self, addr: u16, value: u8);

    fn advance_cpu_cycles(&mut self, cycles: u32);

    fn poll_nmi(&mut self) -> bool;
    fn poll_irq(&mut self) -> bool;

    fn push_trace_event(&mut self, event: TraceEvent);
}
```

生成代码示例：

```rust
pub fn block_80a2<R: RuntimeAbi>(
    rt: &mut R,
    cpu: &mut CpuState,
) -> BlockExit {
    cpu.a = rt.cpu_read(0x8000);
    cpu.set_zn(cpu.a);
    rt.advance_cpu_cycles(4);

    BlockExit::Next(BlockId::B80A6)
}
```

这使重编译器只负责 6502 指令语义，卡带芯片行为由运行时处理。

---

## 5. Cartridge 数据结构

```rust
pub struct Cartridge {
    pub metadata: CartridgeMetadata,
    pub prg_rom: Vec<u8>,
    pub chr: ChrStorage,
    pub prg_ram: PrgRam,
    pub mapper: Box<dyn MapperChip>,
}
```

CHR 存储需要区分 ROM 和 RAM：

```rust
pub enum ChrStorage {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}
```

PRG-RAM 可以建模为：

```rust
pub struct PrgRam {
    pub data: Vec<u8>,
    pub battery_backed: bool,
    pub writable: bool,
}
```

---

## 6. MapperContext

`MapperContext` 用于向 Mapper 提供它需要访问的存储和运行环境，避免 Mapper 直接持有整个模拟器。

```rust
pub struct MapperContext<'a> {
    pub prg_rom: &'a [u8],
    pub chr: &'a mut ChrStorage,
    pub prg_ram: &'a mut PrgRam,

    pub open_bus: u8,
    pub region: NesRegion,

    pub event_sink: &'a mut CartridgeEventSink,
}
```

区域类型：

```rust
pub enum NesRegion {
    Ntsc,
    Pal,
    Dendy,
}
```

---

## 7. MapperChip 主接口

```rust
pub trait MapperChip {
    fn mapper_id(&self) -> u16;
    fn name(&self) -> &'static str;

    // CPU side
    fn cpu_read(
        &mut self,
        ctx: &mut MapperContext,
        addr: u16,
    ) -> Option<u8>;

    fn cpu_write(
        &mut self,
        ctx: &mut MapperContext,
        addr: u16,
        value: u8,
    ) -> bool;

    // PPU side
    fn ppu_read(
        &mut self,
        ctx: &mut MapperContext,
        addr: u16,
    ) -> Option<u8>;

    fn ppu_write(
        &mut self,
        ctx: &mut MapperContext,
        addr: u16,
        value: u8,
    ) -> bool;

    // timing
    fn cpu_tick(
        &mut self,
        ctx: &mut MapperContext,
        cycles: u32,
    );

    fn ppu_tick(
        &mut self,
        ctx: &mut MapperContext,
        event: PpuBusEvent,
    );

    // IRQ
    fn irq_state(&self) -> IrqState;
    fn clear_irq(&mut self);

    // nametable mirroring
    fn mirroring(&self) -> Mirroring;

    // optional expansion audio
    fn expansion_audio(&mut self) -> Option<&mut dyn ExpansionAudio> {
        None
    }

    // save/load state
    fn save_state(&self) -> MapperSaveState;
    fn load_state(&mut self, state: &MapperSaveState);

    // debug
    fn debug_info(&self) -> MapperDebugInfo {
        MapperDebugInfo::default()
    }
}
```

---

## 8. 地址映射结果设计

对于简单 Mapper，可以把“地址翻译”和“芯片副作用”拆开。

```rust
pub enum CpuMapResult {
    PrgRom { offset: usize },
    PrgRam { offset: usize },
    MapperRegister { value: u8 },
    OpenBus,
    NotMapped,
}

pub enum CpuWriteAction {
    WritePrgRam { offset: usize, value: u8 },
    UpdateRegister,
    Ignore,
    NotMapped,
}

pub enum PpuMapResult {
    ChrRom { offset: usize },
    ChrRam { offset: usize },
    Vram { nametable: usize, offset: usize },
    Palette,
    NotMapped,
}

pub enum PpuWriteAction {
    WriteChrRam { offset: usize, value: u8 },
    WriteVram { nametable: usize, offset: usize, value: u8 },
    Ignore,
    NotMapped,
}
```

可选辅助 trait：

```rust
pub trait AddressMapper {
    fn map_cpu_read(&self, addr: u16) -> CpuMapResult;
    fn map_cpu_write(&self, addr: u16, value: u8) -> CpuWriteAction;

    fn map_ppu_read(&self, addr: u16) -> PpuMapResult;
    fn map_ppu_write(&self, addr: u16, value: u8) -> PpuWriteAction;
}
```

---

## 9. Bus 如何调用 Mapper

### 9.1 CPU read

```rust
pub fn cpu_read(&mut self, addr: u16) -> u8 {
    match addr {
        0x0000..=0x1FFF => {
            self.cpu_ram[(addr as usize) & 0x07FF]
        }

        0x2000..=0x3FFF => {
            self.ppu.cpu_read_register(0x2000 + (addr & 7))
        }

        0x4000..=0x4017 => {
            self.apu_io_read(addr)
        }

        0x4020..=0xFFFF => {
            self.cartridge
                .cpu_read(addr)
                .unwrap_or(self.open_bus)
        }
    }
}
```

### 9.2 CPU write

```rust
pub fn cpu_write(&mut self, addr: u16, value: u8) {
    match addr {
        0x0000..=0x1FFF => {
            self.cpu_ram[(addr as usize) & 0x07FF] = value;
        }

        0x2000..=0x3FFF => {
            self.ppu.cpu_write_register(0x2000 + (addr & 7), value);
        }

        0x4000..=0x4017 => {
            self.apu_io_write(addr, value);
        }

        0x4020..=0xFFFF => {
            self.cartridge.cpu_write(addr, value);
        }
    }
}
```

### 9.3 PPU read/write

```rust
pub fn ppu_read(&mut self, addr: u16) -> u8 {
    let addr = addr & 0x3FFF;

    if let Some(value) = self.cartridge.ppu_read(addr) {
        return value;
    }

    match addr {
        0x2000..=0x2FFF => self.vram_read(addr),
        0x3F00..=0x3FFF => self.palette_read(addr),
        _ => self.open_bus,
    }
}

pub fn ppu_write(&mut self, addr: u16, value: u8) {
    let addr = addr & 0x3FFF;

    if self.cartridge.ppu_write(addr, value) {
        return;
    }

    match addr {
        0x2000..=0x2FFF => self.vram_write(addr, value),
        0x3F00..=0x3FFF => self.palette_write(addr, value),
        _ => {}
    }
}
```

---

## 10. 时钟推进与 IRQ

`advance_cpu_cycles()` 需要推进 CPU、PPU、APU 和 Mapper。

```rust
impl RuntimeAbi for NesRuntime {
    fn advance_cpu_cycles(&mut self, cycles: u32) {
        for _ in 0..cycles {
            self.apu.clock_cpu();

            for _ in 0..3 {
                let event = self.ppu.clock();
                self.cartridge.ppu_tick(event);
            }

            self.cartridge.cpu_tick(1);

            if self.cartridge.irq_state().is_active() {
                self.cpu_irq_line = true;
            }
        }
    }
}
```

这对 MMC3、VRC、FDS、扩展音频等芯片尤其重要。

---

## 11. PPUBusEvent

一些 Mapper 需要观察 PPU 地址线，尤其是 MMC3 的 A12 上升沿计数。

```rust
pub struct PpuBusEvent {
    pub frame: u64,
    pub scanline: i16,
    pub dot: u16,
    pub addr: u16,
    pub access: PpuAccessKind,
}

pub enum PpuAccessKind {
    Read,
    Write,
    Idle,
}
```

---

## 12. IRQ 状态

```rust
pub enum IrqState {
    Inactive,
    Active,
}

impl IrqState {
    pub fn is_active(&self) -> bool {
        matches!(self, IrqState::Active)
    }
}
```

Mapper 触发 IRQ 时不直接操作 CPU，而是暴露状态：

```rust
fn irq_state(&self) -> IrqState;
fn clear_irq(&mut self);
```

CPU/Runtime 在合适时机读取该状态并拉起 IRQ line。

---

## 13. Mirroring

```rust
pub enum Mirroring {
    Horizontal,
    Vertical,
    SingleScreenLower,
    SingleScreenUpper,
    FourScreen,
    MapperControlled,
}
```

Nametable 地址映射示例：

```rust
pub fn map_nametable_addr(addr: u16, mirroring: Mirroring) -> usize {
    let index = (addr - 0x2000) as usize & 0x0FFF;
    let table = index / 0x0400;
    let offset = index & 0x03FF;

    let physical_table = match mirroring {
        Mirroring::Vertical => match table {
            0 | 2 => 0,
            1 | 3 => 1,
            _ => unreachable!(),
        },

        Mirroring::Horizontal => match table {
            0 | 1 => 0,
            2 | 3 => 1,
            _ => unreachable!(),
        },

        Mirroring::SingleScreenLower => 0,
        Mirroring::SingleScreenUpper => 1,
        Mirroring::FourScreen => table,
        Mirroring::MapperControlled => table,
    };

    physical_table * 0x0400 + offset
}
```

---

## 14. NROM / Mapper 0 示例

NROM 是最简单的 Mapper，适合 Battle City 默认实现。

```rust
pub struct Mapper000Nrom {
    mirroring: Mirroring,
}

impl MapperChip for Mapper000Nrom {
    fn mapper_id(&self) -> u16 {
        0
    }

    fn name(&self) -> &'static str {
        "NROM"
    }

    fn cpu_read(
        &mut self,
        ctx: &mut MapperContext,
        addr: u16,
    ) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => {
                let mut offset = (addr - 0x8000) as usize;

                if ctx.prg_rom.len() == 0x4000 {
                    offset &= 0x3FFF;
                }

                Some(ctx.prg_rom[offset])
            }
            _ => None,
        }
    }

    fn cpu_write(
        &mut self,
        _ctx: &mut MapperContext,
        _addr: u16,
        _value: u8,
    ) -> bool {
        false
    }

    fn ppu_read(
        &mut self,
        ctx: &mut MapperContext,
        addr: u16,
    ) -> Option<u8> {
        match addr {
            0x0000..=0x1FFF => match &ctx.chr {
                ChrStorage::Rom(chr) => Some(chr[addr as usize]),
                ChrStorage::Ram(chr) => Some(chr[addr as usize]),
            },
            _ => None,
        }
    }

    fn ppu_write(
        &mut self,
        ctx: &mut MapperContext,
        addr: u16,
        value: u8,
    ) -> bool {
        match addr {
            0x0000..=0x1FFF => match &mut ctx.chr {
                ChrStorage::Ram(chr) => {
                    chr[addr as usize] = value;
                    true
                }
                ChrStorage::Rom(_) => false,
            },
            _ => false,
        }
    }

    fn cpu_tick(&mut self, _ctx: &mut MapperContext, _cycles: u32) {}

    fn ppu_tick(&mut self, _ctx: &mut MapperContext, _event: PpuBusEvent) {}

    fn irq_state(&self) -> IrqState {
        IrqState::Inactive
    }

    fn clear_irq(&mut self) {}

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> MapperSaveState {
        MapperSaveState::Nrom
    }

    fn load_state(&mut self, _state: &MapperSaveState) {}
}
```

---

## 15. UxROM / Mapper 2 示例

UxROM 主要提供 16 KiB PRG bank switching。

```rust
pub struct Mapper002Uxrom {
    selected_prg_bank: usize,
    prg_bank_count: usize,
    mirroring: Mirroring,
}

impl MapperChip for Mapper002Uxrom {
    fn mapper_id(&self) -> u16 {
        2
    }

    fn name(&self) -> &'static str {
        "UxROM"
    }

    fn cpu_read(
        &mut self,
        ctx: &mut MapperContext,
        addr: u16,
    ) -> Option<u8> {
        match addr {
            0x8000..=0xBFFF => {
                let bank = self.selected_prg_bank % self.prg_bank_count;
                let offset = bank * 0x4000 + (addr as usize - 0x8000);
                Some(ctx.prg_rom[offset])
            }

            0xC000..=0xFFFF => {
                let bank = self.prg_bank_count - 1;
                let offset = bank * 0x4000 + (addr as usize - 0xC000);
                Some(ctx.prg_rom[offset])
            }

            _ => None,
        }
    }

    fn cpu_write(
        &mut self,
        _ctx: &mut MapperContext,
        addr: u16,
        value: u8,
    ) -> bool {
        match addr {
            0x8000..=0xFFFF => {
                self.selected_prg_bank = value as usize;
                true
            }
            _ => false,
        }
    }

    // ppu_read / ppu_write 可按 CHR-RAM 或固定 CHR 处理。
    // 其他接口省略。
}
```

---

## 16. CNROM / Mapper 3 示例

CNROM 主要提供 8 KiB CHR bank switching。

```rust
pub struct Mapper003Cnrom {
    selected_chr_bank: usize,
    chr_bank_count: usize,
    mirroring: Mirroring,
}

impl Mapper003Cnrom {
    fn chr_offset(&self, addr: u16) -> usize {
        let bank = self.selected_chr_bank % self.chr_bank_count;
        bank * 0x2000 + addr as usize
    }
}
```

典型行为：

```text
CPU 写 $8000-$FFFF
    → 更新 selected_chr_bank

PPU 读 $0000-$1FFF
    → 根据 selected_chr_bank 读取 CHR-ROM
```

---

## 17. MMC1 / Mapper 1 示例

MMC1 使用 5-bit 串行移位寄存器配置 PRG/CHR/mirroring。

```rust
pub struct Mapper001Mmc1 {
    shift: u8,
    shift_count: u8,

    control: u8,
    chr_bank0: u8,
    chr_bank1: u8,
    prg_bank: u8,

    prg_mode: PrgMode,
    chr_mode: ChrMode,
    mirroring: Mirroring,
}

impl Mapper001Mmc1 {
    fn write_serial(&mut self, addr: u16, value: u8) {
        if value & 0x80 != 0 {
            self.shift = 0;
            self.shift_count = 0;
            self.control |= 0x0C;
            self.update_modes();
            return;
        }

        self.shift >>= 1;
        self.shift |= (value & 1) << 4;
        self.shift_count += 1;

        if self.shift_count == 5 {
            self.commit_register(addr, self.shift);
            self.shift = 0;
            self.shift_count = 0;
        }
    }

    fn commit_register(&mut self, addr: u16, value: u8) {
        match addr {
            0x8000..=0x9FFF => self.control = value,
            0xA000..=0xBFFF => self.chr_bank0 = value,
            0xC000..=0xDFFF => self.chr_bank1 = value,
            0xE000..=0xFFFF => self.prg_bank = value,
            _ => {}
        }

        self.update_modes();
    }

    fn update_modes(&mut self) {
        // 根据 control 寄存器更新 mirroring、PRG mode、CHR mode。
    }
}
```

接口层不需要特殊处理 MMC1；复杂性被封装在 `cpu_write()` 内部。

---

## 18. MMC3 / Mapper 4 示例

MMC3 的关键能力是：

- PRG bank switching
- CHR bank switching
- Mapper-controlled mirroring
- 基于 PPU A12 的 scanline IRQ

因此它需要 `ppu_tick()` 观察 PPU 访问事件。

```rust
pub struct Mapper004Mmc3 {
    bank_select: u8,
    bank_regs: [u8; 8],

    prg_mode: bool,
    chr_mode: bool,

    irq_latch: u8,
    irq_counter: u8,
    irq_reload: bool,
    irq_enabled: bool,
    irq_pending: bool,

    last_a12: bool,
    a12_low_cycles: u32,

    mirroring: Mirroring,
}
```

简化的 A12 观察逻辑：

```rust
impl Mapper004Mmc3 {
    fn observe_ppu_addr(&mut self, addr: u16) {
        let a12 = (addr & 0x1000) != 0;

        if !a12 {
            self.a12_low_cycles += 1;
        }

        if a12 && !self.last_a12 && self.a12_low_cycles >= 2 {
            self.clock_irq_counter();
        }

        if a12 {
            self.a12_low_cycles = 0;
        }

        self.last_a12 = a12;
    }

    fn clock_irq_counter(&mut self) {
        if self.irq_counter == 0 || self.irq_reload {
            self.irq_counter = self.irq_latch;
            self.irq_reload = false;
        } else {
            self.irq_counter -= 1;
        }

        if self.irq_counter == 0 && self.irq_enabled {
            self.irq_pending = true;
        }
    }
}

impl MapperChip for Mapper004Mmc3 {
    fn ppu_tick(
        &mut self,
        _ctx: &mut MapperContext,
        event: PpuBusEvent,
    ) {
        if matches!(event.access, PpuAccessKind::Read) {
            self.observe_ppu_addr(event.addr);
        }
    }

    fn irq_state(&self) -> IrqState {
        if self.irq_pending {
            IrqState::Active
        } else {
            IrqState::Inactive
        }
    }

    fn clear_irq(&mut self) {
        self.irq_pending = false;
    }

    // 其他接口省略。
}
```

---

## 19. 扩展音频接口

Famicom 卡带可能包含扩展音频，例如：

- VRC6
- VRC7
- Namco 163
- Sunsoft 5B
- MMC5 audio
- FDS audio

建议作为 Mapper 的可选子接口。

```rust
pub trait ExpansionAudio {
    fn write_audio_register(&mut self, addr: u16, value: u8);
    fn clock_cpu(&mut self, cycles: u32);
    fn sample(&mut self) -> f32;
    fn reset(&mut self);
}
```

Mapper 暴露：

```rust
fn expansion_audio(&mut self) -> Option<&mut dyn ExpansionAudio> {
    None
}
```

Runtime 混音：

```rust
pub fn mix_audio_frame(&mut self) -> f32 {
    let apu = self.apu.sample();

    let ext = self.cartridge
        .mapper
        .expansion_audio()
        .map(|audio| audio.sample())
        .unwrap_or(0.0);

    self.audio_mixer.mix(apu, ext)
}
```

---

## 20. Save State 设计

Mapper 状态必须纳入 save state。不能只保存 CPU/PPU/RAM，否则 bank 状态、IRQ 计数器、扩展音频状态都会丢失。

```rust
#[derive(Serialize, Deserialize)]
pub enum MapperSaveState {
    Nrom,

    Uxrom {
        selected_prg_bank: u8,
    },

    Cnrom {
        selected_chr_bank: u8,
    },

    Mmc1 {
        shift: u8,
        shift_count: u8,
        control: u8,
        chr_bank0: u8,
        chr_bank1: u8,
        prg_bank: u8,
    },

    Mmc3 {
        bank_select: u8,
        bank_regs: [u8; 8],
        irq_latch: u8,
        irq_counter: u8,
        irq_reload: bool,
        irq_enabled: bool,
        irq_pending: bool,
        last_a12: bool,
    },

    Custom(Vec<u8>),
}
```

---

## 21. Debug / Trace 接口

调试接口用于查看当前 bank 映射、寄存器、IRQ 状态等。

```rust
pub struct MapperDebugInfo {
    pub mapper_name: String,
    pub registers: Vec<DebugRegister>,
    pub prg_banks: Vec<DebugBankMapping>,
    pub chr_banks: Vec<DebugBankMapping>,
    pub irq: DebugIrqState,
}

pub struct DebugRegister {
    pub name: String,
    pub value: u32,
}

pub struct DebugBankMapping {
    pub cpu_or_ppu_range: String,
    pub bank: usize,
    pub offset: usize,
}

pub struct DebugIrqState {
    pub enabled: bool,
    pub pending: bool,
    pub counter: Option<u32>,
    pub latch: Option<u32>,
}
```

示例输出：

```text
Mapper: MMC3

PRG:
  $8000-$9FFF -> PRG bank 06
  $A000-$BFFF -> PRG bank 02
  $C000-$DFFF -> PRG bank FE
  $E000-$FFFF -> PRG bank FF

CHR:
  $0000-$07FF -> CHR bank 10
  $0800-$0FFF -> CHR bank 11
  $1000-$13FF -> CHR bank 22

IRQ:
  latch = 07
  counter = 03
  enabled = true
  pending = false
```

---

## 22. 原生移植事件接口

传统模拟器只需要返回字节；原生移植框架还需要把卡带行为转成资源事件，用于 WGPU 渲染和原生音频。

```rust
pub enum CartridgeEvent {
    PrgBankChanged {
        region: CpuRegion,
        bank: usize,
    },

    ChrBankChanged {
        region: PpuRegion,
        bank: usize,
    },

    MirroringChanged {
        mirroring: Mirroring,
    },

    IrqAsserted,
    IrqCleared,

    ExpansionAudioRegisterWrite {
        addr: u16,
        value: u8,
    },
}
```

用途：

```text
CHR bank changed
    → 更新 texture atlas view
    → 更新 tile material
    → 避免每帧重新解析全部 CHR

PRG bank changed
    → 记录重编译 block 所属 bank
    → 帮助动态 dispatcher 定位代码

Mirroring changed
    → 更新 nametable view
```

---

## 23. 动态分派与静态特化

### 23.1 开发/调试模式：动态分派

```rust
pub struct Cartridge {
    pub mapper: Box<dyn MapperChip>,
}
```

优点：

- 通用
- 易扩展
- 适合 CLI 工具与调试器
- 可在运行时加载不同 ROM

缺点：

- 每次 Mapper 调用有动态分派开销

---

### 23.2 发布/原生移植模式：泛型静态分派

```rust
pub struct NesRuntime<M: MapperChip> {
    pub mapper: M,
}
```

优点：

- 可内联
- 性能更好
- 适合专用游戏移植产物

缺点：

- 每个 Mapper / GameProfile 需要生成或编译专用 runtime

推荐策略：

```text
开发模式：
    Box<dyn MapperChip>

发布模式：
    NesRuntime<Mapper000Nrom>
    NesRuntime<Mapper004Mmc3>
```

Battle City 默认实现：

```rust
type BattleCityRuntime = NesRuntime<Mapper000Nrom>;
```

---

## 24. GameProfile 中的 Mapper 描述

### 24.1 Battle City / NROM

```toml
[rom]
system = "nes"
mapper = 0
mirroring = "horizontal"
prg_rom_size = 16384
chr_rom_size = 8192
prg_ram_size = 0
chr_ram_size = 0

[mapper]
kind = "NROM"
variant = "nrom-128"
irq_model = "none"

[mapper.features]
prg_bank_switching = false
chr_bank_switching = false
scanline_irq = false
mapper_controlled_mirroring = false
expansion_audio = false
```

### 24.2 MMC3 示例

```toml
[rom]
system = "nes"
mapper = 4
submapper = 0
mirroring = "mapper_controlled"
prg_rom_size = 262144
chr_rom_size = 131072
prg_ram_size = 8192
chr_ram_size = 0

[mapper]
kind = "MMC3"
irq_model = "ppu_a12"
variant = "standard"
battery = false
four_screen = false

[mapper.features]
prg_bank_switching = true
chr_bank_switching = true
scanline_irq = true
mapper_controlled_mirroring = true
expansion_audio = false
```

---

## 25. 和重编译器的协作

### 25.1 Bank-aware code cache

对于支持 bank switching 的游戏，PRG-ROM 的同一个 CPU 地址可能对应不同物理 ROM bank。

因此重编译 block 的 key 不应只用 CPU 地址：

```rust
pub struct BlockKey {
    pub cpu_addr: u16,
    pub prg_bank_id: Option<u16>,
    pub physical_rom_offset: usize,
}
```

NROM 可简化为：

```rust
BlockKey {
    cpu_addr,
    prg_bank_id: None,
    physical_rom_offset,
}
```

MMC3/UxROM 等则需要记录实际 bank。

---

### 25.2 Dispatcher fallback

间接跳转或跨 bank 跳转可能无法静态完全确定。

```rust
pub enum BlockExit {
    Next(BlockId),
    Jump(BlockId),
    IndirectJump { addr: u16 },
    Return,
    Interrupt,
}
```

运行时 dispatcher 需要结合当前 Mapper 映射解析目标：

```rust
pub fn dispatch_indirect_jump(
    rt: &mut impl RuntimeAbi,
    cpu_addr: u16,
) -> BlockId {
    let physical = rt.resolve_prg_physical_addr(cpu_addr);
    rt.lookup_or_compile_block(cpu_addr, physical)
}
```

因此 `RuntimeAbi` 可增加可选调试/重编译接口：

```rust
pub trait RecompilerRuntimeExt {
    fn resolve_prg_physical_addr(&self, cpu_addr: u16) -> Option<usize>;
    fn current_prg_bank_id(&self, cpu_addr: u16) -> Option<u16>;
}
```

---

## 26. 测试策略

### 26.1 Mapper 单元测试

每个 Mapper 至少测试：

- CPU read mapping
- CPU write register side effect
- PPU read mapping
- PPU write CHR-RAM
- Mirroring
- IRQ enable/disable
- IRQ latch/counter
- Save/load state
- Debug info
- Native event emission

---

### 26.2 Recompiled vs Interpreter 对照

同一 ROM、同一输入 replay 下比较：

- CPU registers
- RAM hash
- PRG bank state
- CHR bank state
- IRQ state
- OAM hash
- Framebuffer hash

---

### 26.3 Mapper-specific Golden Test

示例：

```text
NROM:
  Battle City 标题画面
  Battle City 开始游戏
  CHR 固定映射

UxROM:
  PRG bank switch 后读取不同代码/data

CNROM:
  CHR bank switch 后图形改变

MMC1:
  5-bit serial register 写入
  PRG/CHR mode 切换
  mirroring 切换

MMC3:
  bank select/register
  PRG mode
  CHR mode
  A12 IRQ counter
  IRQ pending/clear
```

---

## 27. 实现顺序

推荐按以下顺序实现：

```text
1. 定义 RuntimeAbi
2. 定义 Cartridge / ChrStorage / PrgRam
3. 定义 MapperChip trait
4. 实现 MapperContext
5. 实现 Mapper000Nrom
6. 将 Battle City 接入 NROM
7. 将 Bus 的 CPU/PPU read/write 改为经 Cartridge 转发
8. 加入 save/load state
9. 加入 debug info
10. 加入 native CartridgeEvent
11. 实现 UxROM
12. 实现 CNROM
13. 实现 MMC1
14. 实现 MMC3 的 bank switching
15. 实现 MMC3 的 A12 IRQ
16. 加入 ExpansionAudio trait
17. 扩展到 VRC6/VRC7/N163/MMC5/FDS
18. 做静态特化优化
```

---

## 28. 实现检查清单

### 架构检查

- [ ] 重编译代码只依赖 `RuntimeAbi`
- [ ] Mapper 不被写进生成代码
- [ ] Mapper 属于 `Cartridge`
- [ ] Mapper 同时支持 CPU 和 PPU 总线
- [ ] Mapper 可观察 PPU bus event
- [ ] Mapper 可产生 IRQ
- [ ] Mapper 可控制 mirroring
- [ ] Mapper 可选支持 expansion audio
- [ ] Mapper 支持 save/load state
- [ ] Mapper 支持 debug info
- [ ] Mapper 可输出 native events

### NROM 检查

- [ ] 16 KiB PRG 镜像处理正确
- [ ] 32 KiB PRG 固定映射正确
- [ ] CHR-ROM 只读
- [ ] CHR-RAM 可写
- [ ] Mirroring 来自 header/profile
- [ ] Battle City 可运行

### MMC3 检查

- [ ] bank select 正确
- [ ] PRG mode 正确
- [ ] CHR mode 正确
- [ ] mirroring register 正确
- [ ] IRQ latch 正确
- [ ] IRQ reload 正确
- [ ] IRQ enable/disable 正确
- [ ] A12 rising edge 观察正确
- [ ] IRQ pending/clear 正确
- [ ] Save state 包含 IRQ 状态

---

## 29. 结论

在 FC/NES 重编译器中，卡带芯片接口的核心设计是：

```text
重编译器负责 6502 指令语义
RuntimeAbi 负责统一内存/时钟/中断接口
Cartridge 负责承载 PRG/CHR/RAM
MapperChip 负责模拟卡带芯片行为
PPU/APU/输入/原生渲染通过 Runtime 与事件层协作
```

最终的关键接口可以收敛为：

```rust
pub trait MapperChip {
    fn cpu_read(&mut self, ctx: &mut MapperContext, addr: u16) -> Option<u8>;
    fn cpu_write(&mut self, ctx: &mut MapperContext, addr: u16, value: u8) -> bool;

    fn ppu_read(&mut self, ctx: &mut MapperContext, addr: u16) -> Option<u8>;
    fn ppu_write(&mut self, ctx: &mut MapperContext, addr: u16, value: u8) -> bool;

    fn cpu_tick(&mut self, ctx: &mut MapperContext, cycles: u32);
    fn ppu_tick(&mut self, ctx: &mut MapperContext, event: PpuBusEvent);

    fn irq_state(&self) -> IrqState;
    fn clear_irq(&mut self);

    fn mirroring(&self) -> Mirroring;

    fn expansion_audio(&mut self) -> Option<&mut dyn ExpansionAudio>;

    fn save_state(&self) -> MapperSaveState;
    fn load_state(&mut self, state: &MapperSaveState);
}
```

这样设计后，Battle City 的 NROM 是最简单实现；后续扩展 MMC1、MMC3、VRC6、MMC5、FDS 时，不需要修改重编译器生成代码，只需要新增对应 `MapperChip` 实现和 Profile 描述。

---

## 30. 参考资料

- NESdev Wiki: Mapper  
  https://www.nesdev.org/wiki/Mapper

- NESdev Wiki: CPU memory map  
  https://www.nesdev.org/wiki/CPU_memory_map

- NESdev Wiki: PPU memory map  
  https://www.nesdev.org/wiki/PPU_memory_map

- NESdev Wiki: PPU registers  
  https://www.nesdev.org/wiki/PPU_registers

- NESdev Wiki: CHR ROM vs. CHR RAM  
  https://www.nesdev.org/wiki/CHR_ROM_vs._CHR_RAM

- NESdev Wiki: NROM  
  https://www.nesdev.org/wiki/NROM

- NESdev Wiki: MMC1  
  https://www.nesdev.org/wiki/MMC1

- NESdev Wiki: MMC3  
  https://www.nesdev.org/wiki/MMC3

- NESdev Wiki: Expansion audio  
  https://www.nesdev.org/wiki/Category:Expansion_audio
