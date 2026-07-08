//! 原生应用框架 — 封装窗口/事件循环/渲染/音频/输入
//!
//! 提供 `NesApp` 结构体，游戏 crate 只需实现 `GameHandlers` trait，
//! 所有平台细节（winit 事件循环、WGPU 渲染、CPAL 音频、输入轮询、egui 调试 UI）
//! 都由本模块处理。
//!
//! # 用法
//!
//! ```ignore
//! use nptk_app::{NesApp, GameHandlers, FrameContext};
//!
//! struct MyGame { ... }
//!
//! impl GameHandlers for MyGame {
//!     fn run_frame(&mut self, ctx: &mut FrameContext) { ... }
//! }
//!
//! fn main() {
//!     let game = MyGame::new();
//!     NesApp::new(game).run().unwrap();
//! }
//! ```

use std::sync::Arc;
use std::sync::mpsc;

use winit::application::ApplicationHandler;
use winit::event::{KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::Window;

use nptk_audio::apu_mixer::ApuMixer;
use nptk_audio::cpal_output::CpalOutput;
use nptk_input::backend::{
    InputBackend, InputDeviceInfo, InputEventSink, PhysicalDeviceId, RawGamepadState,
};
use nptk_input::backends::winit_keyboard::WinitKeyboardBackend;
use nptk_input::canonical::CanonicalGamepadState;
use nptk_input::nes_controller::NesControllerState;
use nptk_input::nes_controller::canonical_to_nes_port;
use nptk_wgpu::debug_ui::DebugOverlay;
use nptk_wgpu::renderer::{RenderMode, WgpuRenderer};

// ---------------------------------------------------------------------------
// Execution mode
// ---------------------------------------------------------------------------

/// NES 执行模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecMode {
    /// 纯 6502 解释器
    Interpreter,
    /// 重编译原生 dispatch + 解释器回退
    Recompiled,
}

// ---------------------------------------------------------------------------
// Frame context — 传递给游戏回调的帧数据
// ---------------------------------------------------------------------------

/// 每帧上下文 — 游戏回调通过此结构访问平台资源
pub struct FrameContext<'a> {
    /// 重编译运行时（可选）
    pub recompiled: &'a mut Option<RecompiledRuntimeWrapper>,
    /// 当前执行模式
    pub exec_mode: &'a mut ExecMode,
    /// 渲染模式
    pub render_mode: &'a mut RenderMode,
    /// 是否显示调试 UI
    pub show_debug: &'a mut bool,
    /// 是否暂停
    pub paused: &'a mut bool,
    /// 输入状态（NES 控制器 port 1）
    pub input_state: &'a mut NesControllerState,
    /// 帧缓冲区（256x240 索引色）
    pub framebuffer: &'a mut [u8; 256 * 240],
    /// APU 混音器
    pub apu_mixer: &'a mut ApuMixer,
    /// 音频发送器
    pub audio_tx: &'a mut Option<mpsc::Sender<f32>>,
    /// 调试叠加层
    pub debug_overlay: &'a mut DebugOverlay,
}

// ---------------------------------------------------------------------------
// Game handlers trait — 游戏 crate 需实现此 trait
// ---------------------------------------------------------------------------

/// 游戏回调 trait — 游戏 crate 实现此 trait 来接入平台框架
pub trait GameHandlers {
    /// 每帧回调 — 游戏在此执行 NES 帧逻辑
    fn run_frame(&mut self, ctx: &mut FrameContext);

    /// 窗口标题（可选覆盖）
    fn window_title(&self) -> &str {
        "NES Game — Porting Toolkit"
    }

    /// 窗口初始大小（可选覆盖）
    fn window_size(&self) -> (f64, f64) {
        (768.0, 576.0)
    }
}

// ---------------------------------------------------------------------------
// RecompiledRuntime wrapper
// ---------------------------------------------------------------------------

/// 重编译运行时包装器
pub struct RecompiledRuntimeWrapper {
    pub inner: nptk_native_runtime::runtime::RecompiledRuntime,
}

impl RecompiledRuntimeWrapper {
    pub fn new(inner: nptk_native_runtime::runtime::RecompiledRuntime) -> Self {
        Self { inner }
    }

    pub fn framebuffer(&self) -> &[u8; 256 * 240] {
        self.inner.framebuffer()
    }

    pub fn run_frame(&mut self) {
        self.inner.run_frame();
    }
}

// ---------------------------------------------------------------------------
// NesApp — 主应用结构体
// ---------------------------------------------------------------------------

