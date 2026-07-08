//! FLTK-based standalone debug window implementation.
//!
//! Runs in its own thread, receiving `DebugCommand` messages from the
//! main (winit) thread via an `mpsc` channel.  Uses `Fl::awake()` to
//! safely update FLTK widgets from the receiver loop.

use std::sync::mpsc;
use std::sync::Mutex;
use std::sync::Arc;
use std::time::Instant;

use fltk::{app, button::Button, frame::Frame, group::*, input::Input, prelude::*, window::Window};
use fltk::enums::FrameType;

use crate::debug_data::*;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const WINDOW_W: i32 = 820;
const WINDOW_H: i32 = 620;
const REFRESH_MS: f64 = 16.0; // ~60 fps

// ---------------------------------------------------------------------------
// DebugWindow — public API
// ---------------------------------------------------------------------------

/// Handle for controlling the FLTK debug window from the main thread.
pub struct DebugWindowHandle {
    /// Channel sender for pushing commands to the FLTK thread.
    pub tx: mpsc::Sender<DebugCommand>,
    /// Channel receiver for events coming back from the FLTK thread.
    pub rx: mpsc::Receiver<DebugEvent>,
    /// Shared latest data snapshot (read by FLTK thread).
    shared: Arc<Mutex<Option<DebugData>>>,
}

impl DebugWindowHandle {
    /// Spawn the FLTK debug window in a new thread and return a handle.
    ///
    /// The window will appear immediately.  Call `send(DebugCommand::Update(…))`
    /// each frame to keep the display current.  Call `send(DebugCommand::Shutdown)`
    /// to close the window gracefully.
    pub fn spawn() -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<DebugCommand>();
        let (evt_tx, evt_rx) = mpsc::channel::<DebugEvent>();
        let shared: Arc<Mutex<Option<DebugData>>> = Arc::new(Mutex::new(None));
        let shared_clone = Arc::clone(&shared);

        std::thread::spawn(move || {
            run_fltk_window(cmd_rx, evt_tx, shared_clone);
        });

        DebugWindowHandle {
            tx: cmd_tx,
            rx: evt_rx,
            shared,
        }
    }

    /// Push the latest NES state into the shared buffer.
    /// The FLTK thread will pick it up on its next refresh cycle.
    pub fn update(&self, data: DebugData) {
        if let Ok(mut guard) = self.shared.lock() {
            *guard = Some(data);
        }
    }
}

// ---------------------------------------------------------------------------
// FLTK window internals
// ---------------------------------------------------------------------------

struct DebugWidgets {
    // --- Top toolbar ---
    pause_btn: Button,
    step_btn: Button,
    fps_frame: Frame,
    frame_frame: Frame,
    hash_frame: Frame,

    // --- CPU panel ---
    cpu_a: Frame,
    cpu_x: Frame,
    cpu_y: Frame,
    cpu_sp: Frame,
    cpu_pc: Frame,
    cpu_cycles: Frame,
    cpu_flag_c: Frame,
    cpu_flag_z: Frame,
    cpu_flag_i: Frame,
    cpu_flag_d: Frame,
    cpu_flag_v: Frame,
    cpu_flag_n: Frame,

    // --- PPU panel ---
    ppu_ctrl: Frame,
    ppu_mask: Frame,
    ppu_status: Frame,
    ppu_scanline: Frame,
    ppu_cycle: Frame,
    ppu_dot: Frame,

    // --- RAM viewer ---
    ram_hex: Frame,
    ram_nav_top: Button,
    ram_nav_up: Button,
    ram_nav_down: Button,
    ram_nav_stack: Button,
    ram_search_input: Input,
    ram_search_btn: Button,
    ram_highlight_check: Button,

    // --- Input mapping ---
    input_mappings: InputMappings,
    mapping_path: String,
    // We store the rebind state in the FLTK app data
}

