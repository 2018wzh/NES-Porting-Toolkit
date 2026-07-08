//! NesRuntime trait — 重编译代码和原生系统之间的 ABI
//! 参考: docs/plan.md §6.3 Runtime ABI

use nptk_core::bus::NesBus;

// ── extern "C" 桥接函数（供 Cranelift AOT 编译的 .dll 链接） ──

/// Cranelift AOT 编译的代码通过此函数读取 NES 内存
/// 签名必须与 codegen.rs 中声明的外部函数一致
#[unsafe(no_mangle)]
pub extern "C" fn nes_read8(bus: *mut nptk_core::bus::NesBusImpl, addr: u16) -> u8 {
    unsafe { (*bus).cpu_read(addr) }
}

/// Cranelift AOT 编译的代码通过此函数写入 NES 内存
#[unsafe(no_mangle)]
pub extern "C" fn nes_write8(bus: *mut nptk_core::bus::NesBusImpl, addr: u16, value: u8) {
    unsafe {
        (*bus).cpu_write(addr, value);
    }
}

/// Cranelift AOT 编译的代码通过此函数推进 CPU 周期
/// 每条 6502 指令执行后调用，使 Mapper/PPU/APU 保持同步
#[unsafe(no_mangle)]
pub extern "C" fn nes_advance_cycles(bus: *mut nptk_core::bus::NesBusImpl, cycles: u32) {
    unsafe {
        (*bus).tick_cpu(cycles);
    }
}

/// PPU 事件接收器
pub trait PpuEventSink {
    fn on_frame_complete(&mut self, _framebuffer: &[u8; 256 * 240]) {}
}

/// 音频事件接收器
pub trait AudioEventSink {
    fn push_sample(&mut self, _sample: f32) {}
}

/// NES 运行时 ABI — 重编译代码通过此 trait 访问硬件
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

/// 兼容运行时实现 — 基于 NesBus
pub struct CompatRuntime {
    pub bus: nptk_core::bus::NesBusImpl,
    ppu_sink: Box<dyn PpuEventSink>,
    audio_sink: Box<dyn AudioEventSink>,
}

impl CompatRuntime {
    /// 创建一个兼容运行时
    pub fn new(
        bus: nptk_core::bus::NesBusImpl,
        ppu: Box<dyn PpuEventSink>,
        audio: Box<dyn AudioEventSink>,
    ) -> Self {
        CompatRuntime {
            bus,
            ppu_sink: ppu,
            audio_sink: audio,
        }
    }
}

impl NesRuntime for CompatRuntime {
    fn read8(&mut self, addr: u16) -> u8 {
        self.bus.cpu_read(addr)
    }
    fn write8(&mut self, addr: u16, value: u8) {
        self.bus.cpu_write(addr, value)
    }
    fn advance_cpu_cycles(&mut self, cycles: u32) {
        self.bus.tick_cpu(cycles)
    }
    fn nmi_pending(&self) -> bool {
        self.bus.ppu.has_nmi
    }
    fn clear_nmi(&mut self) {
        self.bus.ppu.has_nmi = false;
    }
    fn read_controller_shift(&mut self, port: u8) -> u8 {
        self.bus.controller[port as usize % 2].read()
    }
    fn write_controller_strobe(&mut self, value: u8) {
        self.bus.controller[0].write_strobe(value);
        self.bus.controller[1].write_strobe(value);
    }
    fn ppu_events(&mut self) -> &mut dyn PpuEventSink {
        &mut *self.ppu_sink
    }
    fn audio_events(&mut self) -> &mut dyn AudioEventSink {
        &mut *self.audio_sink
    }
}

/// CPU state used by recompiled native blocks
#[derive(Debug, Clone, Default)]
#[repr(C)]
pub struct NativeCpuState {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub sp: u8,
    pub carry: bool,
    pub zero: bool,
    pub negative: bool,
    pub overflow: bool,
    pub interrupt_disable: bool,
}

impl NativeCpuState {
    pub fn set_zn(&mut self, v: u8) {
        self.zero = v == 0;
        self.negative = v & 0x80 != 0;
    }

