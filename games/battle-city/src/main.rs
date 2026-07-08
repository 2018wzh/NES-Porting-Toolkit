//! Battle City — NES 原生移植
//!
//! 完整集成:
//! - WGPU 渲染 (framebuffer 兼容 + native tilemap/sprite)
//! - CPAL 音频输出 (APU 混音 → PCM)
//! - egui 调试 UI (CPU/PPU/RAM 查看器 + 输入映射编辑器)
//! - nes-input 输入系统 (winit 键盘 + gilrs 手柄)
//! - 6502 解释器 + 可选重编译 native dispatch

use std::sync::Arc;
use std::sync::mpsc;

use winit::application::ApplicationHandler;
use winit::event::{KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::Window;

use nptk_core::bus::NesBusImpl;
use nptk_input::nes_controller::NesControllerState;
use nptk_core::rom::parse_rom;
use nptk_core::system::NesSystem;
use nptk_wgpu::debug_ui::{DebugData, DebugOverlay};
use nptk_wgpu::renderer::{RenderMode, WgpuRenderer};
use nptk_audio::apu_mixer::ApuMixer;
use nptk_audio::cpal_output::CpalOutput;
use nptk_input::backends::winit_keyboard::WinitKeyboardBackend;
use nptk_input::backend::{InputBackend, InputEventSink, InputDeviceInfo, PhysicalDeviceId, RawGamepadState};
use nptk_input::canonical::CanonicalGamepadState;
use nptk_input::nes_controller::canonical_to_nes_port;
use nptk_native_runtime::runtime::{RecompiledRuntime, PpuEventSink, AudioEventSink};

// ── Execution mode ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExecMode {
    /// Pure 6502 interpreter (compat-interpreter)
    Interpreter,
    /// Recompiled native dispatch + interpreter fallback (recompiled-compat)
    Recompiled,
}

// ── State ────────────────────────────────────────────────────────────────

struct BattleCityApp {
    system: NesSystem,
    recompiled: Option<RecompiledRuntime>,
    exec_mode: ExecMode,
    window: Option<Arc<Window>>,
    renderer: Option<WgpuRenderer>,

    // egui
    egui_state: Option<egui_winit::State>,
    egui_renderer: Option<egui_wgpu::Renderer>,
    debug_overlay: DebugOverlay,

    // Audio
    apu_mixer: ApuMixer,
    cpal_output: CpalOutput,
    audio_tx: Option<mpsc::Sender<f32>>,

    // Input
    keyboard_backend: WinitKeyboardBackend,
    gilrs_backend: Option<nptk_input::backends::gilrs_gamepad::GilrsBackend>,
    input_state: NesControllerState,
    paused: bool,
    render_mode: RenderMode,
    show_debug: bool,
}

impl BattleCityApp {
    fn new(system: NesSystem, use_recompiled: bool) -> Self {
        let gilrs = nptk_input::backends::gilrs_gamepad::GilrsBackend::new().ok();

        let (recompiled, exec_mode) = if use_recompiled {
            // Create a RecompiledRuntime from the system's bus
            let rom_path = std::env::args().nth(1).unwrap_or_else(|| "roms/BattleCity (Japan).nes".into());
            let data = std::fs::read(&rom_path).ok();
            let recompiled = data.and_then(|d| {
                let rom = parse_rom(&d).ok()?;
                let mapper = nptk_core::mapper::create_mapper(rom.header.mapper_id, &rom)?;
                let cartridge = nptk_core::mapper::Cartridge::new_simple(
                    nptk_core::mapper::CartridgeMetadata {
                        mapper_id: rom.header.mapper_id,
                        submapper_id: rom.header.submapper_id,
                        prg_rom_size: rom.header.prg_rom_size,
                        chr_rom_size: rom.header.chr_rom_size,
                        has_sram: rom.header.has_sram,
                        has_trainer: rom.header.has_trainer,
                        battery_backed: false,
                    },
                    rom.prg_rom.clone(),
                    nptk_core::mapper::ChrStorage::Rom(rom.chr_rom.clone().unwrap_or_default()),
                    mapper,
                );
                let bus = NesBusImpl::new(cartridge);
                struct NullSink;
                impl PpuEventSink for NullSink {}
                impl AudioEventSink for NullSink {}
                let mut rt = RecompiledRuntime::new(bus, Box::new(NullSink), Box::new(NullSink));
                // Register all statically-linked AOT blocks
                let dispatch = nptk_battle_city::nes_blocks::get_dispatch();
                for (addr, func) in dispatch {
                    rt.add_cabi_block(addr, func);
                }
                tracing::info!("Registered {} AOT blocks (statically linked)", rt.cabi_dispatch.len());
                Some(rt)
            });
            (recompiled, ExecMode::Recompiled)
        } else {
            (None, ExecMode::Interpreter)
        };

        BattleCityApp {
            system,
            recompiled,
            exec_mode,
            window: None,
            renderer: None,
            egui_state: None,
            egui_renderer: None,
            debug_overlay: DebugOverlay::new(),
            apu_mixer: ApuMixer::new(44100),
            cpal_output: CpalOutput::new(),
            audio_tx: None,
            keyboard_backend: WinitKeyboardBackend::new(),
            gilrs_backend: gilrs,
            input_state: NesControllerState::default(),
            paused: false,
            render_mode: RenderMode::Framebuffer,
            show_debug: true,
        }
    }