/// NES 原生应用 — 封装完整的窗口/渲染/音频/输入/调试 UI 框架
pub struct NesApp<G: GameHandlers> {
    /// 游戏处理器
    pub game: G,

    // 窗口
    window: Option<Arc<Window>>,

    // 渲染
    renderer: Option<WgpuRenderer>,

    // egui
    egui_state: Option<egui_winit::State>,
    egui_renderer: Option<egui_wgpu::Renderer>,

    // 音频
    apu_mixer: ApuMixer,
    cpal_output: CpalOutput,
    audio_tx: Option<mpsc::Sender<f32>>,

    // 输入
    keyboard_backend: WinitKeyboardBackend,
    gilrs_backend: Option<nptk_input::backends::gilrs_gamepad::GilrsBackend>,

    // 状态
    paused: bool,
    render_mode: RenderMode,
    show_debug: bool,
}

impl<G: GameHandlers> NesApp<G> {
    /// 创建新的 NES 应用
    pub fn new(game: G) -> Self {
        let gilrs = nptk_input::backends::gilrs_gamepad::GilrsBackend::new().ok();

        NesApp {
            game,
            window: None,
            renderer: None,
            egui_state: None,
            egui_renderer: None,
            apu_mixer: ApuMixer::new(44100),
            cpal_output: CpalOutput::new(),
            audio_tx: None,
            keyboard_backend: WinitKeyboardBackend::new(),
            gilrs_backend: gilrs,
            paused: false,
            render_mode: RenderMode::Framebuffer,
            show_debug: true,
        }
    }

    /// 运行应用（阻塞直到窗口关闭）
    pub fn run(mut self) -> Result<(), Box<dyn std::error::Error>> {
        let event_loop = EventLoop::new()?;
        event_loop.run_app(&mut self)?;
        Ok(())
    }

    /// 轮询游戏手柄输入并合并到规范状态
    fn poll_gamepad(
        backend: &mut dyn InputBackend,
        canonical: &mut CanonicalGamepadState,
        now_ns: u64,
    ) {
        struct GamepadMergeSink {
            out: &'static mut CanonicalGamepadState,
        }
        impl InputEventSink for GamepadMergeSink {
            fn on_raw_gamepad(&mut self, state: RawGamepadState) {
                let b = |i| state.buttons.get(i).copied().unwrap_or(false);
                if b(0) {
                    self.out.south = true;
                }
                if b(1) {
                    self.out.east = true;
                }
                if b(11) {
                    self.out.dpad_up = true;
                }
                if b(12) {
                    self.out.dpad_down = true;
                }
                if b(13) {
                    self.out.dpad_left = true;
                }
                if b(14) {
                    self.out.dpad_right = true;
                }
                if b(7) {
                    self.out.start = true;
                }
                if b(6) {
                    self.out.select = true;
                }
            }
            fn on_device_connected(&mut self, _info: InputDeviceInfo) {}
            fn on_device_disconnected(&mut self, _id: PhysicalDeviceId) {}
        }
        let mut sink = unsafe {
            GamepadMergeSink {
                out: &mut *(&mut *canonical as *mut _),
            }
        };
        backend.poll(now_ns, &mut sink);
    }
}

// ---------------------------------------------------------------------------
// ApplicationHandler implementation
// ---------------------------------------------------------------------------

impl<G: GameHandlers> ApplicationHandler for NesApp<G> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let (w, h) = self.game.window_size();
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title(self.game.window_title())
                        .with_inner_size(winit::dpi::LogicalSize::new(w, h)),
                )
                .unwrap(),
        );

        // 初始化 egui
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

        // 让 egui 先处理事件
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

                // 全局热键
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
                    // NES 控制器按键 → 送入键盘后端
                    KeyCode::KeyZ => self.keyboard_backend.handle_key_event("z", pressed),
                    KeyCode::KeyX => self.keyboard_backend.handle_key_event("x", pressed),
                    KeyCode::Enter => self.keyboard_backend.handle_key_event("Enter", pressed),
                    KeyCode::ShiftRight => {
                        self.keyboard_backend.handle_key_event("RShift", pressed)
                    }
                    KeyCode::ArrowUp => self.keyboard_backend.handle_key_event("ArrowUp", pressed),
                    KeyCode::ArrowDown => {
                        self.keyboard_backend.handle_key_event("ArrowDown", pressed)
                    }
                    KeyCode::ArrowLeft => {
                        self.keyboard_backend.handle_key_event("ArrowLeft", pressed)
                    }
                    KeyCode::ArrowRight => self
                        .keyboard_backend
                        .handle_key_event("ArrowRight", pressed),
                    _ => {}
                }
            }

            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Redraw / frame logic