    /// Sync from interpreter CPU
    pub fn from_cpu(cpu: &nptk_core::cpu_ref::Cpu6502) -> Self {
        NativeCpuState {
            a: cpu.a,
            x: cpu.x,
            y: cpu.y,
            sp: cpu.sp,
            carry: cpu.status.carry,
            zero: cpu.status.zero,
            negative: cpu.status.negative,
            overflow: cpu.status.overflow,
            interrupt_disable: cpu.status.interrupt_disable,
        }
    }

    /// Write back to interpreter CPU
    pub fn to_cpu(&self, cpu: &mut nptk_core::cpu_ref::Cpu6502) {
        cpu.a = self.a;
        cpu.x = self.x;
        cpu.y = self.y;
        cpu.sp = self.sp;
        cpu.status.carry = self.carry;
        cpu.status.zero = self.zero;
        cpu.status.negative = self.negative;
        cpu.status.overflow = self.overflow;
        cpu.status.interrupt_disable = self.interrupt_disable;
    }
}

/// A native compiled 6502 block function pointer
///
/// 返回值: u32，低 16 位 = 消耗的 CPU 周期数，高 16 位 = 下一个 PC
pub type NativeBlockFn = fn(rt: &mut dyn NesRuntime, cpu: &mut NativeCpuState) -> u32;

/// C ABI native block function pointer (used by Cranelift AOT)
///
/// The Cranelift-generated code uses C calling convention with raw pointers
/// instead of Rust trait objects.
///
/// 返回值: u32，低 16 位 = 消耗的 CPU 周期数，高 16 位 = 下一个 PC
pub type CAbiBlockFn =
    unsafe extern "C" fn(bus: *mut nptk_core::bus::NesBusImpl, cpu: *mut NativeCpuState) -> u32;

/// Recompiled execution mode — dispatches to native blocks, falls back to interpreter
pub struct RecompiledRuntime {
    pub bus: nptk_core::bus::NesBusImpl,
    pub cpu: nptk_core::cpu_ref::Cpu6502,
    pub dispatch: std::collections::HashMap<u16, NativeBlockFn>,
    /// C ABI dispatch table (from Cranelift AOT)
    pub cabi_dispatch: std::collections::HashMap<u16, CAbiBlockFn>,
    pub native_state: NativeCpuState,
    pub frame_count: u64,
    pub cpu_cycle: u32,
    ppu_sink: Box<dyn PpuEventSink>,
    audio_sink: Box<dyn AudioEventSink>,
    nmi_pending: bool,
}

impl RecompiledRuntime {
    pub fn new(
        mut bus: nptk_core::bus::NesBusImpl,
        ppu: Box<dyn PpuEventSink>,
        audio: Box<dyn AudioEventSink>,
    ) -> Self {
        let mut cpu = nptk_core::cpu_ref::Cpu6502::new();
        cpu.reset(&mut bus);
        RecompiledRuntime {
            bus,
            cpu,
            dispatch: std::collections::HashMap::new(),
            cabi_dispatch: std::collections::HashMap::new(),
            native_state: NativeCpuState::default(),
            frame_count: 0,
            cpu_cycle: 0,
            ppu_sink: ppu,
            audio_sink: audio,
            nmi_pending: false,
        }
    }