fn build_ui() -> (Window, DebugWidgets, Arc<Mutex<DebugUiState>>) {
    let ui_state = Arc::new(Mutex::new(DebugUiState::default()));

    let mut win = Window::new(100, 100, WINDOW_W, WINDOW_H, "NES Debug UI");

    // ── Top toolbar ──────────────────────────────────────────────
    let mut pause_btn = Button::new(10, 10, 80, 28, "Pause");
    let mut step_btn = Button::new(96, 10, 90, 28, "Step Frame");
    step_btn.deactivate(); // disabled until paused

    let mut fps_frame = Frame::new(200, 10, 100, 28, "FPS: --");
    let mut frame_frame = Frame::new(310, 10, 120, 28, "Frame: --");
    let mut hash_frame = Frame::new(440, 10, 200, 28, "");

    // ── CPU panel (left column) ──────────────────────────────────
    let mut cpu_pack = Pack::new(10, 50, 240, 260, "CPU");
    cpu_pack.set_label("CPU");
    cpu_pack.set_frame(FrameType::DownBox);

    let mut cpu_a = Frame::new(10, 50, 220, 20, "A:  --");
    let mut cpu_x = Frame::new(10, 70, 220, 20, "X:  --");
    let mut cpu_y = Frame::new(10, 90, 220, 20, "Y:  --");
    let mut cpu_sp = Frame::new(10, 110, 220, 20, "SP: --");
    let mut cpu_pc = Frame::new(10, 130, 220, 20, "PC: --");
    let mut cpu_cycles = Frame::new(10, 150, 220, 20, "Cycles: --");

    let mut cpu_flag_c = Frame::new(10, 180, 70, 20, "C:0");
    let mut cpu_flag_z = Frame::new(80, 180, 70, 20, "Z:0");
    let mut cpu_flag_i = Frame::new(150, 180, 70, 20, "I:0");
    let mut cpu_flag_d = Frame::new(10, 200, 70, 20, "D:0");
    let mut cpu_flag_v = Frame::new(80, 200, 70, 20, "V:0");
    let mut cpu_flag_n = Frame::new(150, 200, 70, 20, "N:0");

    cpu_pack.end();

    // ── PPU panel (left column, below CPU) ───────────────────────
    let mut ppu_pack = Pack::new(10, 320, 240, 260, "PPU");
    ppu_pack.set_label("PPU");
    ppu_pack.set_frame(FrameType::DownBox);

    let mut ppu_ctrl = Frame::new(10, 320, 220, 20, "CTRL:   --");
    let mut ppu_mask = Frame::new(10, 340, 220, 20, "MASK:   --");
    let mut ppu_status = Frame::new(10, 360, 220, 20, "STATUS: --");
    let mut ppu_scanline = Frame::new(10, 390, 220, 20, "Scanline: --");
    let mut ppu_cycle = Frame::new(10, 410, 220, 20, "Cycle:    --");
    let mut ppu_dot = Frame::new(10, 430, 220, 20, "Dot:      --");

    ppu_pack.end();

    // ── RAM viewer (bottom area) ─────────────────────────────────
    let mut ram_hex = Frame::new(270, 50, 530, 440, "");
    ram_hex.set_frame(FrameType::DownBox);
    ram_hex.set_label_size(10);

    let mut ram_nav_top = Button::new(270, 500, 80, 24, "Top");
    let mut ram_nav_stack = Button::new(356, 500, 80, 24, "Stack");
    let mut ram_nav_up = Button::new(442, 500, 80, 24, "Page Up");
    let mut ram_nav_down = Button::new(528, 500, 80, 24, "Page Dn");

    let mut ram_search_input = Input::new(620, 500, 60, 24, "Go:$");
    let mut ram_search_btn = Button::new(686, 500, 40, 24, "Go");

    let mut ram_highlight_check = Button::new(270, 530, 140, 24, "Highlight changes");
    ram_highlight_check.set_type(fltk::button::ButtonType::Toggle);
    ram_highlight_check.set_value(true);

    // ── Input mapping (right side, placeholder) ──────────────────
    // For now, a simple frame; full input mapping editor will be added later.
    let mut input_frame = Frame::new(270, 560, 530, 40, "Input: Save/Load mappings via RON");

    win.end();
    win.show();

    let widgets = DebugWidgets {
        pause_btn,
        step_btn,
        fps_frame,
        frame_frame,
        hash_frame,
        cpu_a,
        cpu_x,
        cpu_y,
        cpu_sp,
        cpu_pc,
        cpu_cycles,
        cpu_flag_c,
        cpu_flag_z,
        cpu_flag_i,
        cpu_flag_d,
        cpu_flag_v,
        cpu_flag_n,
        ppu_ctrl,
        ppu_mask,
        ppu_status,
        ppu_scanline,
        ppu_cycle,
        ppu_dot,
        ram_hex,
        ram_nav_top,
        ram_nav_up,
        ram_nav_down,
        ram_nav_stack,
        ram_search_input,
        ram_search_btn,
        ram_highlight_check,
        input_mappings: InputMappings::default(),
        mapping_path: "input_mappings.ron".into(),
    };

    (win, widgets, ui_state)
}

// ---------------------------------------------------------------------------
// FLTK event loop (runs in its own thread)
// ---------------------------------------------------------------------------