    /// Poll a gamepad backend and merge its buttons into `canonical`.
    /// Gamepad input overrides keyboard (OR'd together).
    fn poll_gamepad(
        backend: &mut dyn InputBackend,
        canonical: &mut CanonicalGamepadState,
        now_ns: u64,
    ) {
        struct GamepadMergeSink { out: &'static mut CanonicalGamepadState }
        impl InputEventSink for GamepadMergeSink {
            fn on_raw_gamepad(&mut self, state: RawGamepadState) {
                let b = |i| state.buttons.get(i).copied().unwrap_or(false);
                if b(0) { self.out.south = true; }
                if b(1) { self.out.east = true; }
                if b(11) { self.out.dpad_up = true; }
                if b(12) { self.out.dpad_down = true; }
                if b(13) { self.out.dpad_left = true; }
                if b(14) { self.out.dpad_right = true; }
                if b(7) { self.out.start = true; }
                if b(6) { self.out.select = true; }
            }
            fn on_device_connected(&mut self, _info: InputDeviceInfo) {}
            fn on_device_disconnected(&mut self, _id: PhysicalDeviceId) {}
        }
        // SAFETY: GamepadMergeSink is only used within poll() and canonical lives
        // for the duration of this call.
        let mut sink = unsafe { GamepadMergeSink { out: &mut *(&mut *canonical as *mut _) } };
        backend.poll(now_ns, &mut sink);
    }
}

// ── ApplicationHandler ────────────────────────────────────────────────────

impl ApplicationHandler for BattleCityApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Battle City — NES Porting Toolkit")
                        .with_inner_size(winit::dpi::LogicalSize::new(768.0, 576.0)),
                )
                .unwrap(),
        );

        // Initialise egui
        let egui_state = egui_winit::State::new(
            egui::Context::default(),
            egui::ViewportId::ROOT,
            &window,
            None,
            None,
            None,
        );

        self.egui_state = Some(egui_state);
        self.window = Some(window);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let window = match self.window.as_ref() {
            Some(w) => w.clone(),
            None => return,
        };

        // Let egui process the event first
        if let Some(ref mut egui_state) = self.egui_state {
            let _ = egui_state.on_window_event(&window, &event);
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::RedrawRequested => {
                self.handle_redraw(&window);
                window.request_redraw();
            }

            WindowEvent::Resized(size) => {
                if let Some(r) = &mut self.renderer {
                    r.resize(size.width, size.height);
                }
            }

            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(key),
                        state,
                        ..
                    },
                ..
            } => {
                let pressed = state.is_pressed();

                // Global hotkeys (not consumed by egui when debug panel focused)
                match key {
                    KeyCode::Escape => event_loop.exit(),
                    KeyCode::Space => {
                        if pressed {
                            self.paused = !self.paused;
                        }
                    }
                    KeyCode::F1 => {
                        if pressed {
                            self.render_mode = match self.render_mode {
                                RenderMode::Framebuffer => RenderMode::Native,
                                RenderMode::Native => RenderMode::Framebuffer,
                            };
                            println!("Render: {:?}", self.render_mode);
                        }
                    }
                    KeyCode::F2 => {
                        if pressed {
                            self.show_debug = !self.show_debug;
                            println!("Debug UI: {}", self.show_debug);
                        }
                    }
                    KeyCode::F3 => {
                        if pressed {
                            self.exec_mode = match self.exec_mode {
                                ExecMode::Interpreter => ExecMode::Recompiled,
                                ExecMode::Recompiled => ExecMode::Interpreter,
                            };
                            println!("Exec mode: {:?}", self.exec_mode);
                        }
                    }
                    // NES controller keys → feed to keyboard backend
                    KeyCode::KeyZ => self.keyboard_backend.handle_key_event("z", pressed),
                    KeyCode::KeyX => self.keyboard_backend.handle_key_event("x", pressed),
                    KeyCode::Enter => self.keyboard_backend.handle_key_event("Enter", pressed),
                    KeyCode::ShiftRight => self.keyboard_backend.handle_key_event("RShift", pressed),
                    KeyCode::ArrowUp => self.keyboard_backend.handle_key_event("ArrowUp", pressed),
                    KeyCode::ArrowDown => self.keyboard_backend.handle_key_event("ArrowDown", pressed),
                    KeyCode::ArrowLeft => self.keyboard_backend.handle_key_event("ArrowLeft", pressed),
                    KeyCode::ArrowRight => self.keyboard_backend.handle_key_event("ArrowRight", pressed),
                    _ => {}
                }
            }

            _ => {}
        }
    }
}

