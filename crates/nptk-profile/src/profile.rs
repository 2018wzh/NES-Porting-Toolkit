use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// GameProfile – top-level profile for a single game
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameProfile {
    pub game: GameSection,
    pub rom: RomSection,
    pub cpu: CpuSection,
    pub ppu: PpuSection,
    pub audio: AudioSection,
    pub input: InputSection,
    pub testing: TestingSection,
}

// ---------------------------------------------------------------------------
// Game section
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameSection {
    pub id: String,
    pub display_name: String,
    pub region: Option<String>,
    pub default_mode: RunMode,
}

impl Default for GameSection {
    fn default() -> Self {
        Self {
            id: String::new(),
            display_name: String::new(),
            region: None,
            default_mode: RunMode::Auto,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum RunMode {
    CompatInterpreter,
    RecompiledCompat,
    NativePort,
    #[default]
    Auto,
}

// ---------------------------------------------------------------------------
// ROM section
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RomSection {
    pub system: String,
    pub accepted_format: Vec<String>,
    pub mapper: u16,
    pub mapper_name: Option<String>,
    pub mirroring: String,
    pub prg_size: usize,
    pub chr_size: usize,
    pub has_sram: bool,
    pub known_dump: Vec<KnownDump>,
}

impl Default for RomSection {
    fn default() -> Self {
        Self {
            system: String::from("NES"),
            accepted_format: vec![String::from("nes")],
            mapper: 0,
            mapper_name: None,
            mirroring: String::from("horizontal"),
            prg_size: 0,
            chr_size: 0,
            has_sram: false,
            known_dump: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownDump {
    pub name: String,
    pub prg_crc32: Option<String>,
    pub chr_crc32: Option<String>,
    pub combined_crc32: Option<String>,
}

// ---------------------------------------------------------------------------
// CPU section
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuSection {
    pub reset_vector: Option<String>,
    pub nmi_vector: Option<String>,
    pub irq_vector: Option<String>,
    pub allow_decimal_mode: bool,
    pub unknown_indirect_jump: String,
}

impl Default for CpuSection {
    fn default() -> Self {
        Self {
            reset_vector: None,
            nmi_vector: None,
            irq_vector: None,
            allow_decimal_mode: true,
            unknown_indirect_jump: String::from("trap"),
        }
    }
}

// ---------------------------------------------------------------------------
// PPU section
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PpuSection {
    pub initial_mode: String,
    pub native_mode: String,
    pub sprite_source: String,
    pub background_source: String,
    pub chr_export: bool,
    pub palette_policy: String,
}

impl Default for PpuSection {
    fn default() -> Self {
        Self {
            initial_mode: String::from("interpreter"),
            native_mode: String::from("native"),
            sprite_source: String::from("all"),
            background_source: String::from("all"),
            chr_export: true,
            palette_policy: String::from("exact"),
        }
    }
}

// ---------------------------------------------------------------------------
// Audio section
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSection {
    pub initial_mode: String,
    pub native_sfx: String,
    pub native_bgm: String,
}

impl Default for AudioSection {
    fn default() -> Self {
        Self {
            initial_mode: String::from("interpreter"),
            native_sfx: String::from("none"),
            native_bgm: String::from("none"),
        }
    }
}

// ---------------------------------------------------------------------------
// Input section
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputSection {
    pub controller_ports: u8,
    pub default_backend_policy: String,
    pub default_port_1: String,
    pub input_profile: Option<String>,
}

impl Default for InputSection {
    fn default() -> Self {
        Self {
            controller_ports: 2,
            default_backend_policy: String::from("auto"),
            default_port_1: String::from("nes_standard"),
            input_profile: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Testing section
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestingSection {
    pub enable_trace_compare: bool,
    pub enable_golden_frames: bool,
    pub enable_input_replay: bool,
}

impl Default for TestingSection {
    fn default() -> Self {
        Self {
            enable_trace_compare: false,
            enable_golden_frames: false,
            enable_input_replay: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Serde helpers for TOML loading
// ---------------------------------------------------------------------------

/// Deserialize a `GameProfile` from a TOML file on disk.
pub fn load_profile(path: &str) -> Result<GameProfile, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)?;
    let profile: GameProfile = toml::from_str(&content)?;
    Ok(profile)
}

/// Deserialize a `GameProfile` from a TOML string.
pub fn load_profile_from_str(toml_str: &str) -> Result<GameProfile, Box<dyn std::error::Error>> {
    let profile: GameProfile = toml::from_str(toml_str)?;
    Ok(profile)
}
