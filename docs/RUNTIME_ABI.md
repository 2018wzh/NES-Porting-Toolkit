# NesRuntime ABI Specification

## Overview

The `NesRuntime` trait defines the **Application Binary Interface** between
recompiled 6502 native code and the runtime environment. Every memory access,
MMIO operation, and timing event flows through this interface to ensure correct
NES hardware behaviour regardless of whether the backend is a compatibility
layer or a native implementation.

This ABI is defined in `crates/nptk-native-runtime/src/runtime.rs`.

## NesRuntime Trait

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

### Method Reference

#### Memory Access

`read8(&mut self, addr: u16) -> u8`

Read a single byte from the NES CPU address space. The runtime routes the
address through the full `NesBus`, handling:
- Internal RAM (`$0000-$07FF`) with mirroring (`$0800-$1FFF`)
- PPU registers (`$2000-$2007`, mirrored `$2008-$3FFF`) with register read side effects
- APU and controller registers (`$4000-$4017`) including strobe behaviour
- PRG-ROM via the mapper (`$8000-$FFFF`)

`write8(&mut self, addr: u16, value: u8)`

Write a single byte to the NES CPU address space. Special handling includes:
- OAM DMA trigger via `$4014` (suspends CPU for 513-514 cycles)
- Controller strobe via `$4016` (latches button state into shift register)
- PPU register writes with address latch side effects

#### Timing

`advance_cpu_cycles(&mut self, cycles: u32)`

Advance the system by `cycles` CPU cycles. The runtime:
1. Advances the PPU by `cycles * 3` PPU dots (3:1 ratio)
2. Advances the APU by `cycles` APU cycles (1:1 ratio)
3. Accumulates the total cycle counter

#### NMI Handling

`nmi_pending(&self) -> bool`

Returns `true` when an NMI (Non-Maskable Interrupt) is pending. On NES, NMI
triggers at the start of VBlank (scanline 241). The recompiled code checks
this at the end of each frame or after `advance_cpu_cycles` spans a VBlank
boundary.

`clear_nmi(&mut self)`

Acknowledges and clears the pending NMI flag. Called after the recompiled
NMI handler begins execution.

#### Controller Input

`read_controller_shift(&mut self, port: u8) -> u8`

Reads one bit from the controller shift register. Each read returns bit 0
of the latched state and advances the shift index. After 8 reads, subsequent
reads return 1 (open bus behaviour). This mirrors the hardware behaviour of
reading `$4016`/`$4017`.

`write_controller_strobe(&mut self, value: u8)`

Writes the strobe bit to both controller ports. A 0-to-1 transition latches
the current button state into the shift registers. This mirrors the hardware
behaviour of writing to `$4016`.

#### Event Sinks

`ppu_events(&mut self) -> &mut dyn PpuEventSink`

Returns a mutable reference to the PPU event sink. The recompiled code uses
this to signal frame completion and deliver framebuffers.

`audio_events(&mut self) -> &mut dyn AudioEventSink`

Returns a mutable reference to the audio event sink. The recompiled code
pushes audio samples through this channel.

## PpuEventSink Trait

```rust
pub trait PpuEventSink {
    fn on_frame_complete(&mut self, framebuffer: &[u8; 256 * 240]) {}
}
```

The default implementation is a no-op. Implementations override
`on_frame_complete` to receive the 256x240 indexed framebuffer each time
the PPU finishes rendering a frame.

## AudioEventSink Trait

```rust
pub trait AudioEventSink {
    fn push_sample(&mut self, sample: f32) {}
}
```

The default implementation is a no-op. Implementations override
`push_sample` to receive individual PCM audio samples (float, range
approximately -1.0 to 1.0).

## CompatRuntime Implementation

`CompatRuntime` is the reference implementation of `NesRuntime`, backed by a
full `NesBusImpl`:

```rust
pub struct CompatRuntime {
    pub bus: NesBusImpl,
    ppu_sink: Box<dyn PpuEventSink>,
    audio_sink: Box<dyn AudioEventSink>,
}
```

All `NesRuntime` methods delegate directly to the `NesBus`:

| Method | Delegation |
|---|---|
| `read8(addr)` | `self.bus.cpu_read(addr)` |
| `write8(addr, value)` | `self.bus.cpu_write(addr, value)` |
| `advance_cpu_cycles(cycles)` | `self.bus.tick_cpu(cycles)` |
| `nmi_pending()` | Returns `false` (NMI managed by `NesSystem` frame loop) |
| `clear_nmi()` | No-op |
| `read_controller_shift(port)` | `self.bus.controller[port % 2].read()` |
| `write_controller_strobe(value)` | Writes strobe to both controller ports |
| `ppu_events()` | Returns `&mut *self.ppu_sink` |
| `audio_events()` | Returns `&mut *self.audio_sink` |