// ── Redraw / frame logic ──────────────────────────────────────────────────

impl BattleCityApp {
    fn handle_redraw(&mut self, window: &Window) {
        // Lazy init renderer + audio on first redraw
        if self.renderer.is_none() {
            let size = window.inner_size();
            self.renderer = Some(
                pollster::block_on(WgpuRenderer::new(window, size.width, size.height))
                    .expect("Failed to create WGPU renderer"),
            );

            // Create egui renderer
            let renderer = self.renderer.as_ref().unwrap();
            self.egui_renderer = Some(egui_wgpu::Renderer::new(
                &renderer.device,
                renderer.config.format,
                None,
                1,
                false,
            ));
        }

        // Start audio on first frame
        if self.audio_tx.is_none() {
            self.audio_tx = self.cpal_output.start();
            if self.audio_tx.is_some() {
                // Recreate ApuMixer with the actual output sample rate
                let actual_rate = self.cpal_output.sample_rate();
                self.apu_mixer = ApuMixer::new(actual_rate);
                println!("Audio: CPAL output stream started at {} Hz", actual_rate);
            } else {
                println!("Audio: unavailable (no output device)");
            }
        }

        // ── Poll input backends BEFORE frame ──────────────────────────
        if !self.paused {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default().as_nanos() as u64;

            // Start with keyboard state
            let mut canonical = self.keyboard_backend.state().clone();

            // Poll gamepad backends — gamepad input overrides keyboard
            if let Some(ref mut gilrs) = self.gilrs_backend {
                let _ = Self::poll_gamepad(gilrs, &mut canonical, now);
            }
            // Map to NES port 1
            self.input_state = canonical_to_nes_port(&canonical, 1);
            self.system.bus.controller[0].set_current(self.input_state);

            // Run frame — choose execution mode
            let fb = match self.exec_mode {
                ExecMode::Recompiled => {
                    if let Some(ref mut rt) = self.recompiled {
                        // Sync controller state to recompiled runtime
                        rt.bus.controller[0].set_current(self.input_state);
                        rt.run_frame();
                        *rt.framebuffer()
                    } else {
                        *self.system.run_frame()
                    }
                }
                ExecMode::Interpreter => {
                    *self.system.run_frame()
                }
            };

            // Feed audio samples
            if let Some(ref tx) = self.audio_tx {
                let apu = &self.system.bus.apu;
                // Collect APU channel outputs and mix
                let p1 = apu.pulse1_output();
                let p2 = apu.pulse2_output();
                let tri = apu.triangle_output();
                let noise = apu.noise_output();
                self.apu_mixer.mix(
                    nptk_core::system::CPU_CYCLES_PER_FRAME,
                    p1, p2, tri, noise,
                );
                let samples = self.apu_mixer.drain_samples();
                for s in samples {
                    // Send sample to audio thread (blocks only if buffer is full)
                    let _ = tx.send(s);
                }
            }

            let renderer = self.renderer.as_mut().unwrap();
            renderer.render_mode = self.render_mode;

            match self.render_mode {
                RenderMode::Framebuffer => {
                    renderer.upload_framebuffer(&fb);
                }
                RenderMode::Native => {
                    // Read CHR data from mapper (needs &mut mapper; done first)
                    let mut chr_data = vec![0u8; 8192];
                    for addr in 0..8192u16 {
                        if let Some(b) = self.system.bus.cartridge.ppu_read(addr) {
                            chr_data[addr as usize] = b;
                        }
                    }

                    // Read PPU state (after mapper borrow is released)
                    let ppu = &self.system.bus.ppu;
                    let nametable = &ppu.nametable_data()[..1024];
                    let attr = &ppu.nametable_data()[960..1024];
                    let palette = ppu.palette_data();
                    let oam = ppu.oam_data();
                    let ppu_ctrl = ppu.ctrl;

                    renderer.upload_native_data(
                        &chr_data,
                        nametable,
                        attr,
                        palette,
                        oam,
                        ppu_ctrl,
                    );
                }
            }

            // ── Build debug snapshot ────────────────────────────────
            if self.show_debug {
                let cpu = &self.system.cpu;
                let ppu = &self.system.bus.ppu;

                // Compute simple frame hash
                let mut hash: u64 = 0;
                for (i, &b) in fb.iter().enumerate() {
                    hash = hash.wrapping_mul(31).wrapping_add(b as u64);
                    if i % 101 == 0 { hash = hash.rotate_left(7); }
                }

                self.debug_overlay.update_nes_state(DebugData {
                    cpu_a: cpu.a,
                    cpu_x: cpu.x,
                    cpu_y: cpu.y,
                    cpu_sp: cpu.sp,
                    cpu_pc: cpu.pc,
                    cpu_flag_c: cpu.status.carry,
                    cpu_flag_z: cpu.status.zero,
                    cpu_flag_i: cpu.status.interrupt_disable,
                    cpu_flag_d: cpu.status.decimal,
                    cpu_flag_v: cpu.status.overflow,
                    cpu_flag_n: cpu.status.negative,
                    cpu_cycles: cpu.cycles,
                    cpu_cycle_count: self.system.cpu_cycle,
                    ppu_ctrl: ppu.ctrl,
                    ppu_mask: ppu.mask,
                    ppu_status: ppu.status,
                    ppu_scanline: ppu.scanline,
                    ppu_cycle: ppu.cycle,
                    ppu_dot: self.system.ppu_dot,
                    frame_count: self.system.frame_count,
                    frame_hash: hash,
                    ram: Some(*self.system.ram()),
                });
            }
        }

        // ── Render ──────────────────────────────────────────────────
        // Begin egui frame
        let egui_state = self.egui_state.as_mut().unwrap();
        let raw_input = egui_state.take_egui_input(window);
        let egui_ctx = egui_state.egui_ctx().clone();

        let egui_full_output = egui_ctx.run(raw_input, |ctx| {
            if self.show_debug {
                self.debug_overlay.render(ctx);

                // Sync pause state between debug overlay and app
                if self.debug_overlay.state.pause_emulation != self.paused {
                    self.paused = self.debug_overlay.state.pause_emulation;
                }
                if self.debug_overlay.state.step_frame {
                    self.debug_overlay.state.step_frame = false;
                    self.paused = false;
                }
            }
        });

        // Handle egui output (cursor, etc.)
        let _ = egui_state.handle_platform_output(window, egui_full_output.platform_output);

        // Prepare egui primitives
        let egui_primitives = egui_ctx.tessellate(
            egui_full_output.shapes,
            egui_ctx.pixels_per_point(),
        );

        // Update egui textures and buffers
        let egui_renderer = self.egui_renderer.as_mut().unwrap();
        let renderer = self.renderer.as_mut().unwrap();
        for (id, delta) in egui_full_output.textures_delta.set {
            egui_renderer.update_texture(&renderer.device, &renderer.queue, id, &delta);
        }

        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [renderer.config.width, renderer.config.height],
            pixels_per_point: window.scale_factor() as f32,
        };