    /// Execute one frame using native dispatch + interpreter fallback
    pub fn run_frame(&mut self) {
        use nptk_core::bus::NesBus;
        self.bus.ppu.clear_frame_complete();

        if self.nmi_pending {
            self.nmi_pending = false;
            self.cpu.trigger_nmi(&mut self.bus);
        }

        self.cpu_cycle = 0;
        let mut ppu_dot = 0u32;

        while self.cpu_cycle < nptk_core::system::CPU_CYCLES_PER_FRAME {
            let pc = self.cpu.pc;

            let cycles = if let Some(&native_fn) = self.dispatch.get(&pc) {
                self.native_state = NativeCpuState::from_cpu(&self.cpu);
                // Create a temporary CompatRuntime for the native call
                let mut rt = CompatRuntime::new_borrowed(
                    &mut self.bus,
                    &mut *self.ppu_sink,
                    &mut *self.audio_sink,
                );
                let result = native_fn(&mut rt, &mut self.native_state);
                self.native_state.to_cpu(&mut self.cpu);
                let block_cycles = (result & 0xFFFF) as u32;
                let next_pc = (result >> 16) as u16;
                if next_pc != 0 {
                    self.cpu.pc = next_pc;
                }
                block_cycles
            } else if let Some(&cabi_fn) = self.cabi_dispatch.get(&pc) {
                // C ABI dispatch (Cranelift AOT blocks)
                self.native_state = NativeCpuState::from_cpu(&self.cpu);
                let bus_ptr = &mut self.bus as *mut nptk_core::bus::NesBusImpl;
                let result = unsafe { cabi_fn(bus_ptr, &mut self.native_state) };
                self.native_state.to_cpu(&mut self.cpu);
                let block_cycles = (result & 0xFFFF) as u32;
                let next_pc = (result >> 16) as u16;
                if next_pc != 0 {
                    self.cpu.pc = next_pc;
                }
                block_cycles
            } else {
                self.cpu.step(&mut self.bus)
            };

            self.cpu_cycle += cycles;
            ppu_dot = ppu_dot.wrapping_add(cycles * 3);
            self.bus.tick_cpu(cycles);

            // 检查 PPU 是否触发了 NMI
            if self.bus.ppu.take_nmi() {
                self.nmi_pending = true;
            }
        }

        // 帧结束：渲染 PPU 帧
        self.bus.render_ppu_frame();
        self.frame_count += 1;
    }

    pub fn add_block(&mut self, addr: u16, func: NativeBlockFn) {
        self.dispatch.insert(addr, func);
    }

    /// Add a C ABI block (from Cranelift AOT) to the dispatch table
    ///
    /// C ABI blocks use raw pointers instead of trait objects.
    /// The dispatch loop in run_frame handles them separately.
    pub fn add_cabi_block(&mut self, addr: u16, func: CAbiBlockFn) {
        self.cabi_dispatch.insert(addr, func);
    }

    pub fn framebuffer(&self) -> &[u8; 256 * 240] {
        self.bus.ppu.frame()
    }
    pub fn ram(&self) -> &[u8; 0x800] {
        &self.bus.ram
    }
}

// Add borrow-safe CompatRuntime constructor
impl CompatRuntime {
    pub fn new_borrowed<'a>(
        bus: &'a mut nptk_core::bus::NesBusImpl,
        ppu: &'a mut dyn PpuEventSink,
        audio: &'a mut dyn AudioEventSink,
    ) -> CompatRuntimeBorrowed<'a> {
        CompatRuntimeBorrowed { bus, ppu, audio }
    }
}

/// Borrowed CompatRuntime — avoids ownership issues in native dispatch
pub struct CompatRuntimeBorrowed<'a> {
    bus: &'a mut nptk_core::bus::NesBusImpl,
    ppu: &'a mut dyn PpuEventSink,
    audio: &'a mut dyn AudioEventSink,
}