// ---------------------------------------------------------------------------

impl<G: GameHandlers> NesApp<G> {
    fn handle_redraw(&mut self, window: &Window) {
        // 首次渲染时懒初始化渲染器和音频
        if self.renderer.is_none() {
            let size = window.inner_size();
            self.renderer = Some(
                pollster::block_on(WgpuRenderer::new(window, size.width, size.height))
                    .expect("Failed to create WGPU renderer"),
            );

            // 创建 egui renderer
            let renderer = self.renderer.as_ref().unwrap();
            self.egui_renderer = Some(egui_wgpu::Renderer::new(
                &renderer.device,
                renderer.config.format,
                None,
                1,
                false,
            ));
        }

        // 首次帧时启动音频
        if self.audio_tx.is_none() {
            self.audio_tx = self.cpal_output.start();
            if self.audio_tx.is_some() {
                let actual_rate = self.cpal_output.sample_rate();
                self.apu_mixer = ApuMixer::new(actual_rate);
                println!("Audio: CPAL output stream started at {} Hz", actual_rate);
            } else {
                println!("Audio: unavailable (no output device)");
            }
        }

        // ── 输入轮询 ──────────────────────────────────────────────
        if !self.paused {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64;

            let mut canonical = self.keyboard_backend.state().clone();

            if let Some(ref mut gilrs) = self.gilrs_backend {
                let _ = Self::poll_gamepad(gilrs, &mut canonical, now);
            }

            let mut input_state = canonical_to_nes_port(&canonical, 1);

            // ── 构建帧上下文 ──────────────────────────────────────
            let mut fb = [0u8; 256 * 240];
            let mut exec_mode = ExecMode::Interpreter;
            let mut recompiled_wrapper: Option<RecompiledRuntimeWrapper> = None;
            let mut debug_overlay = DebugOverlay::new();

            // 调用游戏帧回调 — 游戏在此执行 NES 帧逻辑
            {
                let mut ctx = FrameContext {
                    recompiled: &mut recompiled_wrapper,
                    exec_mode: &mut exec_mode,
                    render_mode: &mut self.render_mode,
                    show_debug: &mut self.show_debug,
                    paused: &mut self.paused,
                    input_state: &mut input_state,
                    framebuffer: &mut fb,
                    apu_mixer: &mut self.apu_mixer,
                    audio_tx: &mut self.audio_tx,
                    debug_overlay: &mut debug_overlay,
                };
                self.game.run_frame(&mut ctx);
            }

            // ── 渲染 ──────────────────────────────────────────────
            let renderer = self.renderer.as_mut().unwrap();
            renderer.render_mode = self.render_mode;

            match self.render_mode {
                RenderMode::Framebuffer => {
                    renderer.upload_framebuffer(&fb);
                }
                RenderMode::Native => {
                    // 游戏帧回调中已处理 native 数据上传
                }
            }
        }

        // ── egui 渲染 ────────────────────────────────────────────
        let egui_state = self.egui_state.as_mut().unwrap();
        let raw_input = egui_state.take_egui_input(window);
        let egui_ctx = egui_state.egui_ctx().clone();

        let egui_full_output = egui_ctx.run(raw_input, |_ctx| {
            // 游戏帧回调中已更新 debug_overlay
        });

        let _ = egui_state.handle_platform_output(window, egui_full_output.platform_output);

        let egui_primitives =
            egui_ctx.tessellate(egui_full_output.shapes, egui_ctx.pixels_per_point());

        let egui_renderer = self.egui_renderer.as_mut().unwrap();
        let renderer = self.renderer.as_mut().unwrap();
        for (id, delta) in egui_full_output.textures_delta.set {
            egui_renderer.update_texture(&renderer.device, &renderer.queue, id, &delta);
        }

        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [renderer.config.width, renderer.config.height],
            pixels_per_point: window.scale_factor() as f32,
        };

        let output = renderer
            .surface
            .get_current_texture()
            .expect("Failed to get surface texture");
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = renderer
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

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

            // 绘制 NES 内容
            match self.render_mode {
                RenderMode::Framebuffer => {
                    rpass.set_pipeline(&renderer.fb_pipeline);
                    rpass.set_bind_group(0, &renderer.fb_bind_group, &[]);
                    rpass.draw(0..4, 0..1);
                }
                RenderMode::Native => {
                    renderer.tilemap.render(&mut rpass);
                    renderer.sprite.render(&mut rpass);
                }
            }

            // 绘制 egui 叠加层
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