        // Single combined render pass: NES content + egui overlay
        let output = renderer.surface.get_current_texture()
            .expect("Failed to get surface texture");
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = renderer.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor::default(),
        );

        egui_renderer.update_buffers(
            &renderer.device,
            &renderer.queue,
            &mut encoder,
            &egui_primitives,
            &screen_descriptor,
        );

        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Combined Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            // Draw NES content
            match self.render_mode {
                RenderMode::Framebuffer => {
                    rpass.set_pipeline(&renderer.fb_pipeline);
                    rpass.set_bind_group(0, &renderer.fb_bind_group, &[]);
                    rpass.draw(0..4, 0..1);
                }
                RenderMode::Native => {
                    // Native tilemap + sprite instanced rendering
                    renderer.tilemap.render(&mut rpass);
                    renderer.sprite.render(&mut rpass);
                }
            }

            // Draw egui overlay on top
            // SAFETY: rpass is dropped before encoder.finish() on line below.
            // The transmute is needed because egui-wgpu 0.30 requires
            // RenderPass<'static> but the actual borrow is scoped correctly.
            let rpass_static: &mut wgpu::RenderPass<'static> =
                unsafe { std::mem::transmute(&mut rpass) };
            egui_renderer.render(rpass_static, &egui_primitives, &screen_descriptor);
        }

        for id in egui_full_output.textures_delta.free {
            egui_renderer.free_texture(&id);
        }

        renderer.queue.submit([encoder.finish()]);
        output.present();
    }
}

