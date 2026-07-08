//! egui debug UI
//!
//! Panels:
//! - Left:   CPU registers, flags, PPU state
//! - Right:  Input mapping editor (key rebinding per NES button)
//! - Bottom: RAM viewer ($0000-$07FF hex dump)
//! - Top:    Frame controls (pause, step, FPS)

use std::collections::HashMap;
use std::time::Instant;

use egui::Color32;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// NES button identifiers
// ---------------------------------------------------------------------------

/// NES controller buttons (8 discrete buttons per port).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub enum NesButton {
    A,
    B,
    Select,
    Start,
    Up,
    Down,
    Left,
    Right,
}

impl NesButton {
    pub const ALL: [NesButton; 8] = [
        NesButton::A,
        NesButton::B,
        NesButton::Select,
        NesButton::Start,
        NesButton::Up,
        NesButton::Down,
        NesButton::Left,
        NesButton::Right,
    ];

    pub fn name(self) -> &'static str {
        match self {
            NesButton::A => "A",
            NesButton::B => "B",
            NesButton::Select => "Select",
            NesButton::Start => "Start",
            NesButton::Up => "Up",
            NesButton::Down => "Down",
            NesButton::Left => "Left",
            NesButton::Right => "Right",
        }
    }
}

// ---------------------------------------------------------------------------
// Data snapshot fed from the NES system each frame
// ---------------------------------------------------------------------------

/// A snapshot of NES state pushed to the debug UI once per frame.
#[derive(Debug, Clone)]
pub struct DebugData {
    // CPU
    pub cpu_a: u8,
    pub cpu_x: u8,
    pub cpu_y: u8,
    pub cpu_sp: u8,
    pub cpu_pc: u16,
    pub cpu_flag_c: bool,
    pub cpu_flag_z: bool,
    pub cpu_flag_i: bool,
    pub cpu_flag_d: bool,
    pub cpu_flag_v: bool,
    pub cpu_flag_n: bool,
    pub cpu_cycles: u64,
    pub cpu_cycle_count: u32,
    // PPU
    pub ppu_ctrl: u8,
    pub ppu_mask: u8,
    pub ppu_status: u8,
    pub ppu_scanline: u16,
    pub ppu_cycle: u16,
    pub ppu_dot: u32,
    // Frame
    pub frame_count: u64,
    pub frame_hash: u64,
    // RAM snapshot
    pub ram: Option<[u8; 0x800]>,
}