fn run_fltk_window(
    cmd_rx: mpsc::Receiver<DebugCommand>,
    evt_tx: mpsc::Sender<DebugEvent>,
    shared: Arc<Mutex<Option<DebugData>>>,
) {
    let (_win, mut w, _ui_state) = build_ui();

    // Track previous RAM for dirty highlighting
    let mut prev_ram: Option<[u8; 0x800]> = None;
    let mut _frame_count: u64 = 0;
    let mut fps_smooth: f64 = 0.0;
    let mut fps_acc: u64 = 0;
    let mut fps_timer = Instant::now();

    // Set up a periodic timeout to refresh the UI
    app::add_timeout3(REFRESH_MS, move |_| {
        // This closure will be re-created each tick; we handle refresh inline.
    });

    // Main FLTK event loop
    while app::wait() {
        // ── Drain command channel ────────────────────────────────
        // FLTK is not thread-safe, so we only peek at the shared data
        // that was written by the main thread.

        // ── Read latest shared data ──────────────────────────────
        let data = {
            if let Ok(mut guard) = shared.lock() {
                guard.take()
            } else {
                None
            }
        };

        if let Some(ref d) = data {
            // Update FPS
            _frame_count += 1;
            fps_acc += 1;
            let elapsed = fps_timer.elapsed().as_secs_f64();
            if elapsed >= 1.0 {
                fps_smooth = fps_acc as f64 / elapsed;
                fps_acc = 0;
                fps_timer = Instant::now();
            }

            // ── Update toolbar ───────────────────────────────────
            w.fps_frame.set_label(&format!("FPS: {:.1}", fps_smooth));
            w.frame_frame
                .set_label(&format!("Frame: {}", d.frame_count));
            w.hash_frame
                .set_label(&format!("Hash: {:016X}", d.frame_hash));

            // ── Update CPU panel ─────────────────────────────────
            w.cpu_a.set_label(&format!("A:  {:02X}", d.cpu_a));
            w.cpu_x.set_label(&format!("X:  {:02X}", d.cpu_x));
            w.cpu_y.set_label(&format!("Y:  {:02X}", d.cpu_y));
            w.cpu_sp.set_label(&format!("SP: {:02X}", d.cpu_sp));
            w.cpu_pc.set_label(&format!("PC: {:04X}", d.cpu_pc));
            w.cpu_cycles
                .set_label(&format!("Cycles: {}", d.cpu_cycles));

            let flag = |name: &str, set: bool| -> String {
                if set {
                    format!("{}:1", name)
                } else {
                    format!("{}:0", name)
                }
            };
            w.cpu_flag_c.set_label(&flag("C", d.cpu_flag_c));
            w.cpu_flag_z.set_label(&flag("Z", d.cpu_flag_z));
            w.cpu_flag_i.set_label(&flag("I", d.cpu_flag_i));
            w.cpu_flag_d.set_label(&flag("D", d.cpu_flag_d));
            w.cpu_flag_v.set_label(&flag("V", d.cpu_flag_v));
            w.cpu_flag_n.set_label(&flag("N", d.cpu_flag_n));

            // ── Update PPU panel ─────────────────────────────────
            w.ppu_ctrl
                .set_label(&format!("CTRL:   {:02X}", d.ppu_ctrl));
            w.ppu_mask
                .set_label(&format!("MASK:   {:02X}", d.ppu_mask));
            w.ppu_status
                .set_label(&format!("STATUS: {:02X}", d.ppu_status));
            w.ppu_scanline
                .set_label(&format!("Scanline: {}", d.ppu_scanline));
            w.ppu_cycle
                .set_label(&format!("Cycle:    {}", d.ppu_cycle));
            w.ppu_dot.set_label(&format!("Dot:      {}", d.ppu_dot));

            // ── Update RAM viewer ────────────────────────────────
            if let Some(ref ram) = d.ram {
                let highlight = w.ram_highlight_check.value();
                let _start = 0usize; // show first 32 rows
                let rows = 28;
                let mut hex_text = String::new();
                for row in 0..rows {
                    let addr = row * 16;
                    if addr >= 0x800 {
                        break;
                    }
                    hex_text.push_str(&format!("{:04X}: ", addr));
                    for col in 0..16 {
                        let a = addr + col;
                        if a >= 0x800 {
                            break;
                        }
                        let byte = ram[a];
                        let changed = highlight
                            && prev_ram.map_or(false, |p| p[a] != byte);
                        if changed {
                            // Use a marker for changed bytes (FLTK doesn't support
                            // inline color in Frame easily, so we use a prefix)
                            hex_text.push_str(&format!("\x1b[91m{:02X}\x1b[0m ", byte));
                        } else {
                            hex_text.push_str(&format!("{:02X} ", byte));
                        }
                        if col == 7 {
                            hex_text.push(' ');
                        }
                    }
                    hex_text.push_str("  |");
                    for col in 0..16 {
                        let a = addr + col;
                        if a >= 0x800 {
                            break;
                        }
                        let byte = ram[a];
                        let ch = if byte >= 0x20 && byte < 0x7F {
                            byte as char
                        } else {
                            '.'
                        };
                        hex_text.push(ch);
                    }
                    hex_text.push('|');
                    hex_text.push('\n');
                }
                w.ram_hex.set_label(&hex_text);
                prev_ram = Some(*ram);
            }

            // ── Check for Shutdown command ───────────────────────
            // We also check the channel for shutdown signals.
            if let Ok(cmd) = cmd_rx.try_recv() {
                match cmd {
                    DebugCommand::Shutdown => {
                        let _ = evt_tx.send(DebugEvent::WindowClosed);
                        break;
                    }
                    DebugCommand::Update(_) => {
                        // Already handled via shared state above; ignore.
                    }
                }
            }
        } else {
            // No new data; still check for shutdown
            if let Ok(cmd) = cmd_rx.try_recv() {
                match cmd {
                    DebugCommand::Shutdown => {
                        let _ = evt_tx.send(DebugEvent::WindowClosed);
                        break;
                    }
                    DebugCommand::Update(_) => {}
                }
            }
        }

        // Small sleep to avoid busy-waiting
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
}