// ── main ──────────────────────────────────────────────────────────────────

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let use_recompiled = args.iter().any(|a| a == "--recompiled" || a == "-r");
    let rom_path = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.clone())
        .unwrap_or_else(|| "roms/BattleCity (Japan).nes".into());

    let data = std::fs::read(&rom_path)?;
    let rom = parse_rom(&data)?;
    let mapper = nptk_core::mapper::create_mapper(rom.header.mapper_id, &rom)
        .ok_or_else(|| format!("Mapper {} not supported", rom.header.mapper_id))?;
    let cartridge = nptk_core::mapper::Cartridge::new_simple(
        nptk_core::mapper::CartridgeMetadata {
            mapper_id: rom.header.mapper_id,
            submapper_id: rom.header.submapper_id,
            prg_rom_size: rom.header.prg_rom_size,
            chr_rom_size: rom.header.chr_rom_size,
            has_sram: rom.header.has_sram,
            has_trainer: rom.header.has_trainer,
            battery_backed: false,
        },
        rom.prg_rom.clone(),
        nptk_core::mapper::ChrStorage::Rom(rom.chr_rom.clone().unwrap_or_default()),
        mapper,
    );

    println!("Battle City — NES Porting Toolkit");
    println!(
        "  Mapper: {}, PRG: {}KB, CHR: {}KB, Mirroring: {:?}",
        rom.header.mapper_id,
        rom.header.prg_rom_size / 1024,
        rom.header.chr_rom_size / 1024,
        rom.header.mirroring
    );
    println!("  Controls: Z/X=AB, Arrows=DPad, Enter=Start, RShift=Select");
    println!("  F1=Render mode, F2=Debug UI, F3=Exec mode, Space=Pause, Esc=Exit");
    println!("  Mode: {}", if use_recompiled { "Recompiled" } else { "Interpreter" });

    let bus = NesBusImpl::new(cartridge);
    let system = NesSystem::new(bus);
    let mut app = BattleCityApp::new(system, use_recompiled);

    let event_loop = EventLoop::new()?;
    event_loop.run_app(&mut app)?;
    Ok(())
}
