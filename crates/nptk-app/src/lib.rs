//! 原生应用框架 — 封装窗口/事件循环/渲染/音频/输入
//!
//! 提供 `NesApp` 结构体，游戏 crate 只需实现 `GameHandlers` trait，
//! 所有平台细节（winit 事件循环、WGPU 渲染、CPAL 音频、输入轮询、FLTK 调试 UI）
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
use nptk_debug_ui::{DebugCommand, DebugData, DebugWindowHandle};
use nptk_input::backend::{
    InputBackend, InputDeviceInfo, InputEventSink, PhysicalDeviceId, RawGamepadState,
};
use nptk_input::backends::winit_keyboard::WinitKeyboardBackend;
use nptk_input::canonical::CanonicalGamepadState;
use nptk_input::nes_controller::NesControllerState;
use nptk_input::nes_controller::canonical_to_nes_port;
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
    /// 是否显示调试 UI（由 FLTK 窗口存在与否决定）
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
    /// 调试数据收集器（用于发送到 FLTK 窗口）
    pub debug_collector: &'a mut DebugCollector,
}

// ---------------------------------------------------------------------------
// Debug collector — 替代旧的 DebugOverlay
// ---------------------------------------------------------------------------

/// 调试数据收集器 — 游戏帧回调中填充，然后由 `NesApp` 发送到 FLTK 窗口。
pub struct DebugCollector {
    /// 最新收集的 NES 状态
    pub data: Option<DebugData>,
    /// 是否启用收集（由 FLTK 窗口存在与否控制）
    pub enabled: bool,
}

impl DebugCollector {
    pub fn new() -> Self {
        Self {
            data: None,
            enabled: false,
        }
    }

    /// 更新调试数据（由游戏帧回调调用）
    pub fn update(&mut self, data: DebugData) {
        if self.enabled {
            self.data = Some(data);
        }
    }
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
    window: Option<Arc<winit::window::Window>>,

    // 渲染
    renderer: Option<WgpuRenderer>,

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

    // FLTK 调试窗口
    debug_handle: Option<DebugWindowHandle>,
}

impl<G: GameHandlers> NesApp<G> {
    /// 创建新的 NES 应用
    pub fn new(game: G) -> Self {
        let gilrs = nptk_input::backends::gilrs_gamepad::GilrsBackend::new().ok();

        NesApp {
            game,
            window: None,
            renderer: None,
            apu_mixer: ApuMixer::new(44100),
            cpal_output: CpalOutput::new(),
            audio_tx: None,
            keyboard_backend: WinitKeyboardBackend::new(),
            gilrs_backend: gilrs,
            paused: false,
            render_mode: RenderMode::Framebuffer,
            debug_handle: None,
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

    /// 切换 FLTK 调试窗口的打开/关闭
    fn toggle_debug_window(&mut self) {
        if self.debug_handle.is_some() {
            // 关闭调试窗口
            if let Some(handle) = self.debug_handle.take() {
                let _ = handle.tx.send(DebugCommand::Shutdown);
            }
            println!("Debug UI: closed");
        } else {
            // 打开调试窗口
            let handle = DebugWindowHandle::spawn();
            println!("Debug UI: opened (FLTK window)");
            self.debug_handle = Some(handle);
        }
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
                    winit::window::Window::default_attributes()
                        .with_title(self.game.window_title())
                        .with_inner_size(winit::dpi::LogicalSize::new(w, h)),
                )
                .unwrap(),
        );

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
                            self.toggle_debug_window();
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
            let mut debug_collector = DebugCollector::new();
            debug_collector.enabled = self.debug_handle.is_some();

            // 调用游戏帧回调 — 游戏在此执行 NES 帧逻辑
            {
                let mut show_debug = self.debug_handle.is_some();
                let mut ctx = FrameContext {
                    recompiled: &mut recompiled_wrapper,
                    exec_mode: &mut exec_mode,
                    render_mode: &mut self.render_mode,
                    show_debug: &mut show_debug,
                    paused: &mut self.paused,
                    input_state: &mut input_state,
                    framebuffer: &mut fb,
                    apu_mixer: &mut self.apu_mixer,
                    audio_tx: &mut self.audio_tx,
                    debug_collector: &mut debug_collector,
                };
                self.game.run_frame(&mut ctx);
            }

            // ── 发送调试数据到 FLTK 窗口 ──────────────────────────
            if let Some(ref handle) = self.debug_handle {
                if let Some(data) = debug_collector.data.take() {
                    handle.update(data);
                }
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

        // ── WGPU 渲染（无 egui 叠加层）───────────────────────────
        let renderer = self.renderer.as_ref().unwrap();

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

        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("NES Render Pass"),
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
                    rpass.set_vertex_buffer(0, renderer.dummy_vb.slice(..));
                    rpass.set_bind_group(0, &renderer.fb_bind_group, &[]);
                    rpass.draw(0..4, 0..1);
                }
                RenderMode::Native => {
                    renderer.tilemap.render(&mut rpass);
                    renderer.sprite.render(&mut rpass);
                }
            }
        }

        renderer.queue.submit([encoder.finish()]);
        output.present();
    }
}