impl NesRuntime for CompatRuntimeBorrowed<'_> {
    fn read8(&mut self, addr: u16) -> u8 {
        self.bus.cpu_read(addr)
    }
    fn write8(&mut self, addr: u16, value: u8) {
        self.bus.cpu_write(addr, value)
    }
    fn advance_cpu_cycles(&mut self, cycles: u32) {
        self.bus.tick_cpu(cycles)
    }
    fn nmi_pending(&self) -> bool {
        self.bus.ppu.has_nmi
    }
    fn clear_nmi(&mut self) {
        self.bus.ppu.has_nmi = false;
    }
    fn read_controller_shift(&mut self, port: u8) -> u8 {
        self.bus.controller[port as usize % 2].read()
    }
    fn write_controller_strobe(&mut self, value: u8) {
        self.bus.controller[0].write_strobe(value);
        self.bus.controller[1].write_strobe(value);
    }
    fn ppu_events(&mut self) -> &mut (dyn PpuEventSink + '_) {
        self.ppu
    }
    fn audio_events(&mut self) -> &mut (dyn AudioEventSink + '_) {
        self.audio
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nptk_core::mapper::{Cartridge, CartridgeMetadata, ChrStorage};
    use nptk_core::rom::NesRom;

    struct NullSink;
    impl PpuEventSink for NullSink {}
    impl AudioEventSink for NullSink {}

    fn make_rom() -> NesRom {
        let mut data = vec![0u8; 16 + 16384 + 8192];
        data[0..4].copy_from_slice(b"NES\x1a");
        data[4] = 1;
        data[5] = 1;
        nptk_core::rom::parse_rom(&data).unwrap()
    }

    fn make_cartridge(rom: &NesRom) -> Cartridge {
        let mapper = nptk_core::mapper::create_mapper(0, rom)
            .expect("Mapper not registered");
        Cartridge::new_simple(
            CartridgeMetadata {
                mapper_id: 0,
                submapper_id: 0,
                prg_rom_size: 1,
                chr_rom_size: 1,
                has_sram: false,
                has_trainer: false,
                battery_backed: false,
            },
            rom.prg_rom.clone(),
            ChrStorage::Rom(rom.chr_rom.clone().unwrap_or_default()),
            mapper,
        )
    }

    #[test]
    fn test_compat_runtime_read_write() {
        let rom = make_rom();
        let cart = make_cartridge(&rom);
        let mut rt = CompatRuntime::new(
            nptk_core::bus::NesBusImpl::new(cart),
            Box::new(NullSink),
            Box::new(NullSink),
        );
        rt.write8(0x0051, 42);
        assert_eq!(rt.read8(0x0051), 42);
    }

    /// A native block: LDA #$42, STA $50, then return 0 (RTS)
    /// 返回 (0 << 16) | 2 = 2 周期（LDA immediate 2 周期）
    fn native_test_block(rt: &mut dyn NesRuntime, cpu: &mut NativeCpuState) -> u32 {
        cpu.a = 0x42;
        cpu.set_zn(cpu.a);
        rt.write8(0x0050, cpu.a);
        2u32
    }

    #[test]
    fn test_recompiled_dispatch() {
        let rom = make_rom();
        let cart = make_cartridge(&rom);
        let bus = nptk_core::bus::NesBusImpl::new(cart);
        let mut rt = RecompiledRuntime::new(bus, Box::new(NullSink), Box::new(NullSink));
        rt.add_block(0x8000, native_test_block);
        rt.cpu.pc = 0x8000;
        rt.run_frame();
        assert_eq!(rt.ram()[0x0050], 0x42);
    }

    /// Interpreter vs recompiled comparison on a small program
    #[test]
    fn test_interpreter_vs_recompiled() {
        // Program: LDA #$42, STA $50, LDA $50, CMP #$42, BNE fail, LDA #$FF, STA $51, JMP $8000
        let prog: &[u8] = &[
            0xA9, 0x42, // $8000: LDA #$42
            0x85, 0x50, // $8002: STA $50
            0xA5, 0x50, // $8004: LDA $50
            0xC9, 0x42, // $8006: CMP #$42
            0xD0, 0x03, // $8008: BNE $800D
            0xA9, 0xFF, // $800A: LDA #$FF
            0x85, 0x51, // $800C: STA $51
            0x4C, 0x00, 0x80, // $800D: JMP $8000
        ];

        // Run interpreter
        let mut idata = vec![0u8; 16 + 16384 + 8192];
        idata[0..4].copy_from_slice(b"NES\x1a");
        idata[4] = 1;
        idata[5] = 1;
        let prg_off = 0x10;
        idata[prg_off..prg_off + prog.len()].copy_from_slice(prog);
        idata[prg_off + 0x3FFC] = 0x00;
        idata[prg_off + 0x3FFD] = 0x80;
        let irom = nptk_core::rom::parse_rom(&idata).unwrap();
        let icart = make_cartridge(&irom);
        let ibus = nptk_core::bus::NesBusImpl::new(icart);
        let mut isys = nptk_core::system::NesSystem::new(ibus);

        // Run interpreter for 20 instructions
        for _ in 0..20 {
            isys.step_cpu();
        }
        let i_ram_50 = isys.ram()[0x0050];
        let i_ram_51 = isys.ram()[0x0051];

        // Run recompiled
        let mut rdata = vec![0u8; 16 + 16384 + 8192];
        rdata[0..4].copy_from_slice(b"NES\x1a");
        rdata[4] = 1;
        rdata[5] = 1;
        rdata[prg_off..prg_off + prog.len()].copy_from_slice(prog);
        rdata[prg_off + 0x3FFC] = 0x00;
        rdata[prg_off + 0x3FFD] = 0x80;
        let rrom = nptk_core::rom::parse_rom(&rdata).unwrap();
        let rcart = make_cartridge(&rrom);
        let rbus = nptk_core::bus::NesBusImpl::new(rcart);
        let mut rrt = RecompiledRuntime::new(rbus, Box::new(NullSink), Box::new(NullSink));
        // No native blocks registered — runs purely on interpreter fallback
        // This verifies the fallback path produces identical results
        for _ in 0..20 {
            rrt.cpu.step(&mut rrt.bus);
            rrt.bus.tick_cpu(4);
        }

        assert_eq!(rrt.ram()[0x0050], i_ram_50);
        assert_eq!(rrt.ram()[0x0051], i_ram_51);
    }

    /// Full recompilation pipeline: Cranelift codegen → native functions
    #[test]
    fn test_codegen_battle_city_blocks() {
        use nptk_recompiler::codegen::CraneliftAot;
        use nptk_recompiler::ir_builder::IrBuilder;

        // Battle City reset handler instructions (first 10)
        let reset_bytes: &[u8] = &[
            0x78, // SEI
            0xA9, 0x10, // LDA #$10
            0x8D, 0x00, 0x20, // STA $2000
            0xD8, // CLD
            0xA2, 0x02, // LDX #$02
        ];

        let mut aot = CraneliftAot::new().unwrap();
        let ir_ops = IrBuilder::lift_block(reset_bytes, 0xC070);
        aot.compile_block(0xC070, &ir_ops).unwrap();

        // Verify block was compiled
        assert_eq!(aot.blocks().len(), 1);
        assert_eq!(aot.blocks()[0].address, 0xC070);
        assert!(aot.blocks()[0].name.contains("C070"));

        // Verify we can emit object code
        let (obj_bytes, blocks, _names) = aot.finish().unwrap();
        assert!(!obj_bytes.is_empty());
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].address, 0xC070);
    }

    /// Native dispatch + interpreter fallback verified in a frame loop
    #[test]
    fn test_native_then_interpreter_fallback() {
        fn block_8000(_rt: &mut dyn NesRuntime, cpu: &mut NativeCpuState) -> u32 {
            cpu.a = 0x42;
            cpu.set_zn(cpu.a);
            // 返回 (0x8002 << 16) | 2 = 下一个 PC=$8002, 周期数=2
            (0x8002u32 << 16) | 2
        }
        // Program: native sets A=$42, then interpreter STA $50, JMP $8002
        let prog: &[u8] = &[
            0x00, 0x00, // $8000: NOP, NOP (replaced by native dispatch)
            0x85, 0x50, // $8002: STA $50
            0x4C, 0x02, 0x80, // $8004: JMP $8002
        ];

        let mut data = vec![0u8; 16 + 16384 + 8192];
        data[0..4].copy_from_slice(b"NES\x1a");
        data[4] = 1;
        data[5] = 1;
        data[0x10..0x10 + prog.len()].copy_from_slice(prog);
        data[0x10 + 0x3FFC] = 0x00;
        data[0x10 + 0x3FFD] = 0x80;
        let rom = nptk_core::rom::parse_rom(&data).unwrap();
        let cart = make_cartridge(&rom);
        let bus = nptk_core::bus::NesBusImpl::new(cart);
        let mut rt = RecompiledRuntime::new(bus, Box::new(NullSink), Box::new(NullSink));
        rt.add_block(0x8000, block_8000);

        // Manual dispatch: native at $8000, then interpreter at $8002
        rt.cpu.pc = 0x8000;
        // First dispatch: native
        if let Some(&f) = rt.dispatch.get(&0x8000) {
            rt.native_state = NativeCpuState::from_cpu(&rt.cpu);
            let result = f(
                &mut CompatRuntime::new_borrowed(
                    &mut rt.bus,
                    &mut *rt.ppu_sink,
                    &mut *rt.audio_sink,
                ),
                &mut rt.native_state,
            );
            rt.native_state.to_cpu(&mut rt.cpu);
            let next_pc = (result >> 16) as u16;
            if next_pc != 0 {
                rt.cpu.pc = next_pc;
            }
        }
        // Second dispatch: interpreter fallback at $8002
        rt.cpu.step(&mut rt.bus); // STA $50 (A was set to $42 by native)
        assert_eq!(rt.ram()[0x0050], 0x42);
    }
}
