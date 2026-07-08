//! Data types shared between the NES system and the debug UI.
//!
//! These types are extracted from `nptk-wgpu::debug_ui` so that the
//! FLTK-based debug window can use them without depending on egui/wgpu.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// NES button identifiers
// ---------------------------------------------------------------------------

/// NES controller buttons (8 discrete buttons per port).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
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
    pub ram_view_start: usize,     // scroll offset in the hex dump
    pub ram_search_addr: String,   // text field for address search
    pub ram_highlight_dirty: bool, // highlight bytes changed since last frame

    // FPS tracking
    pub fps: f64,
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
        }
    }
}

// ---------------------------------------------------------------------------
// Commands sent from the main thread to the FLTK debug window thread
// ---------------------------------------------------------------------------

/// Commands sent from the main (winit) thread to the FLTK debug window.
pub enum DebugCommand {
    /// Update the debug display with fresh NES state.
    Update(DebugData),
    /// Gracefully shut down the FLTK window thread.
    Shutdown,
}

/// Events sent from the FLTK debug window thread back to the main thread.
pub enum DebugEvent {
    /// The FLTK window was closed by the user.
    WindowClosed,
    /// The user modified input mappings (saved to file).
    InputMappingsChanged(InputMappings),
}