impl Default for DebugData {
    fn default() -> Self {
        Self {
            cpu_a: 0,
            cpu_x: 0,
            cpu_y: 0,
            cpu_sp: 0xFD,
            cpu_pc: 0,
            cpu_flag_c: false,
            cpu_flag_z: false,
            cpu_flag_i: false,
            cpu_flag_d: false,
            cpu_flag_v: false,
            cpu_flag_n: false,
            cpu_cycles: 0,
            cpu_cycle_count: 0,
            ppu_ctrl: 0,
            ppu_mask: 0,
            ppu_status: 0xA0,
            ppu_scanline: 0,
            ppu_cycle: 0,
            ppu_dot: 0,
            frame_count: 0,
            frame_hash: 0,
            ram: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Input mapping persistence (RON)
// ---------------------------------------------------------------------------

/// Persisted key mapping for both ports.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputMappings {
    pub port1: HashMap<NesButton, String>,
    pub port2: HashMap<NesButton, String>,
}

impl Default for InputMappings {
    fn default() -> Self {
        let mut port1 = HashMap::new();
        let mut port2 = HashMap::new();

        // Port 1 defaults (common emulator layout)
        port1.insert(NesButton::A, "Z".into());
        port1.insert(NesButton::B, "X".into());
        port1.insert(NesButton::Select, "RightShift".into());
        port1.insert(NesButton::Start, "Enter".into());
        port1.insert(NesButton::Up, "ArrowUp".into());
        port1.insert(NesButton::Down, "ArrowDown".into());
        port1.insert(NesButton::Left, "ArrowLeft".into());
        port1.insert(NesButton::Right, "ArrowRight".into());

        // Port 2 defaults
        port2.insert(NesButton::A, "Numpad1".into());
        port2.insert(NesButton::B, "Numpad2".into());
        port2.insert(NesButton::Select, "Numpad3".into());
        port2.insert(NesButton::Start, "Numpad0".into());
        port2.insert(NesButton::Up, "T".into());
        port2.insert(NesButton::Down, "G".into());
        port2.insert(NesButton::Left, "F".into());
        port2.insert(NesButton::Right, "H".into());

        InputMappings { port1, port2 }
    }
}

impl InputMappings {
    /// Load from a RON file path, falling back to defaults.
    pub fn load(path: &str) -> Self {
        match std::fs::read_to_string(path) {
            Ok(contents) => match ron::de::from_str::<InputMappings>(&contents) {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!("Failed to parse input mappings: {}, using defaults", e);
                    InputMappings::default()
                }
            },
            Err(_) => InputMappings::default(),
        }
    }

    /// Save to a RON file path.
    pub fn save(&self, path: &str) {
        match ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()) {
            Ok(text) => {
                if let Err(e) = std::fs::write(path, text) {
                    tracing::error!("Failed to write input mappings to {}: {}", path, e);
                }
            }
            Err(e) => {
                tracing::error!("Failed to serialize input mappings: {}", e);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Debug UI state
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct DebugUiState {
    // Panel visibility
    pub show_cpu: bool,
    pub show_ram: bool,
    pub show_ppu: bool,
    pub show_input: bool,
    pub show_frame_hash: bool,
    pub show_game_state: bool,

    // Emulation control
    pub pause_emulation: bool,
    pub step_frame: bool,

    // RAM viewer state
    pub ram_view_start: usize,  // scroll offset in the hex dump
    pub ram_search_addr: String, // text field for address search
    pub ram_highlight_dirty: bool, // highlight bytes changed since last frame

    // FPS tracking
    pub fps: f64,
    pub last_frame_time: Option<Instant>,
}

impl Default for DebugUiState {
    fn default() -> Self {
        Self {
            show_cpu: true,
            show_ram: true,
            show_ppu: true,
            show_input: true,
            show_frame_hash: true,
            show_game_state: true,
            pause_emulation: false,
            step_frame: false,
            ram_view_start: 0,
            ram_search_addr: String::new(),
            ram_highlight_dirty: true,
            fps: 0.0,
            last_frame_time: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Rebind state machine
// ---------------------------------------------------------------------------

/// Which port+button is waiting for a key press.
#[derive(Debug, Clone)]
struct RebindState {
    port: u8,
    button: NesButton,
}

// ---------------------------------------------------------------------------
// DebugOverlay
// ---------------------------------------------------------------------------

pub struct DebugOverlay {
    pub state: DebugUiState,

    /// Latest NES state snapshot (None if not yet received).
    pub nes_data: Option<DebugData>,

    /// Input key mappings, persisted via RON.
    pub input_mappings: InputMappings,

    /// Path to the RON file for saving/loading mappings.
    pub input_mapping_path: String,

    /// If set, we are waiting for the next key press to rebind this button.
    rebind_state: Option<RebindState>,

    /// Buffer of the previous RAM snapshot to highlight changes.
    previous_ram: Option<[u8; 0x800]>,

    /// FPS smoothed value.
    fps_smooth: f64,

    /// Count frames for periodic FPS update.
    frame_acc: u64,
    fps_timer: Instant,
}

impl DebugOverlay {
    pub fn new() -> Self {
        Self {
            state: DebugUiState::default(),
            nes_data: None,
            input_mappings: InputMappings::default(),
            input_mapping_path: "input_mappings.ron".into(),
            rebind_state: None,
            previous_ram: None,
            fps_smooth: 0.0,
            frame_acc: 0,
            fps_timer: Instant::now(),
        }
    }

    /// Create with a custom mapping file path.
    pub fn with_mapping_path(mut self, path: impl Into<String>) -> Self {
        let p = path.into();
        self.input_mappings = InputMappings::load(&p);
        self.input_mapping_path = p;
        self
    }

    // ------------------------------------------------------------------
    // Public hooks
    // ------------------------------------------------------------------

    /// Feed the latest NES state into the debug UI (call once per frame).
    pub fn update_nes_state(&mut self, data: DebugData) {
        self.nes_data = Some(data);
    }

    /// Main render entry point -- call after wgpu render pass.
    pub fn render(&mut self, ctx: &egui::Context) {
        // Update FPS
        self.update_fps();

        // Process pending key events for rebind
        self.process_rebind(ctx);

        // Top panel: frame controls
        self.render_top_panel(ctx);

        // Left panel: CPU / PPU state
        self.render_left_panel(ctx);

        // Right panel: input mapping
        self.render_right_panel(ctx);

        // Bottom panel: RAM viewer
        self.render_bottom_panel(ctx);
    }

    // ------------------------------------------------------------------
    // FPS
    // ------------------------------------------------------------------

    fn update_fps(&mut self) {
        self.frame_acc += 1;
        let elapsed = self.fps_timer.elapsed().as_secs_f64();
        if elapsed >= 1.0 {
            self.fps_smooth = self.frame_acc as f64 / elapsed;
            self.state.fps = self.fps_smooth;
            self.frame_acc = 0;
            self.fps_timer = Instant::now();
        }
    }

    // ------------------------------------------------------------------
    // Key rebind processing
    // ------------------------------------------------------------------

    fn process_rebind(&mut self, ctx: &egui::Context) {
        // Snapshot the current rebind state so we can release the borrow.
        let rebind_snapshot = self.rebind_state.clone();
        let Some(ref rebind) = rebind_snapshot else {
            return;
        };

        let mut captured_key: Option<String> = None;
        let mut cancelled = false;

        ctx.input(|i| {
            for event in &i.events {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    ..
                } = event
                {
                    let name = key.name().to_string();
                    if name == "Escape" {
                        cancelled = true;
                    } else {
                        captured_key = Some(name);
                    }
                    break;
                }
            }
        });

        if cancelled {
            self.rebind_state = None;
        } else if let Some(key_name) = captured_key {
            self.set_binding(rebind.port, rebind.button, &key_name);
            self.rebind_state = None;
        }
    }

    fn set_binding(&mut self, port: u8, button: NesButton, key_name: &str) {
        let map = if port == 1 {
            &mut self.input_mappings.port1
        } else {
            &mut self.input_mappings.port2
        };
        map.insert(button, key_name.to_string());
    }

    fn get_binding(&self, port: u8, button: NesButton) -> &str {
        let map = if port == 1 {
            &self.input_mappings.port1
        } else {
            &self.input_mappings.port2
        };
        map.get(&button).map(|s| s.as_str()).unwrap_or("---")
    }

    // ------------------------------------------------------------------
    // Panel renderers
    // ------------------------------------------------------------------

    /// Top panel: frame controls, FPS, pause/step.
    fn render_top_panel(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("frame_controls").show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Pause / Resume
                let pause_label = if self.state.pause_emulation {
                    "Resume"
                } else {
                    "Pause"
                };
                if ui.button(pause_label).clicked() {
                    self.state.pause_emulation = !self.state.pause_emulation;
                }

                // Step frame (only when paused)
                if self.state.pause_emulation {
                    if ui.button("Step Frame").clicked() {
                        self.state.step_frame = true;
                    }
                }

                ui.separator();

                // FPS display
                ui.label(format!("FPS: {:.1}", self.fps_smooth));

                ui.separator();

                // Frame info
                if let Some(ref data) = self.nes_data {
                    ui.label(format!("Frame: {}", data.frame_count));

                    if self.state.show_frame_hash {
                        ui.label(format!("Hash: {:016X}", data.frame_hash));
                    }
                } else {
                    ui.label("Frame: ---");
                }

                // Checkboxes on the right
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.checkbox(&mut self.state.show_frame_hash, "Hash");
                    ui.checkbox(&mut self.state.show_input, "Input");
                    ui.checkbox(&mut self.state.show_ram, "RAM");
                    ui.checkbox(&mut self.state.show_game_state, "State");
                    ui.checkbox(&mut self.state.show_ppu, "PPU");
                    ui.checkbox(&mut self.state.show_cpu, "CPU");
                });
            });
        });
    }