## How Recompiled Code Calls Into the Runtime

Recompiled 6502 code does not access memory directly. Every load and store
is lowered to a call through the `NesRuntime` vtable. For example, the
6502 instruction `LDA $2002` (read PPU status) becomes:

```
// Conceptual native code generated by AOT recompiler:
let value = runtime.read8(0x2002);  // vtable dispatch
runtime.advance_cpu_cycles(4);      // LDA absolute takes 4 cycles
cpu.a = value;
cpu.status.set_zn(value);
```

And `STA $2000` (write PPU control) becomes:

```
runtime.write8(0x2000, cpu.a);      // vtable dispatch
runtime.advance_cpu_cycles(4);
```

This indirection is essential because many NES addresses have side effects:
reading `$2002` clears the VBlank flag, writing `$2000` changes the PPU
control register and nametable selection, and accessing `$4000-$4017`
affects audio registers.

## Memory Map Reference

### CPU Address Space (`$0000-$FFFF`)

| Range | Size | Description |
|---|---|---|
| `$0000-$00FF` | 256 B | Zero Page |
| `$0100-$01FF` | 256 B | Stack (SP at `$01FD` on reset) |
| `$0200-$07FF` | 1536 B | General RAM |
| `$0800-$1FFF` | 6 KB | Mirrors of `$0000-$07FF` (repeats every `$0800`) |
| `$2000-$2007` | 8 B | PPU registers |
| `$2008-$3FFF` | 8 KB | Mirrors of `$2000-$2007` (repeats every 8 bytes) |
| `$4000-$4015` | 16 B | APU registers |
| `$4016` | 1 B | Controller port 1 + strobe |
| `$4017` | 1 B | Controller port 2 + APU frame counter |
| `$4018-$401F` | 8 B | CPU test / debug (usually open bus) |
| `$4020-$5FFF` | 8 KB | Expansion ROM (rarely used) |
| `$6000-$7FFF` | 8 KB | SRAM / Battery-backed save (mapper-dependent) |
| `$8000-$BFFF` | 16 KB | PRG-ROM lower bank (or full PRG in 16 KiB NROM) |
| `$C000-$FFFF` | 16 KB | PRG-ROM upper bank (fixed in NROM), vectors at `$FFFA-$FFFF` |

### Interrupt Vectors

| Address | Vector |
|---|---|
| `$FFFA-$FFFB` | NMI handler address |
| `$FFFC-$FFFD` | Reset handler address |
| `$FFFE-$FFFF` | IRQ / BRK handler address |

### PPU Address Space (`$0000-$3FFF`)

| Range | Size | Description |
|---|---|---|
| `$0000-$0FFF` | 4 KB | Pattern table 0 (CHR-ROM) |
| `$1000-$1FFF` | 4 KB | Pattern table 1 (CHR-ROM) |
| `$2000-$23FF` | 1 KB | Nametable 0 |
| `$2400-$27FF` | 1 KB | Nametable 1 |
| `$2800-$2BFF` | 1 KB | Nametable 2 (mirror of 0 or 1) |
| `$2C00-$2FFF` | 1 KB | Nametable 3 (mirror of 0 or 1) |
| `$3000-$3EFF` | 4 KB | Mirrors of `$2000-$2EFF` |
| `$3F00-$3F0F` | 16 B | Background palette |
| `$3F10-$3F1F` | 16 B | Sprite palette |
| `$3F20-$3FFF` | 224 B | Mirrors of `$3F00-$3F1F` |

### PPU Registers

| Address | Register | Access |
|---|---|---|
| `$2000` | PPUCTRL | Write |
| `$2001` | PPUMASK | Write |
| `$2002` | PPUSTATUS | Read |
| `$2003` | OAMADDR | Write |
| `$2004` | OAMDATA | Read/Write |
| `$2005` | PPUSCROLL | Write (2 writes) |
| `$2006` | PPUADDR | Write (2 writes) |
| `$2007` | PPUDATA | Read/Write |

## See Also

- [RECOMPILER.md](RECOMPILER.md) -- how the recompiler emits calls through this ABI
- [PROFILE_FORMAT.md](PROFILE_FORMAT.md) -- GameProfile configuration
- [INPUT_BACKENDS.md](INPUT_BACKENDS.md) -- input system details