    /// Left panel: CPU registers + flags + PPU state.
    fn render_left_panel(&mut self, ctx: &egui::Context) {
        if !self.state.show_cpu && !self.state.show_ppu {
            return;
        }

        egui::SidePanel::left("debug_panel")
            .default_width(220.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("NES Debug");

                if let Some(ref data) = self.nes_data {
                    if self.state.show_cpu {
                        ui.collapsing("CPU Registers", |ui| {
                            ui.monospace(format!("A:  {:02X}", data.cpu_a));
                            ui.monospace(format!("X:  {:02X}", data.cpu_x));
                            ui.monospace(format!("Y:  {:02X}", data.cpu_y));
                            ui.monospace(format!("SP: {:02X}", data.cpu_sp));
                            ui.monospace(format!("PC: {:04X}", data.cpu_pc));
                            ui.monospace(format!("Cycles: {}", data.cpu_cycles));
                        });

                        ui.collapsing("CPU Flags", |ui| {
                            ui.horizontal(|ui| {
                                self.flag(ui, "C", data.cpu_flag_c);
                                self.flag(ui, "Z", data.cpu_flag_z);
                                self.flag(ui, "I", data.cpu_flag_i);
                            });
                            ui.horizontal(|ui| {
                                self.flag(ui, "D", data.cpu_flag_d);
                                self.flag(ui, "V", data.cpu_flag_v);
                                self.flag(ui, "N", data.cpu_flag_n);
                            });
                        });
                    }

                    if self.state.show_ppu {
                        ui.collapsing("PPU State", |ui| {
                            ui.monospace(format!("CTRL:   {:02X}", data.ppu_ctrl));
                            ui.monospace(format!("MASK:   {:02X}", data.ppu_mask));
                            ui.monospace(format!("STATUS: {:02X}", data.ppu_status));
                            ui.separator();
                            ui.monospace(format!(
                                "Scanline: {}",
                                data.ppu_scanline
                            ));
                            ui.monospace(format!("Cycle:    {}", data.ppu_cycle));
                            ui.monospace(format!("Dot:      {}", data.ppu_dot));
                        });
                    }

                    if self.state.show_ppu {
                        ui.collapsing("PPU Decoded", |ui| {
                            let ctrl = data.ppu_ctrl;
                            ui.monospace(format!(
                                "NameTable: {}",
                                match ctrl & 0x03 {
                                    0 => "0 ($2000)",
                                    1 => "1 ($2400)",
                                    2 => "2 ($2800)",
                                    3 => "3 ($2C00)",
                                    _ => "?",
                                }
                            ));
                            ui.monospace(format!(
                                "VRAM Inc:  {}",
                                if ctrl & 0x04 != 0 { "32" } else { "1" }
                            ));
                            ui.monospace(format!(
                                "Spr PTable: {}",
                                if ctrl & 0x08 != 0 { "$1000" } else { "$0000" }
                            ));
                            ui.monospace(format!(
                                "BG PTable:  {}",
                                if ctrl & 0x10 != 0 { "$1000" } else { "$0000" }
                            ));
                            ui.monospace(format!(
                                "Spr Size:  {}",
                                if ctrl & 0x20 != 0 { "8x16" } else { "8x8" }
                            ));
                            ui.monospace(format!(
                                "NMI:       {}",
                                if ctrl & 0x80 != 0 {
                                    "enabled"
                                } else {
                                    "disabled"
                                }
                            ));

                            let mask = data.ppu_mask;
                            ui.separator();
                            ui.monospace(format!(
                                "Grayscale: {}",
                                if mask & 0x01 != 0 {
                                    "on"
                                } else {
                                    "off"
                                }
                            ));
                            ui.monospace(format!(
                                "Show BG left:  {}",
                                if mask & 0x02 != 0 { "yes" } else { "no" }
                            ));
                            ui.monospace(format!(
                                "Show Spr left: {}",
                                if mask & 0x04 != 0 { "yes" } else { "no" }
                            ));
                            ui.monospace(format!(
                                "BG visible:    {}",
                                if mask & 0x08 != 0 { "yes" } else { "no" }
                            ));
                            ui.monospace(format!(
                                "Spr visible:   {}",
                                if mask & 0x10 != 0 { "yes" } else { "no" }
                            ));
                            ui.monospace(format!(
                                "Emph R/G/B:    {}/{}/{}",
                                if mask & 0x20 != 0 { "1" } else { "0" },
                                if mask & 0x40 != 0 { "1" } else { "0" },
                                if mask & 0x80 != 0 { "1" } else { "0" },
                            ));

                            let status = data.ppu_status;
                            ui.separator();
                            ui.monospace(format!(
                                "VBlank:     {}",
                                if status & 0x80 != 0 {
                                    "active"
                                } else {
                                    "inactive"
                                }
                            ));
                            ui.monospace(format!(
                                "Spr0 Hit:  {}",
                                if status & 0x40 != 0 { "yes" } else { "no" }
                            ));
                            ui.monospace(format!(
                                "Spr Ovrflw: {}",
                                if status & 0x20 != 0 { "yes" } else { "no" }
                            ));
                        });
                    }
                    if self.state.show_game_state {
                        if let Some(ref data) = self.nes_data {
                            if let Some(ref ram) = data.ram {
                                ui.collapsing("Game State (Battle City)", |ui| {
                                    let mode = ram[0x0078];
                                    let mode_str = match mode {
                                        0 => "Title Screen",
                                        1 => "Playing",
                                        2 => "Game Over",
                                        _ => "Unknown",
                                    };
                                    ui.monospace(format!("Mode:       {} ($0078={})", mode_str, mode));
                                    ui.monospace(format!("Lives:      {} ($0051)", ram[0x0051]));
                                    ui.monospace(format!("Stage:      {} ($0085)", ram[0x0085]));
                                    ui.separator();
                                    ui.monospace(format!("Player X:   {} ($00A6)", ram[0x00A6]));
                                    ui.monospace(format!("Player Y:   {} ($00A7)", ram[0x00A7]));
                                    ui.monospace(format!("Tank State: {:02X} ($00A8)", ram[0x00A8]));
                                    ui.monospace(format!("Shield:     {} ($0089)", ram[0x0089]));
                                    ui.separator();
                                    ui.monospace(format!("Enemies:    {} ($00A1)", ram[0x00A1]));
                                    ui.monospace(format!("Block Type: {:02X} ($005C)", ram[0x005C]));
                                    ui.monospace(format!("Power Cnt:  {} ($0019)", ram[0x0019]));
                                    ui.monospace(format!("Power Pos:  {} ($0086)", ram[0x0086]));
                                    ui.monospace(format!("Power Sts:  {:02X} ($0049)", ram[0x0049]));
                                });
                            }
                        }
                    }
                } else {
                    ui.label("(no NES data)");
                }
            });
    }

    /// Render a single CPU flag as a colored indicator.
    fn flag(&self, ui: &mut egui::Ui, name: &str, set: bool) {
        let (color, text) = if set {
            (Color32::from_rgb(0, 200, 0), "1")
        } else {
            (Color32::from_rgb(80, 80, 80), "0")
        };
        ui.label(
            egui::RichText::new(format!("{name}:{text}"))
                .color(color)
                .monospace(),
        );
    }

    // ------------------------------------------------------------------
    // Right panel: Input Mapping Editor
    // ------------------------------------------------------------------

    fn render_right_panel(&mut self, ctx: &egui::Context) {
        if !self.state.show_input {
            return;
        }

        let rebinding = self.rebind_state.is_some();

        egui::SidePanel::right("input_panel")
            .default_width(260.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("Input Mapping");

                if rebinding {
                    ui.colored_label(
                        Color32::from_rgb(255, 200, 50),
                        "Listening for key... (Esc to cancel)",
                    );
                    ui.separator();
                }

                // Port 1
                ui.collapsing("Port 1", |ui| {
                    self.render_port_table(ui, 1);
                });

                // Port 2
                ui.collapsing("Port 2", |ui| {
                    self.render_port_table(ui, 2);
                });

                ui.separator();

                // Save / Load / Reset buttons
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        self.input_mappings.save(&self.input_mapping_path);
                    }
                    if ui.button("Load").clicked() {
                        self.input_mappings =
                            InputMappings::load(&self.input_mapping_path);
                    }
                    if ui.button("Reset Defaults").clicked() {
                        self.input_mappings = InputMappings::default();
                    }
                });

                ui.label(format!("File: {}", self.input_mapping_path));
            });
    }

    fn render_port_table(&mut self, ui: &mut egui::Ui, port: u8) {
        egui::Grid::new(format!("port_{}_grid", port))
            .num_columns(3)
            .striped(true)
            .show(ui, |ui| {
                ui.label("Button");
                ui.label("Binding");
                ui.label("Action");
                ui.end_row();

                for &button in &NesButton::ALL {
                    let binding = self.get_binding(port, button).to_string();
                    let is_rebinding = self
                        .rebind_state
                        .as_ref()
                        .map_or(false, |r| r.port == port && r.button == button);

                    ui.label(button.name());

                    let key_display = if is_rebinding {
                        egui::RichText::new("...").color(Color32::from_rgb(255, 200, 50))
                    } else if binding == "---" {
                        egui::RichText::new("---").color(Color32::from_rgb(128, 128, 128))
                    } else {
                        egui::RichText::new(&binding).monospace()
                    };
                    ui.label(key_display);

                    if ui.button(if is_rebinding { "Cancel" } else { "Rebind" }).clicked() {
                        if is_rebinding {
                            self.rebind_state = None;
                        } else {
                            self.rebind_state = Some(RebindState {
                                port,
                                button,
                            });
                        }
                    }

                    ui.end_row();
                }
            });
    }

    // ------------------------------------------------------------------
    // Bottom panel: RAM viewer
    // ------------------------------------------------------------------

    fn render_bottom_panel(&mut self, ctx: &egui::Context) {
        if !self.state.show_ram {
            return;
        }

        egui::TopBottomPanel::bottom("ram_panel")
            .default_height(200.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("RAM Viewer");
                    ui.separator();

                    // Address search
                    ui.label("Go to: $");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.state.ram_search_addr)
                            .desired_width(60.0)
                            .hint_text("0000"),
                    );
                    if ui.button("Go").clicked() {
                        if let Ok(addr) =
                            u16::from_str_radix(&self.state.ram_search_addr, 16)
                        {
                            self.state.ram_view_start =
                                ((addr as usize) & 0x07FF) / 16 * 16;
                        }
                    }

                    ui.checkbox(
                        &mut self.state.ram_highlight_dirty,
                        "Highlight changes",
                    );
                });

                // Hex dump
                self.render_hex_dump(ui);
            });
    }

    fn render_hex_dump(&mut self, ui: &mut egui::Ui) {
        // Get current RAM
        let ram = match &self.nes_data {
            Some(data) => data.ram.as_ref(),
            None => {
                ui.label("(no RAM data)");
                return;
            }
        };

        let ram = match ram {
            Some(r) => r,
            None => {
                ui.label("(no RAM data)");
                return;
            }
        };

        // Clamp view start
        let start = self.state.ram_view_start.min(0x7F0) & !0xF; // align to row

        let rows_to_show = 32usize; // show at most 32 rows in the panel
        let end = (start + rows_to_show * 16).min(0x800);

        // Two-column layout: hex on left, ASCII on right
        egui::ScrollArea::vertical()
            .id_salt("ram_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Previous RAM for dirty highlighting
                let prev = self.previous_ram.as_ref();

                for row_addr in (start..end).step_by(16) {
                    // Address
                    ui.monospace(format!("{:04X}: ", row_addr));

                    // Hex bytes
                    for col in 0..16 {
                        let addr = row_addr + col;
                        if addr >= 0x800 {
                            break;
                        }
                        let byte = ram[addr];

                        let changed = self.state.ram_highlight_dirty
                            && prev.map_or(false, |p| p[addr] != byte);

                        let text = if changed {
                            egui::RichText::new(format!("{:02X}", byte))
                                .color(Color32::from_rgb(255, 100, 100))
                        } else {
                            egui::RichText::new(format!("{:02X}", byte))
                        };
                        ui.add(egui::Label::new(text).wrap());
                        if col == 7 {
                            ui.add(egui::Label::new(" ").wrap()); // gap
                        }
                    }

                    ui.add(egui::Label::new(" ").wrap()); // gap before ASCII

                    // ASCII representation
                    for col in 0..16 {
                        let addr = row_addr + col;
                        if addr >= 0x800 {
                            break;
                        }
                        let byte = ram[addr];
                        let ch = if byte >= 0x20 && byte < 0x7F {
                            byte as char
                        } else {
                            '.'
                        };
                        let changed = self.state.ram_highlight_dirty
                            && prev.map_or(false, |p| p[addr] != byte);
                        let text = if changed {
                            egui::RichText::new(format!("{}", ch))
                                .color(Color32::from_rgb(255, 100, 100))
                        } else {
                            egui::RichText::new(format!("{}", ch))
                                .color(Color32::from_rgb(150, 150, 150))
                        };
                        ui.add(egui::Label::new(text).wrap());
                    }

                    ui.end_row();
                }
            });

        // Navigation buttons
        ui.horizontal(|ui| {
            if ui.button("Page Up").clicked() && start >= 16 * 16 {
                self.state.ram_view_start = start.saturating_sub(16 * 16);
            }
            if ui.button("Page Down").clicked() {
                let new_start = start + 16 * 16;
                if new_start < 0x800 {
                    self.state.ram_view_start = new_start;
                }
            }
            if ui.button("Top ($0000)").clicked() {
                self.state.ram_view_start = 0;
            }
            if ui.button("Stack ($0100)").clicked() {
                self.state.ram_view_start = 0x0100;
            }

            ui.label(format!(
                "Showing ${:04X}-${:04X}",
                start,
                (end - 1).min(0x7FF)
            ));
        });

        // Save previous RAM for next frame's dirty detection
        self.previous_ram = Some(*ram);
    }
}

impl Default for DebugOverlay {
    fn default() -> Self {
        Self::new()
    }
}
